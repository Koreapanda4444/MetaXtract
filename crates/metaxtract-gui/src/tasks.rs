use crate::state::Desktop;
use std::sync::mpsc;
pub type Callback = Box<dyn FnOnce(&mut Desktop) + Send>;
pub fn channel() -> (mpsc::SyncSender<Callback>, mpsc::Receiver<Callback>) {
    mpsc::sync_channel(4)
}
