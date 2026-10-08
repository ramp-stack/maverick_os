use std::sync::OnceLock;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tokio::runtime::Handle;

pub trait Task: Send + 'static {
    fn run(&mut self) -> impl Future<Output = ()> + Send;
    fn shutdown(self) -> impl Future<Output = ()> + Send;
}

impl<F: Future<Output = ()> + Unpin + Send + 'static> Task for F {
    async fn run(&mut self) -> () {self.await}
    async fn shutdown(self) -> () {}
}


pub static RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct Runtime(CancellationToken, TaskTracker, Handle);
impl Runtime {
    pub(crate) fn new() -> Self {
        match RUNTIME.get() {
            Some(runtime) => runtime.clone(),
            None => {
                let runtime = tokio::runtime::Builder::new_multi_thread().enable_time().enable_io().build().unwrap();
                let handle = runtime.handle().clone();
                let token = CancellationToken::new();
                let tasks = TaskTracker::new();
                let tk = token.clone();
                let ts = tasks.clone();
                std::thread::spawn(move || runtime.block_on(async move {
                    tk.cancelled().await;
                    ts.wait().await;
                }));
                let runtime = Runtime(token, tasks, handle);
                let _ = RUNTIME.set(runtime.clone());
                runtime
            }
        }
    }


    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        match Handle::try_current() {
            Ok(current) if current.id() == self.2.id() => {
                tokio::task::block_in_place(|| self.2.block_on(future))
            }
            _ => self.2.block_on(future),
        }
    }

    pub fn spawn<T: Task>(&self, mut task: T) {
        let token = self.0.clone();
        self.1.spawn_on(async move {loop{
            tokio::select! {
                biased;
                _ = token.cancelled() => {
                    task.shutdown().await;
                    break;
                }
                _ = task.run() => {}
            }
        }}, &self.2);
    }

    pub(crate) fn shutdown(self) {
        self.0.cancel();
        self.1.close();
        self.block_on(self.1.wait());
    }
}
