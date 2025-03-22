use crate::{virtual_filesystem::FileSysHelper, Cmd, FileSystemAction};
use crossbeam::channel::Sender;

#[derive(Clone)]
pub struct WebSocketHelperDelegate {
    pub tx: Sender<Cmd>
}

impl WebSocketHelperDelegate {
    pub fn new(tx: Sender<Cmd>) -> Self {
        Self { tx }
    }
}

impl FileSysHelper for WebSocketHelperDelegate {
    fn handle_filesystem_action(&mut self, action: &FileSystemAction) {
        log::warn!("FileSysHelper for WebSocketHelperDelegate -> Action -> {action:?}");
        let _ = self.tx.try_send(Cmd::FileSystemAction(action.clone()));
    }
}