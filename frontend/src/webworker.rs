#[derive(Debug)]
pub struct Message(pub u32);
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Input(pub u32);
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Output(pub u32);

pub struct WebWorker {}
impl gloo_worker::Worker for WebWorker {
    type Message = Message;
    type Input = Input;
    type Output = Output;

    fn create(_scope: &gloo_worker::WorkerScope<Self>) -> Self {
        log::debug!("create");
        Self {}
    }

    fn update(&mut self, _scope: &gloo_worker::WorkerScope<Self>, msg: Self::Message) {
        log::debug!("update {msg:?}");
    }

    fn received(
        &mut self,
        scope: &gloo_worker::WorkerScope<Self>,
        msg: Self::Input,
        _id: gloo_worker::HandlerId,
    ) {
        log::debug!("received {msg:?}");
        scope.respond(_id, Output(msg.0 + 5001));
    }
}