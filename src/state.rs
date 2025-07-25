use std::collections::HashMap;
use std::fmt::Debug;
use std::any::TypeId;
use std::any::Any;

pub trait Field: Any + Debug {}
impl<I: Any + Debug> Field for I {}

#[derive(Debug, Hash, Eq, PartialEq)]
enum Key {
    Raw(String),
    Id(TypeId)
}

#[derive(Debug, Default)]
pub struct State(HashMap<Key, Box<dyn Any>>);
impl State {
    pub fn set_named<F: Field + 'static>(&mut self, key: String, value: F) {
        self.0.insert(Key::Raw(key), Box::new(value));
    }
    pub fn set<F: Field + 'static>(&mut self, item: F) {
        self.0.insert(Key::Id(TypeId::of::<F>()), Box::new(item));
    }

    pub fn get_named<F: Field + 'static>(&mut self, key: &str) -> Option<&F> {
        self.0.get(&Key::Raw(key.to_string())).and_then(|a| a.downcast_ref())
    }

    pub fn get_named_mut<F: Field + 'static>(&mut self, key: &str) -> Option<&mut F> {
        self.0.get_mut(&Key::Raw(key.to_string())).and_then(|a| a.downcast_mut())
    }

    pub fn get<F: Field + 'static>(&mut self) -> Option<&F> {
        self.0.get(&Key::Id(TypeId::of::<F>())).and_then(|a| a.downcast_ref())
    }
    pub fn get_mut<F: Field + 'static>(&mut self) -> Option<&mut F> {
        self.0.get_mut(&Key::Id(TypeId::of::<F>())).and_then(|a| a.downcast_mut())
    }

    pub fn get_or_default<F: Field + Default + 'static>(&mut self) -> &F {
        self.0.entry(Key::Id(TypeId::of::<F>())).or_insert_with(|| Box::new(F::default())).downcast_ref().unwrap()
    }
    pub fn get_mut_or_default<F: Field + Default + 'static>(&mut self) -> &mut F {
        self.0.entry(Key::Id(TypeId::of::<F>())).or_insert_with(|| Box::new(F::default())).downcast_mut().unwrap()
    }

    pub fn get_named_or_default<F: Field + Default + 'static>(&mut self, key: &str) -> &F {
        self.0.entry(Key::Raw(key.to_string())).or_insert_with(|| Box::new(F::default())).downcast_ref().unwrap()
    }
    pub fn get_named_mut_or_default<F: Field + Default + 'static>(&mut self, key: &str) -> &mut F {
        self.0.entry(Key::Raw(key.to_string())).or_insert_with(|| Box::new(F::default())).downcast_mut().unwrap()
    }
}
