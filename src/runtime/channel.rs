use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::hash_map::DefaultHasher;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::hash::{Hasher, Hash};
use std::future::Future;
use std::sync::Arc;
use std::any::TypeId;
use std::pin::Pin;
use std::any::Any;

use downcast_rs::{impl_downcast, Downcast};
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};
use rand::Rng;

pub use tokio::time::Duration;

use super::{ThreadRequest, ThreadResponse};

///_Channel is the base channel that is used for between thread communications(Restricted to
///Strings at least on wasm)
pub struct _Channel();
impl _Channel {
    fn new() -> (Self, Self) {
        let (a, b) = channel();
        let (c, d) = channel();
        (_Channel(a, d), _Channel(c, b))
    }

    fn send(&mut self, payload: String) {
        let _ = self.0.send(payload);
    }

    fn receive(&mut self) -> Option<String> {
        self.1.try_recv().ok()
    }
}

pub trait Channel<S, R>: Send{
    fn send(&mut self, payload: S);
    fn receive(&mut self) -> Option<R>;
}



//  pub struct Channel<S, R> (pub Box<dyn Channel<ThreadResponse, ThreadRequest>>, pub PhantomData<S>, pub PhantomData<R>);
//  unsafe impl<S, R> Send for Channel<S, R> {}
//  impl<
//      S: Serialize + for<'a> Deserialize <'a>,
//      R: Serialize + for<'a> Deserialize <'a>,
//  >Channel<S, R> for Channel<S, R> {
//      fn send(&mut self, payload: &S) {
//          self.0.send(&ThreadResponse::Callback(serde_json::to_string(payload).unwrap()));
//      }

//      fn receive(&mut self) -> Option<R> {
//          while let Some(r) = self.0.receive() {
//              match r {
//                  ThreadRequest::Post(r) => return Some(serde_json::from_str(&r).unwrap()),
//                  _ => todo!()
//              }
//          }
//          None
//      }
//  }
