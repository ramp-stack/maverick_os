use std::fmt::Debug;

use serde_json::Value;

pub enum FieldOP {
    Read,
    Write(String),
    Value
}

#[macro_export]
macro_rules! field {
    ($f:expr) => {
        Field::new(|new: FieldOP| Ok(match new {
            FieldOP::Write(new) => {$f = serde_json::from_str(&new)?; (None, None)},
            FieldOP::Read => (Some(serde_json::to_string(&$f)?), None),
            FieldOP::Value => (None, Some(serde_json::to_value(&$f)?))
        }))
    }
}

pub struct Field<'a>(Box<dyn FnMut(FieldOP) -> Result<(Option<String>, Option<Value>), serde_json::Error> + 'a>);
impl<'a> Field<'a> {
    pub fn new(access: impl FnMut(FieldOP) -> Result<(Option<String>, Option<Value>), serde_json::Error> + 'a) -> Self {
        Field(Box::new(access))
    }

    pub fn read(&mut self) -> Result<String, serde_json::Error> {Ok((self.0)(FieldOP::Read)?.0.unwrap())}
    pub fn write(&mut self, new: &str) -> Result<(), serde_json::Error> {(self.0)(FieldOP::Write(new.to_string()))?; Ok(())}
    pub fn value(&mut self) -> Result<Value, serde_json::Error> {Ok((self.0)(FieldOP::Value)?.1.unwrap())}
}

impl<'a> Debug for Field<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Field<'a>")
    }
}
