use std::collections::HashMap;
use std::fmt::Debug;
use std::any::TypeId;

use serde::{Serialize, Deserialize};

pub trait Field: Serialize + for<'a> Deserialize <'a> + Default {}
impl<I: Serialize + for<'a> Deserialize <'a> + Default> Field for I {}

#[derive(Debug, Hash, Eq, PartialEq)]
enum Key {
    Raw(String),
    Id(TypeId)
}

#[derive(Debug, Default)]
pub struct State(HashMap<Key, Vec<u8>>);
impl State {
    pub fn set_raw(&mut self, key: String, value: Vec<u8>) {
        self.0.insert(Key::Raw(key), value);
    }
    pub fn set<F: Field + 'static>(&mut self, item: &F) {
        self.0.insert(Key::Id(TypeId::of::<F>()), serde_json::to_vec(&item).unwrap());
    }

    pub fn get_raw(&mut self, key: &str) -> Option<&Vec<u8>> {
        self.0.get(&Key::Raw(key.to_string()))
    }
    pub fn get<F: Field + 'static>(&self) -> F {
        self.0.get(&Key::Id(TypeId::of::<F>())).and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default()
    }
}
