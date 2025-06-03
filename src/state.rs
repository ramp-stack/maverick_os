use std::collections::HashMap;
use std::fmt::Debug;

use serde::{Serialize, Deserialize};

pub trait Field: Serialize + for<'a> Deserialize <'a> + Default + Debug {
    fn ident() -> String where Self: Sized + 'static {
        std::any::type_name::<Self>().to_string()
    }
    fn to_bytes(&self) -> Vec<u8> {serde_json::to_vec(self).unwrap()}
    fn from_bytes(bytes: &[u8]) -> Self where Self: Sized {
        serde_json::from_slice(bytes).unwrap_or_default()
    }
}

impl<I: Serialize + for<'a> Deserialize <'a> + Default + Debug> Field for I {}


#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State(HashMap<String, Vec<u8>>);
impl State {
    pub fn set<F: Field + 'static>(&mut self, item: &F) {
        self.0.insert(F::ident(), item.to_bytes());
    }
    pub fn get<F: Field + 'static>(&self) -> F {
        self.0.get(&F::ident()).map(|b| F::from_bytes(b)).unwrap_or_default()
    }
}
