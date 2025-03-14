pub mod communication_system;
pub mod data_system;
pub mod notification_system;
pub mod render_system;
pub mod widget_render_system;

//     let (render_tx, data_rx): (Sender<Box<dyn Message>>, Receiver<Box<dyn Message>>) = unbounded();
//     let (data_tx, render_rx): (Sender<Box<dyn Message>>, Receiver<Box<dyn Message>>) = unbounded();
//     let data_system = DataSystem::new(data_tx, data_rx);
//     let render_system = RenderSystem::new(render_tx, render_rx);
//     task::spawn(async move { data_system.run().await });
//     render_system.run().await;
