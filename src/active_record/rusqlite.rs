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

pub trait SqliteStorable {
    fn get_ref(&self) -> SqliteRef<'_>;    
    fn get_mut(&self) -> SqliteMut<'_>;    
    fn from_owend(value: SqliteOwned) -> Self where Self: Sized;
}

pub struct SqliteOwned {
    Field(Value),
    Map(BTreeMap<String, Self>, bool),
}

pub struct SqliteRef<'a> {
    Field(&'a dyn ToSql),
    Map(BTreeMap<String, Self>, bool),
}

pub struct SqliteMut<'a> {
    Field(&'a mut dyn ToSql),
    Map(BTreeMap<String, Self>, bool),
}

trait _Location {
    fn create(&self, path: Vec<String>, value: SqliteRef<'_>) -> Result<(), Error>;
    fn read(&self, path: Vec<String>) -> Result<SqliteOwned, Error>;
    fn delete(&self, path: Vec<String>) -> Result<(), Error>;
}
impl _Location for Connection {
    fn create(&self, path: Vec<String>, value: SqliteRef<'_>) -> Result<(), Error> {
        Ok(())
    }

    fn read(&self, path: Vec<String>) -> Result<SqliteOwned, Error> {
        todo!()
    }

    fn delete(&self, path: Vec<String>) -> Result<(), Error> {
        Ok(())
    }
}

    //Error::ToSqlConversionFailure(Box<dyn Error + Send + Sync + 'static>),
    //
//
// No updating indivudual fields treat rows as whole objects???
// Every row has two columns key value
