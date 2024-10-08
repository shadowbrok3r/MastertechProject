use crossbeam::channel::{self, Receiver, Sender};
pub trait UICommandSender {
    fn send_ui(&self, command: UICommand);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UICommand {
    // Listed in the order they show up in the command palette by default!
    Open,
    SaveRecording,
    SaveRecordingSelection,
    SaveBlueprint,
    CloseCurrentRecording,
    CloseAllRecordings,
}

pub enum SystemCommand {
    /// Make this the active application.
    ActivateApp(String),

    /// Close this app and all its recordings.
    CloseApp(String),
}

/// Interface for sending [`SystemCommand`] messages.
pub trait SystemCommandSender {
    fn send_system(&self, command: SystemCommand);
}

// ----------------------------------------------------------------------------

/// Sender that queues up the execution of commands.
#[derive(Clone)]
pub struct CommandSender {
    system_sender: Sender<SystemCommand>,
    ui_sender: Sender<UICommand>,
}

/// Receiver for the [`CommandSender`]
pub struct CommandReceiver {
    system_receiver: Receiver<SystemCommand>,
    ui_receiver: Receiver<UICommand>,
}

impl CommandReceiver {
    /// Receive a [`SystemCommand`] to be executed if any is queued.
    pub fn recv_system(&self) -> Option<SystemCommand> {
        // The only way this can fail (other than being empty)
        // is if the sender has been dropped.
        self.system_receiver.try_recv().ok()
    }

    /// Receive a [`UICommand`] to be executed if any is queued.
    pub fn recv_ui(&self) -> Option<UICommand> {
        // The only way this can fail (other than being empty)
        // is if the sender has been dropped.
        self.ui_receiver.try_recv().ok()
    }
}

/// Creates a new command channel.
pub fn command_channel() -> (CommandSender, CommandReceiver) {
    let (system_sender, system_receiver) = channel::unbounded();
    let (ui_sender, ui_receiver) = channel::unbounded();
    (
        CommandSender {
            system_sender,
            ui_sender,
        },
        CommandReceiver {
            system_receiver,
            ui_receiver,
        },
    )
}

// ----------------------------------------------------------------------------

impl SystemCommandSender for CommandSender {
    /// Send a command to be executed.
    fn send_system(&self, command: SystemCommand) {
        // The only way this can fail is if the receiver has been dropped.
        self.system_sender.send(command).ok();
    }
}

impl UICommandSender for CommandSender {
    /// Send a command to be executed.
    fn send_ui(&self, command: UICommand) {
        // The only way this can fail is if the receiver has been dropped.
        self.ui_sender.send(command).ok();
    }
}

impl UICommand {

}