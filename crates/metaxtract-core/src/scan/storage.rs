use super::{
    ExtractionBackend, FileStamp, FileTask, NativePath, ScanControl, ScanEvent, ScanOptions,
    ScanRoot, ScanSummary, ScannedFile, StopReason, cache::database_artifacts, codec,
    walk::invalid,
};
use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub struct ScanStore {
    pub(crate) conn: Mutex<rusqlite::Connection>,
    pub(crate) path: PathBuf,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Pending,
    Queued,
    Running,
    Done,
}
#[derive(Clone, Debug)]
pub struct TaskRow {
    pub task: FileTask,
    pub state: TaskState,
}
impl ScanStore {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Arc<Self>> {
        let conn = codec::database(path.as_ref())?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS scan_meta(key TEXT PRIMARY KEY,value INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS scan_runs(id INTEGER PRIMARY KEY,root BLOB NOT NULL,root_stamp BLOB NOT NULL,options BLOB NOT NULL,engine TEXT NOT NULL,status TEXT NOT NULL,discovery_done INTEGER NOT NULL DEFAULT 0,created INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS scan_jobs(run INTEGER NOT NULL,id INTEGER NOT NULL,relative BLOB NOT NULL,task BLOB NOT NULL,state TEXT NOT NULL,PRIMARY KEY(run,id),UNIQUE(run,relative),FOREIGN KEY(run) REFERENCES scan_runs(id));
            CREATE TABLE IF NOT EXISTS scan_results(run INTEGER NOT NULL,id INTEGER NOT NULL,data BLOB NOT NULL,digest BLOB NOT NULL,PRIMARY KEY(run,id),FOREIGN KEY(run,id) REFERENCES scan_jobs(run,id));
            CREATE TABLE IF NOT EXISTS scan_notices(run INTEGER NOT NULL,path BLOB,code TEXT NOT NULL,message TEXT NOT NULL,FOREIGN KEY(run) REFERENCES scan_runs(id));
            INSERT OR IGNORE INTO scan_meta(key,value) VALUES('schema',1);").map_err(io::Error::other)?;
        let version: i64 = conn
            .query_row("SELECT value FROM scan_meta WHERE key='schema'", [], |r| {
                r.get(0)
            })
            .map_err(io::Error::other)?;
        if version == 1 {
            conn.execute_batch("BEGIN IMMEDIATE; ALTER TABLE scan_runs ADD COLUMN generation INTEGER NOT NULL DEFAULT 1; ALTER TABLE scan_results ADD COLUMN failed INTEGER NOT NULL DEFAULT 0; ALTER TABLE scan_results ADD COLUMN unstable INTEGER NOT NULL DEFAULT 0; ALTER TABLE scan_results ADD COLUMN hashed_bytes INTEGER NOT NULL DEFAULT 0; UPDATE scan_results SET failed=(json_extract(data, '$.error') IS NOT NULL OR coalesce(json_extract(data, '$.records[0].status')='Failed',0)),unstable=coalesce(json_extract(data, '$.unstable'),0),hashed_bytes=coalesce(json_extract(data, '$.hashes.bytes_read'),0); UPDATE scan_meta SET value=2 WHERE key='schema'; COMMIT;").map_err(io::Error::other)?;
        } else if version != 2 {
            return Err(invalid("unsupported scan store schema"));
        }
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            path: path.as_ref().canonicalize()?,
        }))
    }
    pub(crate) fn options(
        &self,
        root: &ScanRoot,
        mut options: ScanOptions,
        backend: &dyn ExtractionBackend,
    ) -> io::Result<ScanOptions> {
        for path in database_artifacts(&self.path)
            .into_iter()
            .chain(backend.excluded_paths())
        {
            if root.original.to_path()? == path {
                return Err(invalid("scan output overlaps the selected evidence"));
            }
            if let Some(relative) = root.relative_to_root(&path) {
                if !options.walk.excluded.contains(&relative) {
                    options.walk.excluded.push(relative);
                }
            }
        }
        options.validate()?;
        Ok(options)
    }
    pub fn create_run(&self, root: &ScanRoot, options: &ScanOptions) -> io::Result<u64> {
        self.create_run_with_engine(
            root,
            options,
            &super::checkpoint::run_fingerprint(&super::NativeExtractor),
        )
    }
    pub(crate) fn create_run_with_engine(
        &self,
        root: &ScanRoot,
        options: &ScanOptions,
        engine: &str,
    ) -> io::Result<u64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        conn.execute("INSERT INTO scan_runs(root,root_stamp,options,engine,status,created) VALUES(?1,?2,?3,?4,'running',unixepoch())",params![codec::encode(&root.original,1024*1024)?,codec::encode(&root.stamp,4096)?,codec::encode(options,16*1024*1024)?,engine]).map_err(io::Error::other)?;
        Ok(conn.last_insert_rowid() as u64)
    }
    pub fn record_event(&self, run: u64, event: &ScanEvent) -> io::Result<()> {
        let generation = self.checkpoint(run)?.generation;
        self.persist_event(run, generation, event)
    }
    pub(crate) fn persist_event(
        &self,
        run: u64,
        generation: u64,
        event: &ScanEvent,
    ) -> io::Result<()> {
        self.persist_events(run, generation, &[event])
    }
    pub(crate) fn persist_events(
        &self,
        run: u64,
        generation: u64,
        events: &[&ScanEvent],
    ) -> io::Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        super::checkpoint::check_generation(&tx, run, generation)?;
        for event in events {
            persist(&tx, run, event)?;
        }
        tx.commit().map_err(io::Error::other)
    }
    pub fn page_tasks(&self, run: u64, after: u64, limit: usize) -> io::Result<Vec<TaskRow>> {
        if !(1..=256).contains(&limit) {
            return Err(invalid("task page must contain 1..256 rows"));
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let mut query = conn
            .prepare("SELECT task,state FROM scan_jobs WHERE run=?1 AND id>?2 ORDER BY id LIMIT ?3")
            .map_err(io::Error::other)?;
        let rows = query
            .query_map(params![run, after, limit as i64], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(io::Error::other)?;
        let mut out = Vec::new();
        for row in rows {
            let (task, state) = row.map_err(io::Error::other)?;
            out.push(TaskRow {
                task: codec::decode(&task, 1024 * 1024)?,
                state: match state.as_str() {
                    "pending" => TaskState::Pending,
                    "queued" => TaskState::Queued,
                    "running" => TaskState::Running,
                    "done" => TaskState::Done,
                    _ => return Err(invalid("invalid durable task state")),
                },
            });
        }
        Ok(out)
    }
    pub fn read_result(&self, run: u64, id: u64) -> io::Result<Option<ScannedFile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?;
        let row: Option<(Vec<u8>, Vec<u8>)> = conn
            .query_row(
                "SELECT data,digest FROM scan_results WHERE run=?1 AND id=?2 AND length(data)<=?3",
                params![run, id, codec::MAX_BLOB_BYTES as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(io::Error::other)?;
        match row {
            None => Ok(None),
            Some((data, digest)) => {
                if Sha256::digest(&data).as_slice() != digest {
                    return Err(io::Error::other("stored result checksum differs"));
                }
                codec::decode(&data, codec::MAX_BLOB_BYTES).map(Some)
            }
        }
    }
    pub fn scan(
        self: &Arc<Self>,
        root: Arc<ScanRoot>,
        options: ScanOptions,
        backend: Arc<dyn ExtractionBackend>,
        control: ScanControl,
        mut sink: impl FnMut(u64, ScanEvent) -> io::Result<()>,
    ) -> io::Result<(u64, ScanSummary)> {
        let options = self.options(&root, options, backend.as_ref())?;
        let run = self.create_run_with_engine(
            &root,
            &options,
            &super::checkpoint::run_fingerprint(backend.as_ref()),
        )?;
        let journal = super::checkpoint::RunJournal::new(Arc::clone(self), run, 1, &options);
        let result =
            super::pipeline::run_pipeline(root, options, backend, control, journal, |event| {
                sink(run, self.dispatch(run, 1, event)?)
            });
        match result {
            Ok(summary) => Ok((run, self.summary(run, summary.stopped)?)),
            Err(e) => {
                self.fail_generation(run, 1);
                Err(e)
            }
        }
    }
}

pub(crate) fn persist(tx: &Transaction<'_>, run: u64, event: &ScanEvent) -> io::Result<()> {
    match event {
        ScanEvent::Discovered(task) => {
            tx.execute(
                "INSERT INTO scan_jobs(run,id,relative,task,state) VALUES(?1,?2,?3,?4,'queued')",
                params![
                    run,
                    task.id,
                    codec::encode(&task.relative, 1024 * 1024)?,
                    codec::encode(task, 1024 * 1024)?
                ],
            )
            .map_err(io::Error::other)?;
        }
        ScanEvent::Resumed(_) => {}
        ScanEvent::Started(task) => {
            if tx.execute("UPDATE scan_jobs SET state='running' WHERE run=?1 AND id=?2 AND state='queued'",params![run,task.id]).map_err(io::Error::other)?!=1{return Err(invalid("task is not queued"));}
        }
        ScanEvent::Completed(file) => {
            let data = codec::encode(file, codec::MAX_BLOB_BYTES)?;
            tx.execute(
                "INSERT OR REPLACE INTO scan_results(run,id,data,digest,failed,unstable,hashed_bytes) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![run, file.task.id, &data, Sha256::digest(&data).to_vec(),file.failed(),file.unstable,file.hashes.as_ref().map_or(0,|h|h.bytes_read)],
            )
            .map_err(io::Error::other)?;
            if tx
                .execute(
                    "UPDATE scan_jobs SET state='done' WHERE run=?1 AND id=?2",
                    params![run, file.task.id],
                )
                .map_err(io::Error::other)?
                != 1
            {
                return Err(invalid("result has no durable task"));
            }
        }
        ScanEvent::Notice {
            path,
            code,
            message,
        } => {
            let path = path
                .as_ref()
                .map(|p| codec::encode(p, 1024 * 1024))
                .transpose()?;
            tx.execute(
                "INSERT INTO scan_notices(run,path,code,message) VALUES(?1,?2,?3,?4)",
                params![run, path, code, message],
            )
            .map_err(io::Error::other)?;
        }
        ScanEvent::DiscoveryFinished => {
            tx.execute("UPDATE scan_runs SET discovery_done=1 WHERE id=?1", [run])
                .map_err(io::Error::other)?;
        }
        ScanEvent::Finished(summary) => {
            let status = match summary.stopped {
                None => "completed",
                Some(StopReason::Paused) => "paused",
                Some(StopReason::Cancelled) => "cancelled",
                Some(StopReason::Deadline) => "deadline",
            };
            tx.execute(
                "UPDATE scan_runs SET status=?2 WHERE id=?1",
                params![run, status],
            )
            .map_err(io::Error::other)?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ScanCheckpoint {
    pub run: u64,
    pub generation: u64,
    pub root: NativePath,
    pub root_stamp: FileStamp,
    pub options: ScanOptions,
    pub engine: String,
    pub status: String,
    pub discovery_done: bool,
}
