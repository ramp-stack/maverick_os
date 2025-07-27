use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::hash::Hasher;
use std::hash::Hash;
//use std::collections::HashSet;
use std::cmp::Ordering;
use std::fmt::{Display, Debug};
use std::ops::{DerefMut, Deref, AddAssign};
use std::any::Any;
use rusqlite::OptionalExtension;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{Error, ToSql};
use rusqlite::types::Value;

use gxhash::GxBuildHasher;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use air::{DateTime, now, storage::records::RecordPath};

pub trait Field {
    fn to_string(&self) -> String;
    fn set_string(&mut self, s: String) -> Result<(), Error>;
    fn from_string(s: String) -> Result<Self, Error> where Self: Sized;
}

pub trait AirStorable {
    fn get_ref(&self) -> AirRef<'_>;    
    fn get_mut(&self) -> AirMut<'_>;    
    fn from_owend(value: AirOwned) -> Self where Self: Sized;
    fn protocol(&self) -> Protocol;
    fn s_protocol() -> Protocol -> Self where Self: Sized;
}

pub struct AirOwned {
    Field(String),
    Map(BTreeMap<String, Self>),
}

pub struct AirRef<'a> {
    Field(&'a dyn Field),
    Map(BTreeMap<String, Self>),
}

pub struct AirMut<'a> {
    Field(&'a mut dyn Field),
    Map(BTreeMap<String, Self>),
}

trait _Location {
    fn create(&self, path: Vec<String>, value: AirRef<'_>) -> Result<(), Error>;
    fn read(&self, path: Vec<String>) -> Result<AirOwned, Error>;
    fn delete(&self, path: Vec<String>) -> Result<(), Error>;
}
impl _Location for Connection {
    fn create(&self, path: Vec<String>, value: AirRef<'_>) -> Result<(), Error> {
        Ok(())
    }

    fn read(&self, path: Vec<String>) -> Result<AirOwned, Error> {
        todo!()
    }

    fn delete(&self, path: Vec<String>) -> Result<(), Error> {
        Ok(())
    }
}

    //Error::ToSqlConversionFailure(Box<dyn Error + Send + Sync + 'static>),
