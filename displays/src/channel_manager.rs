use crossbeam::channel::{Receiver, Sender, bounded, unbounded};


pub trait ChannelManager<T> {
    fn create_bounded_channel(size: usize) -> (Sender<T>, Receiver<T>);
    fn create_unbounded_channel() -> (Sender<T>, Receiver<T>);
}

impl<T> ChannelManager<T> for T {
    fn create_bounded_channel(size: usize) -> (Sender<T>, Receiver<T>) {
        bounded(size)
    }

    fn create_unbounded_channel() -> (Sender<T>, Receiver<T>) {
        unbounded()
    }
}