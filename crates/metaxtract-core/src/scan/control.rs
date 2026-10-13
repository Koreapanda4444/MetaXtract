use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    Cancelled,
    Paused,
    Deadline,
}
#[derive(Clone, Default)]
pub struct ScanControl {
    state: Arc<AtomicU8>,
    deadline: Arc<Mutex<Option<Instant>>>,
}
impl ScanControl {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.state.store(1, Ordering::Release);
    }
    pub fn pause(&self) {
        let _ = self
            .state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
    }
    pub fn reason(&self) -> Option<StopReason> {
        match self.state.load(Ordering::Acquire) {
            1 => Some(StopReason::Cancelled),
            2 => Some(StopReason::Paused),
            _ => self
                .deadline
                .lock()
                .ok()
                .and_then(|d| *d)
                .filter(|d| Instant::now() >= *d)
                .map(|_| StopReason::Deadline),
        }
    }
    pub fn checkpoint(&self) -> io::Result<()> {
        match self.reason() {
            None => Ok(()),
            Some(StopReason::Deadline) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "scan deadline reached",
            )),
            Some(r) => Err(io::Error::new(
                io::ErrorKind::Interrupted,
                format!("scan {r:?}"),
            )),
        }
    }
    pub(crate) fn begin(&self, milliseconds: Option<u64>) -> io::Result<()> {
        self.checkpoint()?;
        *self
            .deadline
            .lock()
            .map_err(|_| io::Error::other("control lock poisoned"))? =
            milliseconds.map(|ms| Instant::now() + Duration::from_millis(ms));
        Ok(())
    }
    pub fn context(&self, milliseconds: u64) -> ExecutionContext {
        ExecutionContext {
            control: self.clone(),
            deadline: Instant::now() + Duration::from_millis(milliseconds),
        }
    }
}
pub struct ExecutionContext {
    pub(crate) control: ScanControl,
    deadline: Instant,
}
impl ExecutionContext {
    pub fn checkpoint(&self) -> io::Result<()> {
        self.control.checkpoint()?;
        if Instant::now() >= self.deadline {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "file deadline reached",
            ))
        } else {
            Ok(())
        }
    }
    pub fn remaining_ms(&self) -> u64 {
        self.deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}
