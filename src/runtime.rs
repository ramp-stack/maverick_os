use tokio::sync::watch::{channel, Sender, Receiver};
use tokio::time::sleep;

use std::time::Duration;

use air::{Air, Secret};

pub use async_trait::async_trait;

pub type Services = Vec<Box<dyn Service>>;

#[async_trait]
pub trait Service: Send {
    async fn run(&mut self, ctx: &mut air::Context) -> Option<Duration>;
}

struct Task(Box<dyn Service>);
impl Task {
    pub async fn run(mut self, mut ctx: air::Context, mut pause: Option<Receiver<bool>>) {
        loop {
            if let Some(rx) = pause.as_mut() {
                while !*rx.borrow_and_update() {
                    if rx.changed().await.is_err() {return;}
                }
            }

            match self.0.run(&mut ctx).await {
                Some(duration) => sleep(duration).await,
                None => {return;}
            }
        }
    }
}

pub(crate) struct Runtime(Option<tokio::runtime::Runtime>, Sender<bool>);
impl Runtime {
    pub fn start(secret: Secret, services: Services, background: Services) -> (Self, air::Context) {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_time().enable_io().build().unwrap();
        let air = Air::start(secret);
        let (tx, rx) = channel(true);
        background.into_iter().for_each(|s| {runtime.spawn(Task(s).run(air.clone(), None));});
        services.into_iter().for_each(|s| {runtime.spawn(Task(s).run(air.clone(), Some(rx.clone())));});

        (Runtime(Some(runtime), tx), air)
    }

    pub fn pause(&mut self) {let _ = self.1.send(false);}
    pub fn resume(&mut self) {let _ = self.1.send(true);}
    pub fn shutdown(&mut self) {if let Some(r) = self.0.take() {r.shutdown_background();}}
}
