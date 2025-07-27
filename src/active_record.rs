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

use gxhash::GxBuildHasher;
use serde_json::{Value, Map};
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use air::{DateTime, now, storage::records::RecordPath};

use crate::hardware::cache::{RustSqlite, Cache, Connection};
use crate::Id;

//  mod field;
//  use field::{Field, FieldOP};
//  use crate::field;

//  mod map;
//  use map::RecordMap;

#[derive(Debug)]
pub enum Error {
    FloatingFields,
    InvalidValue(Value),
    SerdeJsonError(serde_json::Error),
    SqliteError(rusqlite::Error),
    FromSqliteError(rusqlite::types::FromSqlError)
}
impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{:?}", self)}
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Error {Error::SerdeJsonError(error)}
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Error {Error::SqliteError(error)}
}
impl From<rusqlite::types::FromSqlError> for Error {
    fn from(error: rusqlite::types::FromSqlError) -> Error {Error::FromSqliteError(error)}
}

//  #[async_trait::async_trait]
//  trait Location {
//      async fn create(&mut self, path: Vec<String>, value: SqliteRef<'_>) -> Result<(), Error>;
//      async fn read(&mut self, path: Vec<String>) -> Result<SqliteOwned, Error>;
//      async fn delete(&mut self, path: Vec<String>) -> Result<(), Error>;
//  }

//  pub trait ActiveRecord: Tableize + Debug {
//      fn table_name() -> String where Self: Sized;//Table name is the global identifier for where this object is stored
//                                //in any system
//      fn id(&self) -> Result<String, serde_json::Error>;//The primary key identifies this object uniquly in its table
//      
//      //fn get_ref(&mut self) -> BTreeMap<String, RecordRef<'_>>;

//      //Read object from the database error if missing any non optional fields
//    //fn read<L: _Location>(loc: &mut L) -> Result<Self, Error> where Self: Sized {
//    //    Self::from_value(loc.read::<Self>(vec![Self::table_name()])?)
//    //}
//  }

//  pub trait Tableize {
//      fn from_value(value: Value) -> Result<Self, Error> where Self: Sized;
//      //fn from_raw(raw: RawAtomic) -> Self where Self: Sized;
//      //fn get_mut(&mut self) -> Result<RecordMut<'_>, serde_json::Error>;
//  }

//  default impl<S: Serialize + for<'a> Deserialize<'a>> Tableize for S {
//      fn from_value(value: Value) -> Result<Self, Error> where Self: Sized {
//          Ok(serde_json::from_value(value)?)
//      }
//    //fn get_mut(&mut self) -> Result<RecordMut<'_>, serde_json::Error> {
//    //    Ok(RecordMut::Field(field!(*self)))
//    //}
//  }

//  default impl<S: Serialize + for<'a> Deserialize<'a> + RecordMap> Tableize for S {
//      fn from_value(value: Value) -> Result<Self, Error> where Self: Sized {
//          match value {
//              Value::Object(map) => <S as RecordMap>::from_value(map),
//              x => Err(Error::InvalidValue(x))
//          }
//      }
//    //fn get_mut(&mut self) -> Result<RecordMut<'_>, serde_json::Error> {
//    //    Ok(RecordMut::Map(self))
//    //}
//  }

//  pub enum RecordMut<'a> {//Record Structure
//      Field(Field<'a>),
//      Record(&'a mut dyn ActiveRecord),
//      //Struct(BTreeMap<String, Self>),//Limited number of keys cannot add new fields
//      Map(&'a mut dyn RecordMap),
//  }

//  pub enum RawAtomic {
//      Field(String),
//      Map(BTreeMap<String, Self>)
//  }

//  //Arrays get turned into Objects and objects get turned into table references
//  fn value_to_index(val: &Value) -> u32 {match val {
//      Value::Null => 0,
//      Value::Bool(_) => 1,
//      Value::Number(_) => 2,
//      Value::String(_) => 3,
//      Value::Array(_) => 4,
//      Value::Object(_) => 4,
//  }}

//  pub enum ActiveValue {
//      Bool,
//      Interger,
//      Real,
//      Text,
//      Blob
//      Object,//SubTable
//  }

//  fn value_to_tables(path: Vec<String>, value: &Value) -> Result<BTreeMap<String, ActiveValue>, Error> {
//      match value {
//          Value::Array(values) => {
//              //Check the first entry in the array for options map
//              //Insert __type__ => Array
//              //Check for __type__ Modifiers during deserilaziation of maps
//              self.create(path, Value::Object(Map::from_iter(values.into_iter().enumerate().map(|(k, v)| (k.to_string(), v)))))?;
//          },
//          Value::Object(map) if map.get("__struct__") == Some(&Value::Bool(true)) => {
//              //This a reserved tag that allows for optimizations of maps where the key and the
//              //value(s) are columns for further optimization of a map of maps of maps they should be serialized as one map with a extra field for each of the sub map ids a => value+b+c this can be easily done with the flatten tag for serde
//          },
//          Value::Object(map) => {
//          },
//          //Otherwise its a structure and should have a column for each key
//          //If a structure is falsly assumed to be a map it reads out to the same thing in the
//          //end
//          _ => {return Err(Error::FloatingFields);}
//      }
//  }

//  trait _Location {
//      fn create(&self, path: Vec<String>, value: Value) -> Result<(), Error>;
//      fn read(&self, path: Vec<String>) -> Result<Value, Error>;
//      //fn sync_state(&self, path: String, idx: String, state: StateVector<'_>) -> Result<(), Error>;
//  }
//  impl _Location for Connection {
//      fn create(&self, path: Vec<String>, value: Value) -> Result<(), Error> {
//          .
//          match value {
//              Value::Array(values) => {
//                  //Check the first entry in the array for options map
//                  //Insert __type__ => Array
//                  //Check for __type__ Modifiers during deserilaziation of maps
//                  self.create(path, Value::Object(Map::from_iter(values.into_iter().enumerate().map(|(k, v)| (k.to_string(), v)))))?;
//              },
//              Value::Object(map) if map.get("__horizontal__") == Some(&Value::Bool(true)) => {
//                  //This a reserved tag that allows for optimizations of maps where the key and the
//                  //value(s) are columns for further optimization of a map of maps of maps they should be serialized as one map with a extra field for each of the sub map ids a => value+b+c this can be easily done with the flatten tag for serde
//              },
//              Value::Object(map) => {
//              },
//              //Otherwise its a structure and should have a column for each key
//              //If a structure is falsly assumed to be a map it reads out to the same thing in the
//              //end
//              _ => {return Err(Error::FloatingFields);}
//          }
//          Ok(())
//      }

//      fn read(&self, path: Vec<String>) -> Result<Value, Error> {
//          todo!()
//      }
//  }


//  //1. Active records turn properties into fields and objects into rows
//  //2. All sub maps are



//  //  //#[derive(ActiveRecord)]
//  //  #[derive(Debug, Clone)]
//  //  struct Room {
//  //      id: Id,
//  //      name: String,
//  //      //#[active]
//  //      messages: BTreeMap<Id, Message>,
//  //  }

//  //  impl Tableize2 for Room {
//  //      fn get_table(&mut self) -> Result<BTreeMap<String, Table>, serde_json::Error> {
//  //          Ok(BTreeMap::from([(
//  //              "rooms_table".to_string(),
//  //              Table(BTreeMap::from([
//  //                  ("id".to_string(), DataType::Blob), 
//  //                  ("author".to_string(), DataType::Text), 
//  //                  ("body".to_string(), DataType::Text), 
//  //              ])),
//  //          )]))
//  //      }
//  //  }

//  //  impl ActiveRecord for Room {
//  //      fn table_name() -> String {"Room".to_string()}
//  //      fn id(&self) -> Result<String, serde_json::Error> {serde_json::to_string(&self.id)}

//  //    //fn read(&self) -> Result<BTreeMap<String, Value>, serde_json::Error> {
//  //    //    let mut map = BTreeMap::from([
//  //    //        (Room::table_name(), Value::Object(Map::from([
//  //    //            ("id".to_string(), serde_json::to_value(&self.id)?),
//  //    //            ("name".to_string(), serde_json::to_value(&self.name)?),
//  //    //            ("messages".to_string(), Value::Object(Map::from_iter(
//  //    //                self.messages.iter().map(|(k, v)|
//  //    //                    Ok((serde_json::to_string(&k)?, v.id()))
//  //    //                ).collect::<Result<Vec<_>, serde_json::Error>>()?
//  //    //            )))
//  //    //        ]))),
//  //    //    ]);
//  //    //    map.extend(self.messages.values().map(|v| v.read()));
//  //    //    Ok(map)
//  //    //}
//  //  }

//  //  struct Table(BTreeMap<String, DataType>);

//  //  enum DataType {
//  //      Number,
//  //      Text,
//  //      Bool,
//  //      Blob,
//  //      SubTable(Table)
//  //  }

//  //  trait Tableize2 {
//  //      fn get_table(&mut self) -> Result<BTreeMap<String, Table>, serde_json::Error>;
//  //  }

//  //  //#[derive(Atomic)]
//  //  #[derive(Serialize, Deserialize, Debug, Clone)]
//  //  struct Message {
//  //      id: Id,
//  //      author: String,
//  //      body: String 
//  //  }

//  //  impl Tableize2 for Message {
//  //      fn get_table(&mut self) -> Result<BTreeMap<String, Table>, serde_json::Error> {
//  //          Ok(
//  //              BTreeMap::from([(
//  //                  "messages_table".to_string(),
//  //                  Table(BTreeMap::from([
//  //                      ("id".to_string(), DataType::Blob), 
//  //                      ("author".to_string(), DataType::Text), 
//  //                      ("body".to_string(), DataType::Text), 
//  //                  ])),
//  //              )])
//  //          )
//  //      }
//  //  }

//  //  impl ActiveRecord for Message {
//  //      fn table_name() -> String {"Message".to_string()}
//  //      fn id(&self) -> Result<String, serde_json::Error> {serde_json::to_string(&self.id)}

//  //    //fn read(&self) -> Result<BTreeMap<String, Value>, serde_json::Error> {
//  //    //    let map = BTreeMap::from([
//  //    //        (Message::table_name(), Value::Object(Map::from([
//  //    //            ("id".to_string(), serde_json::to_value(&self.id)?),
//  //    //            ("author".to_string(), serde_json::to_value(&self.author)?),
//  //    //            ("body".to_string(), serde_json::to_value(&self.body)?),
//  //    //        ]))),
//  //    //    ]);
//  //    //    Ok(map)
//  //    //}
//  //  }


    mod test {
        use super::*;

        #[derive(Serialize, Deserilize)]
        struct Room {
            id: Id,
            name: String,
            messages: Set<Message>,
            comments: Set<Comments>,
        }

        impl AirStorable {
            fn get_ref(&self) -> AirRef<'_> {
                AirRef::Map(RoomProtocol, Some(&(&self.id, &self.name)), BTreeMap::from([
                    ("messages".to_string(), )
                ])) 
            }
        }

        impl SqliteStorable {
            fn get_ref(&self) -> SqliteRef<'_> {
                SqliteRef::Table("rooms", BTreeMap::from([
                    ("data".to_string(), SqliteRef::Field(&(&self.id, &self.name))),
                    ("messages".to_string(), self.message.get_ref())
                ])) 
            }
        }

        #[derive(Serialize, Deserilize)]
        struct Message {
            id: Id,
            body: String,
            author: String,
        }



//      #[derive(Serialize)]
//      struct MyStr {
//          a: u32,
//          b: u32,
//          c: u32,
//          x: char
//      }

//      #[derive(Serialize)]
//      struct Wrapper {
//          x: char,
//          #[serde(flatten)]
//          t: BTreeMap<u32, BTreeMap<u32, BTreeMap<u32, char>>>,
//      }

//      #[derive(Serialize, Deserilize, ActiveRecord)]
//      struct Room {
//          id: Id,
//          name: String,
//          messages: Vec<Message>
//      }

//      #[derive(Serialize, Deserilize, ActiveRecord(Field))]
//      struct Message {
//          id: Id,
//          body: String,
//          author: String,
//      }

//      impl ActiveRecord for Message {
//          
//      }

        #[tokio::test]
        async fn test() {
//        //let mut cache = Cache::new();
//        //let mut map: BTreeMap<String, BTreeMap<String, Dated<u32>>> = BTreeMap::default();
//        //map.insert("Hello".to_string(), BTreeMap::from([("a".to_string(), 29.into())]));
//        //cache.sync_remote("mymessage".to_string(), &mut map).await.unwrap();
//        //println!("map: {:?}", map);

//        //map.insert("Goodbye".to_string(), BTreeMap::from([("b".to_string(), 73.into())]));
//        //cache.sync_remote("mymessage".to_string(), &mut map).await.unwrap();
//        //println!("map: {:?}", map);

//        //map.remove("Goodbye");
//        //map.insert("Goodbye".to_string(), BTreeMap::from([("a".to_string(), 20.into())]));
//        //cache.sync_remote("mymessage".to_string(), &mut map).await.unwrap();

//        //println!("map: {:?}", map);
//        //
//          let x = BTreeMap::from([(0, BTreeMap::from([(2, BTreeMap::from([(3, 'a')]))]))]);
//          //let x = MyStr{a: 0, b: 2, c: 3, x: 'a'};
//          let x = Wrapper{x: 'h', t: x};

//          println!("test: {:#?}", serde_json::to_value(&x).unwrap());

            panic!("done");
        }
    }


//  //All sub maps get converted into a id 
//  //If the sub map is an active record then the id points to a record in the global table
//  //Otherwise the id is "0" and points to a record in a sub table
//  //
//  //Where message is Active
//  //messages: BTreeMap<XStringifable, Message> -> BTreeMap<XStringifyable, Id> ->
//  //table_name+messages: XStringifyable, Id
//  //messages: Expanded Message
//  //
//  //Where message is not Active
//  //messages: BTreeMap<XStringifable, Message> -> BTreeMap<XStringifyable, Id> ->
//  //table_name+messages: XStringifyable, Expandede Message
//  //
//  //Where message is not Active
//  //messages: BTreeMap<XStringifable, BTreeMap<YId, Message>>
//  //table_name+messages: BTreeMap<XStringifyable, YId> ->
//  //table_name+messages+Yid: Expandede Message
//  //OR
//  //table_name+messages: XStringifyable, YId, ExpandedMessage

//  //Where message is not Active
//  //messages: BTreeMap<XStringifable, BTreeMap<YId, Message>>
//  //table_name+messages: XStringifyable
//  //table_name+messages+XStryifiyable: YId
//  //table_name+messages+XStringifyable+YId: ExpandedMessage



//  //Issues:
//  //
//  //Cannot differ between object and map in serde json value
//  //Cannot learn of indexing method from a serde json array (hash, ord, id, etc)
//  //Solution is to use struct for all structures and manually impl a map trait for maps
//  //
//  //


//  //Default impl on serializeable objects as fields/structs
//  //Manual impls for maps via RecordMap
//  //Derive impls for ActiveRecords and tag children as active 
//  //



//  //  bob: BTreeMap<u32, BTreeMap<u32, BTreeMap<u32, char>>>

//  //  struct x {
//  //      __table_name: 'bob'
//  //      a: u32,
//  //      b: u32
//  //      c: u32,
//  //      x: char
//  //  }

//  //  table bob

//  //  a | b | c | x
//  //  -------------
//  //  0 | 2 | 3 | 'a'
