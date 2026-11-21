//! Background monitor lifecycle. Dropping a watch cancels polling and never
//! leaves an autonomous watcher running after the case/window is closed.
use eframe::egui;
use metaxtract_app::{self as app, core::scan::{ChangeBatch, ScanControl}};
use std::{io, path::PathBuf, sync::mpsc, time::{Duration, Instant}};

enum Command { Pause, Resume, Stop }
pub struct WatchUi {
    pub root: PathBuf,
    pub case_path: PathBuf,
    pub paused: bool,
    pub pending: bool,
    pub baseline: Option<String>,
    pub coverage_complete: bool,
    pub latest_changes: usize,
    commands: mpsc::Sender<Command>,
    batches: mpsc::Receiver<io::Result<ChangeBatch>>,
    control: ScanControl,
}
impl Drop for WatchUi {
    fn drop(&mut self) {
        self.control.cancel();
        let _ = self.commands.send(Command::Stop);
    }
}
impl WatchUi {
    pub fn start(root: PathBuf, case_path: PathBuf, include_hidden: bool, ctx: egui::Context) -> io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let (sender, batches) = mpsc::sync_channel(8);
        let control = ScanControl::new();
        let worker_control = control.clone();
        let source = root.clone();
        let workspace = case_path.clone();
        std::thread::Builder::new().name("metaxtract-directory-monitor".into()).spawn(move || {
            let result = (|| -> io::Result<()> {
                let mut monitor = app::monitor::start(source, &workspace, include_hidden, &worker_control)?;
                monitor.request_rescan();
                let mut last = Instant::now() - Duration::from_secs(5);
                loop {
                    match receiver.recv_timeout(Duration::from_millis(200)) {
                        Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Ok(Command::Pause) => monitor.pause(),
                        Ok(Command::Resume) => { monitor.resume();last = Instant::now() - Duration::from_secs(5); }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    worker_control.checkpoint()?;
                    if last.elapsed() < Duration::from_secs(5) { continue; }
                    monitor.poll(&worker_control)?;
                    let batch = monitor.drain();
                    match sender.try_send(Ok(batch)) {
                        Ok(()) => ctx.request_repaint(),
                        Err(mpsc::TrySendError::Full(_)) => monitor.request_rescan(),
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    }
                    last = Instant::now();
                }
                Ok(())
            })();
            if let Err(error) = result { let _ = sender.try_send(Err(error));ctx.request_repaint(); }
        })?;
        Ok(Self {
            root, case_path, paused: false, pending: false, baseline: None,
            coverage_complete: false, latest_changes: 0, commands, batches, control,
        })
    }
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        let _ = self.commands.send(if self.paused { Command::Pause } else { Command::Resume });
    }
    pub fn poll(&mut self) -> io::Result<()> {
        loop {
            match self.batches.try_recv() {
                Ok(Ok(batch)) => {
                    self.coverage_complete = batch.coverage_complete;
                    self.latest_changes = batch.changes.len();
                    self.pending |= batch.full_rescan_required || !batch.changes.is_empty();
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Err(io::Error::other("directory monitor stopped")),
            }
        }
    }
}
