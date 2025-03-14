use crossbeam::channel::{Receiver, Sender};
use ratatui::{layout::Rect, prelude::Backend, widgets::{Block, Borders, List, ListItem, Paragraph}, Frame};
use serde::Serialize;
// use crossbeam_channel::{unbounded, Receiver, Sender};
// use anyhow::Result;
// use serde::Serialize;
// use ratatui::{backend::Backend, widgets::{Paragraph, Block, Borders, List, ListItem}, layout::Rect, Frame};
// use tokio::task;

// #[tokio::main]
// async fn main() -> anyhow::Result<(), anyhow::Error> {
//     let (render_tx, data_rx): (Sender<Box<dyn Message>>, Receiver<Box<dyn Message>>) = unbounded();
//     let (data_tx, render_rx): (Sender<Box<dyn Message>>, Receiver<Box<dyn Message>>) = unbounded();

//     let data_system = DataSystem::new(data_tx, data_rx);
//     let render_system = RenderSystem::new(render_tx, render_rx);

//     task::spawn(async move { data_system.run().await });
//     render_system.run().await;

//     Ok(())
// }


// Generic Message trait
pub trait Message: Send {
    fn as_display(&self) -> String;
}

// Trait for renderable widgets
pub trait WidgetRenderer {
    fn render_widget<B: Backend>(&self, frame: &mut Frame, area: Rect);
}

// Implement WidgetRenderer for String
impl WidgetRenderer for String {
    fn render_widget<B: Backend>(&self, frame: &mut Frame, area: Rect) {
        let paragraph = Paragraph::new(self.as_str()).block(Block::default().borders(Borders::ALL).title("Paragraph"));
        frame.render_widget(paragraph, area);
    }
}

// Implement WidgetRenderer for Vec<String>
impl WidgetRenderer for Vec<String> {
    fn render_widget<B: Backend>(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.iter().map(|item| ListItem::new(item.as_str())).collect();
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("List"));
        frame.render_widget(list, area);
    }
}

// Message trait
pub trait UIMessage: Message + WidgetRenderer {}

// Communication trait for sending and receiving
pub trait CommunicationSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error>;
    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error>;
}


// Data system
pub struct DataSystem {
    sender: Sender<Box<dyn Message>>,
    receiver: Receiver<Box<dyn Message>>,
}

impl DataSystem {
    pub fn new(sender: Sender<Box<dyn Message>>, receiver: Receiver<Box<dyn Message>>) -> Self {
        Self { sender, receiver }
    }

    pub async fn run(&self) {
        while let Ok(message) = self.receive() {
            println!("DataSystem received: {}", message.as_display());
        }
    }
}

impl CommunicationSystem for DataSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error> {
        self.sender.try_send(msg).map_err(|e| {
            Err(anyhow::anyhow!("{e:?}"))
        });
        Ok(())
    }

    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error> {
        Ok(self.receiver.try_recv()?)
    }
}

// Render system
pub struct RenderSystem {
    sender: Sender<Box<dyn Message>>,
    receiver: Receiver<Box<dyn Message>>,
}

impl RenderSystem {
    pub fn new(sender: Sender<Box<dyn Message>>, receiver: Receiver<Box<dyn Message>>) -> Self {
        Self { sender, receiver }
    }

    pub async fn run(&self) {
        while let Ok(message) = self.receive() {
            println!("RenderSystem received: {}", message.as_display());
            // Integrate here with ratatui rendering logic
            // E.g., frame and area setup to call message.render_widget(frame, area);
        }
    }
}

impl CommunicationSystem for RenderSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error> {
        self.sender.try_send(msg)?;
        Ok(())
    }

    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error> {
        Ok(self.receiver.try_recv()?)
    }
}