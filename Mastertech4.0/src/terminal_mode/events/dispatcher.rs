use crossbeam::channel::Sender;

pub trait Dispatcher<T> {
    fn dispatch(&self, event: T);
}

impl<T> Dispatcher<T> for Sender<T> {
    fn dispatch(&self, event: T) {
        self.send(event).expect("event should have been sent");
    }
}

