use super::{
    ExtractionBackend, FileTask, ScanCheckpoint, ScanControl, ScanEvent, ScanOptions, ScanRoot,
    ScanStore, ScanSummary, StopReason, codec, pipeline::run_pipeline, walk::invalid,
};
use rusqlite::{OptionalExtension, Transaction, params};
use std::{io, sync::Arc};
pub(crate) trait ScanJournal: Send + Sync {
    fn schedule(&self, task: FileTask) -> io::Result<Option<FileTask>>;
    fn pending(&self, after: u64, limit: usize) -> io::Result<Vec<FileTask>>;
    fn discovery_done(&self) -> io::Result<bool>;
    fn finish_discovery(&self) -> io::Result<()>;
    fn started(&self, task: &FileTask) -> io::Result<()>;
    fn persist_batch(&self, _events: &[&ScanEvent]) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) struct NoJournal;
impl ScanJournal for NoJournal {
    fn schedule(&self, task: FileTask) -> io::Result<Option<FileTask>> {
        Ok(Some(task))
    }
    fn pending(&self, _after: u64, _limit: usize) -> io::Result<Vec<FileTask>> {
        Ok(Vec::new())
    }
    fn discovery_done(&self) -> io::Result<bool> {
        Ok(false)
    }
    fn finish_discovery(&self) -> io::Result<()> {
        Ok(())
    }
    fn started(&self, _task: &FileTask) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) struct RunJournal {
    store: Arc<ScanStore>,
    run: u64,
    generation: u64,
    max_files: u64,
}
impl RunJournal {
    pub(crate) fn new(
        store: Arc<ScanStore>,
        run: u64,
        generation: u64,
        options: &ScanOptions,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            run,
            generation,
            max_files: options.walk.max_files,
        })
    }
}
pub(crate) fn check_generation(tx: &Transaction<'_>, run: u64, generation: u64) -> io::Result<()> {
    let current: Option<u64> = tx
        .query_row("SELECT generation FROM scan_runs WHERE id=?1", [run], |r| {
            r.get(0)
        })
        .optional()
        .map_err(io::Error::other)?;
    if current != Some(generation) {
        return Err(invalid(
            "scan invocation was superseded; stale writes are rejected",
        ));
    }
    Ok(())
}
impl ScanJournal for RunJournal {
    fn persist_batch(&self, events: &[&ScanEvent]) -> io::Result<()> {
        self.store.persist_events(self.run, self.generation, events)
    }
    fn schedule(&self, mut task: FileTask) -> io::Result<Option<FileTask>> {
        let mut conn = self
            .store
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        check_generation(&tx, self.run, self.generation)?;
        let relative = codec::encode(&task.relative, 1024 * 1024)?;
        let existing: Option<(Vec<u8>, String)> = tx
            .query_row(
                "SELECT task,state FROM scan_jobs WHERE run=?1 AND relative=?2",
                params![self.run, &relative],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(io::Error::other)?;
        if let Some((data, state)) = existing {
            if state != "pending" {
                return Ok(None);
            }
            task = codec::decode(&data, 1024 * 1024)?;
            tx.execute(
                "UPDATE scan_jobs SET state='queued' WHERE run=?1 AND id=?2",
                params![self.run, task.id],
            )
            .map_err(io::Error::other)?;
        } else {
            let count: u64 = tx
                .query_row(
                    "SELECT count(*) FROM scan_jobs WHERE run=?1",
                    [self.run],
                    |r| r.get(0),
                )
                .map_err(io::Error::other)?;
            if count >= self.max_files {
                return Err(invalid("durable scan file limit reached"));
            }
            task.id = tx
                .query_row(
                    "SELECT coalesce(max(id),0)+1 FROM scan_jobs WHERE run=?1",
                    [self.run],
                    |r| r.get(0),
                )
                .map_err(io::Error::other)?;
            tx.execute(
                "INSERT INTO scan_jobs(run,id,relative,task,state) VALUES(?1,?2,?3,?4,'queued')",
                params![
                    self.run,
                    task.id,
                    relative,
                    codec::encode(&task, 1024 * 1024)?
                ],
            )
            .map_err(io::Error::other)?;
        }
        tx.commit().map_err(io::Error::other)?;
        Ok(Some(task))
    }
    fn pending(&self, after: u64, limit: usize) -> io::Result<Vec<FileTask>> {
        let mut conn = self
            .store
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        check_generation(&tx, self.run, self.generation)?;
        let mut out = Vec::new();
        {
            let mut query=tx.prepare("SELECT task FROM scan_jobs WHERE run=?1 AND id>?2 AND state='pending' ORDER BY id LIMIT ?3").map_err(io::Error::other)?;
            let rows = query
                .query_map(params![self.run, after, limit as i64], |r| {
                    r.get::<_, Vec<u8>>(0)
                })
                .map_err(io::Error::other)?;
            for row in rows {
                out.push(codec::decode::<FileTask>(
                    &row.map_err(io::Error::other)?,
                    1024 * 1024,
                )?);
            }
        }
        for task in &out {
            tx.execute(
                "UPDATE scan_jobs SET state='queued' WHERE run=?1 AND id=?2",
                params![self.run, task.id],
            )
            .map_err(io::Error::other)?;
        }
        tx.commit().map_err(io::Error::other)?;
        Ok(out)
    }
    fn discovery_done(&self) -> io::Result<bool> {
        let checkpoint = self.store.checkpoint(self.run)?;
        if checkpoint.generation != self.generation {
            return Err(invalid("stale discovery invocation"));
        }
        Ok(checkpoint.discovery_done)
    }
    fn finish_discovery(&self) -> io::Result<()> {
        let mut conn = self
            .store
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        check_generation(&tx, self.run, self.generation)?;
        tx.execute(
            "UPDATE scan_runs SET discovery_done=1 WHERE id=?1",
            [self.run],
        )
        .map_err(io::Error::other)?;
        tx.commit().map_err(io::Error::other)
    }
    fn started(&self, task: &FileTask) -> io::Result<()> {
        let mut conn = self
            .store
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        check_generation(&tx, self.run, self.generation)?;
        if tx
            .execute(
                "UPDATE scan_jobs SET state='running' WHERE run=?1 AND id=?2 AND state='queued'",
                params![self.run, task.id],
            )
            .map_err(io::Error::other)?
            != 1
        {
            return Err(invalid("task is not queued"));
        }
        tx.commit().map_err(io::Error::other)
    }
}
impl ScanStore {
    pub fn checkpoint(&self, run: u64) -> io::Result<ScanCheckpoint> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        conn.query_row("SELECT generation,root,root_stamp,options,engine,status,discovery_done FROM scan_runs WHERE id=?1",[run],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,bool>(6)?))).map_err(io::Error::other).and_then(|(generation,root,stamp,options,engine,status,discovery_done)|Ok(ScanCheckpoint{run,generation,root:codec::decode(&root,1024*1024)?,root_stamp:codec::decode(&stamp,4096)?,options:codec::decode(&options,16*1024*1024)?,engine,status,discovery_done}))
    }
    pub(crate) fn summary(&self, run: u64, stopped: Option<StopReason>) -> io::Result<ScanSummary> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let (completed,failed,unstable,bytes_hashed)=conn.query_row("SELECT count(*),coalesce(sum(failed),0),coalesce(sum(unstable),0),coalesce(sum(hashed_bytes),0) FROM scan_results WHERE run=?1",[run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(io::Error::other)?;
        let discovered = conn
            .query_row("SELECT count(*) FROM scan_jobs WHERE run=?1", [run], |r| {
                r.get(0)
            })
            .map_err(io::Error::other)?;
        let notices = conn
            .query_row(
                "SELECT count(*) FROM scan_notices WHERE run=?1",
                [run],
                |r| r.get(0),
            )
            .map_err(io::Error::other)?;
        Ok(ScanSummary {
            discovered,
            completed,
            failed,
            unstable,
            notices,
            bytes_hashed,
            stopped,
        })
    }
    pub(crate) fn fail_generation(&self, run: u64, generation: u64) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "UPDATE scan_runs SET status='failed' WHERE id=?1 AND generation=?2",
                params![run, generation],
            );
        }
    }
    pub(crate) fn dispatch(
        &self,
        run: u64,
        generation: u64,
        mut event: ScanEvent,
    ) -> io::Result<ScanEvent> {
        if self.checkpoint(run)?.generation != generation {
            return Err(invalid("stale scan event"));
        }
        if let ScanEvent::Finished(summary) = &mut event {
            *summary = self.summary(run, summary.stopped)?;
        }
        if !matches!(
            event,
            ScanEvent::Completed(_)
                | ScanEvent::Discovered(_)
                | ScanEvent::Resumed(_)
                | ScanEvent::Started(_)
                | ScanEvent::DiscoveryFinished
        ) {
            self.persist_event(run, generation, &event)?;
        }
        Ok(event)
    }
    pub fn resume(
        self: &Arc<Self>,
        run: u64,
        root: Arc<ScanRoot>,
        backend: Arc<dyn ExtractionBackend>,
        control: ScanControl,
        mut sink: impl FnMut(u64, ScanEvent) -> io::Result<()>,
    ) -> io::Result<ScanSummary> {
        let checkpoint = self.checkpoint(run)?;
        if checkpoint.engine != run_fingerprint(backend.as_ref()) {
            return Err(invalid(
                "checkpoint engine or backend differs; start a new scan",
            ));
        }
        if checkpoint.root != root.original
            || checkpoint.root_stamp.device != root.stamp.device
            || checkpoint.root_stamp.file_id != root.stamp.file_id
        {
            return Err(invalid("checkpoint root identity differs"));
        }
        let options = self.options(&root, checkpoint.options, backend.as_ref())?;
        control.checkpoint()?;
        let generation = {
            let mut conn = self
                .conn
                .lock()
                .map_err(|_| io::Error::other("store lock poisoned"))?;
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(io::Error::other)?;
            check_generation(&tx, run, checkpoint.generation)?;
            let generation = checkpoint
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("checkpoint generation overflow"))?;
            tx.execute(
                "UPDATE scan_runs SET generation=?2,status='running' WHERE id=?1",
                params![run, generation],
            )
            .map_err(io::Error::other)?;
            tx.execute("UPDATE scan_jobs SET state='pending' WHERE run=?1 AND state IN ('queued','running')",[run]).map_err(io::Error::other)?;
            tx.commit().map_err(io::Error::other)?;
            generation
        };
        let journal = RunJournal::new(Arc::clone(self), run, generation, &options);
        let result = run_pipeline(root, options, backend, control, journal, |event| {
            sink(run, self.dispatch(run, generation, event)?)
        });
        match result {
            Ok(summary) => self.summary(run, summary.stopped),
            Err(e) => {
                self.fail_generation(run, generation);
                Err(e)
            }
        }
    }
}
pub(crate) fn run_fingerprint(backend: &dyn ExtractionBackend) -> String {
    format!("{}:{}", super::engine_fingerprint(), backend.fingerprint())
}
