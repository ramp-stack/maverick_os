use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::hash::Hasher;
use std::hash::Hash;
//use std::collections::HashSet;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::ops::{DerefMut, Deref, AddAssign};

use gxhash::GxBuildHasher;
use serde_json::Value;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use air::{DateTime, now, storage::records::RecordPath};

use crate::hardware::cache::{RustSqlite, Cache, Connection};
use crate::Id;

#[derive(Debug)]
pub enum Error {
    InvalidStateVector
}
impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{:?}", self)}
}

//  pub type Field = (String, u64, u64);//data, state, ttl

//  pub enum AtomicField {
//      Field(Field),
//      Sub(Box<Atomic>)
//  }

//  #[async_trait::async_trait]
//  pub trait SyncAtomic {
//      async fn sync(&mut self, id: String, state: u64, data: &mut String) -> Result<(), Error>;
//  }

//  pub enum Atomic{
//      Field(String, u64, u64),
//    //Array(BTreeMap<f32, Atomic>, u64)//ttl time between checking for new items
//    //Map(BTreeMap<String, Atomic>, u64),//ttl time between checking for new items
//  }

//  impl Atomic {
//      async fn sync(&mut self, now: u64, id: String, sync: &mut impl SyncAtomic) -> Result<(), Error> {
//          match self {
//              Atomic::Field(data, state, ttl) if now > *ttl => {
//                  sync.sync(id, *state, data).await?;
//              },
//            //Atomic::Array(items, ttl) => {
//            //    if now > ttl {
//            //        let cstate: u64 = cache.get(&format!("{}_{}_state", id, name));
//            //    }

//            //},
//            //Atomic::Map(id, fields) => {
//            //    for (name, (date, state, ttl)) in fields {
//            //        
//            //    }
//            //},
//              _ => {}
//          } 
//          Ok(())
//      }
//  }

//  #[async_trait::async_trait]
//  impl SyncAtomic for Cache {
//      async fn sync(&mut self, id: String, state: u64, data: &mut String) -> Result<(), Error> {
//          self.lock(|cache: &Connection| {
//              let cstate: u64 = cache.get(&(id.clone()+"_state"));
//              match state.cmp(&cstate) {
//                  Ordering::Less => *data = cache.get(&(id+"_data")),
//                  Ordering::Equal => {},
//                  Ordering::Greater => cache.set(&(id+"_data"), data)
//              }
//          }).await;    
//          Ok(())
//      }
//  }

//  struct Atomic<T>{
//      inner: T,
//      state: BTreeMap<String, u64>
//  }
//  impl Atomic {

//  }
//  impl AtomicRef for String {}
//  impl AtomicRef for u64 {}

//  #[async_trait::async_trait]
//  pub trait Atomic {
//      fn sub(&mut self) -> BTreeMap<String, &mut dyn Atomic> {BTreeMap::default()}
//  }

//  pub struct DatedAtomic<T>(T, u64, u64); //state(date), hash
//  impl<T: AtomicRef> Atomic for DatedAtomic<T> {
//      fn sub(&mut self) -> BTreeMap<String, &mut dyn Atomic> {self.0.sub()}
//      //async fn sync(&mut self, get_state: impl Fn(String) -> u64, get_data: impl Fn(String) -> String);
//  }


//  pub struct Profile {
//      #[atomic(id)]
//      name: String,
//      #[atomic(latest)]
//      abt: String,
//      #[atomic(latest)]
//      pfp: u64,
//  }

//  pub struct Room {
//      #[atomic(id)]
//      id: Id,
//      #[atomic(latest)]
//      name: String,
//      #[atomic(max)]
//      user_count: u64,
//      messages: Vec<Atomic<Message>>
//  }

//  pub trait AtomicRef {
//  }

//  pub struct Profile {
//      messages: Vec<Atomic<Message>>
//  }

//  impl Profile {
//      fn id(&self) -> String {
//          self.name.clone()//+Profile Protocol || PrivateId
//      }
//  }
  //fn get_field_mut(&mut self, field: &str) -> Field<'_> {
  //    match field {
  //        "name" => Field::new(|new: Option<&str>| match new {
  //            None => serde_json::to_string(&self.name).unwrap(),
  //            Some(new) => {self.name = serde_json::from_str(new).unwrap(); "".to_string()}
  //        }),
  //        _ => {panic!("Field not found");}
  //    }
  //}
//  Construct AtomicRef a btreeMap of name to atomic field (map, array), Array needs mut ref to each item in array
//  AtomicRef only constructs mappings to sub atomics(other fields are all concidederd under self as a single field)
//  Build corosponding state map and store in wrapper struct
//  syncing gets AtomicRef from inner and state
//  pub struct Room(Id, Vec<Message>);
//  pub struct Message {
//      id: Id,
//      body: String,
//      created: DateTime<Utc>,
//      status: bool
//  }





macro_rules! field {
    ($f:expr) => {
        Field::new(|new: Option<&str>| match new {
            None => serde_json::to_string(&$f).unwrap(),
            Some(new) => {$f = serde_json::from_str(new).unwrap(); "".to_string()}
        })
    }
}

pub struct Field<'a>(Box<dyn FnMut(Option<&str>) -> String + 'a>);
impl<'a> Field<'a> {
    pub fn new(access: impl FnMut(Option<&str>) -> String + 'a) -> Self {
        Field(Box::new(access))
    }

    pub fn read(&mut self) -> String {(self.0)(None)}
    pub fn write(&mut self, new: &str) {(self.0)(Some(new));}
}


use std::any::Any;

//  pub struct AtomicMap<T>(BTreeMap<Id, T>);
//  impl<T: AtomicRef> AtomicMap<T> {
//      pub fn insert(&mut self, t: T) -> Option<T> {self.0.insert(t.id(), t)}
//      pub fn merge(&mut self, t: T) -> &T {todo!()}
//      pub fn remove(&mut self, id: &Id) -> Option<T> {self.0.remove(id)}
//  }

//  impl<T> Deref for AtomicMap<T> {
//      type Target = BTreeMap<Id, T>;
//      fn deref(&self) -> &Self::Target {&self.0}
//  }



//  pub enum State {
//      Field(u64),
//      Array(Vec<(f32, State)>),
//      Map(BTreeMap<String, State>)
//  }
//  impl State {
//      pub fn now() -> Self {State::Field(now().timestamp() as u64)}
//  }

//  pub struct CacheAtomic<A: AtomicRef>(A, State, );
//  impl<A: AtomicRef> CacheAtomic<A> {
//      pub fn new(inner: A) -> Self {
//          let state = inner.default_state();
//          CacheAtomic(inner, state)
//      }
//  }

//  impl<A: AtomicRef> Deref for CacheAtomic<A> {
//      type Target = A;
//      fn deref(&self) -> &Self::Target {&self.0}
//  }

//  impl<A: AtomicRef> DerefMut for CacheAtomic<A> {
//      fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
//          &mut self.0
//      }
//  }

//  //  impl<T: Serialize + for<'a> Deserialize<'a> + 'static> AtomicRef for Vec<T> {
//  //      fn sub(&mut self) -> Atomic<'_> {
//  //          Atomic::Array(self.iter_mut().map(|f| field!(*f)).collect())
//  //      }
//  //  }

//  //impl AtomicRef for String {}
//  //impl AtomicRef for u64 {}

//  #[derive(Serialize, Deserialize)]
//  pub struct Profile {
//      //#[atomic(id)] A profile is uniquly identified by the name
//      name: String,

//      profile_color: u64,
//      profile_meta: u64,

//      //#[atomic(field(timestamp))]
//      about_me: String,
//      //#[atomic(field(timestamp))]
//      pfp: u64,
//      //#[atomic(map(timestamp))]
//      sub: SubData,
//      //#[atomic(array(timestamp))]
//      messages: Vec<Message>
//  }

//  impl AtomicRef for Profile {
//      fn id(&self) -> Id {Id::hash(&self.name)}
//      fn sub(&mut self) -> Atomic<'_> {
//          Atomic::Map(BTreeMap::from([
//              (self.id().to_string(), Atomic::Field(field!((self.profile_color, self.profile_meta)))),
//              ("about_me".to_string(), Atomic::Field(field!(self.about_me))),
//              ("pfp".to_string(), Atomic::Field(field!(self.pfp))),
//              ("sub".to_string(), self.sub.sub()),
//              ("messages".to_string(), Atomic::Array(self.messages.iter_mut().map(|f| Atomic::Field(field!(*f))).collect())),
//          ]))
//      }

//      fn default_state(&self) -> State {
//          State::Map(BTreeMap::from([
//              (self.id().to_string(), State::now()),
//              ("about_me".to_string(), State::now()),
//              ("pfp".to_string(), State::now()),
//              ("sub".to_string(), self.sub.default_state()),
//              ("messages".to_string(), State::Array(self.messages.iter().enumerate().map(|(i, _)| (i as f32, State::now())).collect())),
//          ]))
//      }
//  }

//  #[derive(Serialize, Deserialize)]
//  pub struct Message {
//      //#[atomic(id)] //A message can be uniqly identified in any set by its created date
//      created: u64,
//      //#[atomic(timestamp)]
//      body: String,
//      //#[atomic(timestamp)]
//      status: bool
//  }
//  impl AtomicRef for Message {
//      fn id(&self) -> Id {Id::hash(&self.created)}

//      fn sub(&mut self) -> Atomic<'_> {
//          Atomic::Map(BTreeMap::from([
//              ("body".to_string(), Atomic::Field(field!(self.body))),
//              ("status".to_string(), Atomic::Field(field!(self.status))),
//          ]))
//      }

//      fn default_state(&self) -> State {
//          State::Map(BTreeMap::from([
//              ("body".to_string(), State::now()),
//              ("status".to_string(), State::now())
//          ]))
//      }
//  }

//  #[derive(Serialize, Deserialize)]
//  pub struct SubData {
//      //#[atomic(id)]
//      identity: Id,
//      //#[atomic(timestamp)]
//      hello: String
//  }
//  impl AtomicRef for SubData {
//      fn id(&self) -> Id {self.identity}
//      fn sub(&mut self) -> Atomic<'_> {
//          Atomic::Map(BTreeMap::from([("hello".to_string(), Atomic::Field(field!(self.hello)))]))
//      }

//      fn default_state(&self) -> State {State::Map(BTreeMap::from([("hello".to_string(), State::now())]))}
//  }
////#[derive(PartialOrd, PartialEq)]
////struct Ordf32(f32);
////impl Eq for Ordf32 {}
////impl Ord for Ordf32 {
////    fn cmp(&self, other: &Self) -> Ordering {self.0.total_cmp(&other.0)}
////}

////impl Serialize for Ordf32 {
////    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
////        self.0.serialize(serializer)
////    }
////}

////impl<'de> Deserialize<'de> for Ordf32 {
////    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
////        Ok(Ordf32(f32::deserialize(deserializer)?))
////    }
////}

////struct AtomicVec<T>(Vec<f32>, Vec<T>);
////impl<'a, T> Deref for AtomicVec<T> {
////    type Target = Vec<T>;
////    fn deref(&self) -> &Self::Target {&self.1}
////}

////impl<T: Serialize> Serialize for AtomicVec<T> {
////    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
////        self.0.iter().zip(self.1.iter()).map(|(k, v)| (Ordf32(*k), v)).collect::<BTreeMap<Ordf32, &T>>().serialize(serializer)
////    }
////}

////impl<'de, T: for<'a> Deserialize<'a>> Deserialize<'de> for AtomicVec<T> {
////    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
////        let map = BTreeMap::<Ordf32, T>::deserialize(deserializer)?;
////        let (keys, values) = map.into_iter().map(|(k, v)| (k.0, v)).unzip();
////        Ok(AtomicVec(keys, values))
////    }
////}



//  struct AtomicGuard<'a, T: AsAtomic>(&'a mut T, &'a mut StateVector);
//  impl<'a, T: AsAtomic> Deref for AtomicGuard<'a, T> {
//      type Target = T;
//      fn deref(&self) -> &Self::Target {&self.0}
//  }
//  impl<'a, T: AsAtomic> DerefMut for AtomicGuard<'a, T> {
//      fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {&mut self.0}
//  }
//  impl<'a, T: AsAtomic> Drop for AtomicGuard<'a, T> {
//      fn drop(&mut self) {
//          self.0.update(self.1).unwrap()
//      }
//  }




#[async_trait::async_trait]
trait Location {
    async fn get_remote_state<A: Atomic>(&mut self, state: &StateVector) -> Result<StateVector, Error>;
}
#[async_trait::async_trait]
impl Location for Connection {
  //fn get_state<S: State>(&mut self, path: RecordPath) -> Request<S> {}
  //fn get<V: for<'a> Deserialize<'a>>(&mut self, path: RecordPath) -> Request<A>;
  //fn set<V: Serialize>(&mut self, path: RecordPath, value: V) -> Request<Result<(), Error>>;

  //async fn sync<A: Atomic>(&mut self, atomic: &mut A) -> Result<(), Error> {
  //    let state = atomic.state();
  //    match state {
  //        StateVector::Field(state) =>
  //        StateVector::Map(map, extendable) =>
  //    }
  //    //1. Request all states in cache to form a StateVector
  //    //2. Get state vector from atomic
  //    //3. Compare to get a list of read and writes required
  //    //4. Apply reads to atomic
  //    Ok(())
  //}

    async fn get_remote_state(&mut self, path: &RecordPath, state: &StateVector) -> Result<StateVector, Error> {
        match state {
            StateVector::Field(state) => {
                //1. Create tabel path (data: String, state: String)
                //2. Read state from path 
                //3. Panic If state is none
            }
            StateVector::Map(map, extendable) => {
                true => {
                    //If extendable is true all sub types are the same
                    //1. create table path (id: Id, and for each key if field( field: Data, state_field: State, for map key: TableId)
                    //2. read * from path
                    //3. For maps append table_id and read again
                },
                false => {
                    //1. Create table path (... and for each key if field( field: Data, state_field: State, for map key: TableId
                    //2. Read * from path
                    //3. For maps append table_id and read again
                }
            }
        }
        todo!()
    }
}

trait Atomic: Debug {
    //fn id(&self) -> Id;
    //fn merge(&mut self, other: Self) where Self: Sized;
    fn state(&self) -> StateVector;
}

trait State: Any {
    //fn cmp(&self, other: &Self) -> Ordering where Self: Sized;
}

pub enum StateVector {
    Field(Box<dyn State>),
    Map(BTreeMap<String, Self>),
}


#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
struct StaticState(u64);
impl State for StaticState {
}

#[derive(Serialize, Deserialize, Debug)]
struct Static<T>(T, StaticState);
impl<T: Serialize> From<T> for Static<T> {
    fn from(t: T) -> Static<T> {
        let hash = gxhash::gxhash64(&serde_json::to_vec(&t).unwrap(), 0);
        Static(t, StaticState(hash))
    }
}
impl<T: Serialize> Deref for Static<T> {
    type Target = T; fn deref(&self) -> &T {&self.0}
}
impl<T: Serialize + Debug> Atomic for Static<T> {
    //fn id(&self) -> Id {Id::hash(&std::any::type_name_of_val(self).to_string())}
    fn state(&self) -> StateVector {StateVector::Field(Box::new(self.1))}
}
//      fn merge(&mut self, mut other: Self) {
//          if gxhash::gxhash64(&serde_json::to_vec(&self).unwrap(), 0) != gxhash::gxhash64(&serde_json::to_vec(&other).unwrap(), 0) {
//              panic!("Panic State Differed")
//          }
//      }

//      fn sync(&mut self) {
//          
//      }
//  }


//All atomics are well defined at any level
//A Map inside A Map is represented as a pointer to another tabel/air parent
//Growable objects always grow height wise but never add new fields
//
//A BTreeMap is a Key value table 
//A BTreeMap of objects is a Key to table Id store

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
struct DateState(u64, u64);
impl State for DateState {
}

#[derive(Serialize, Deserialize, Debug)]
struct Dated<T>(T, u64, DateState);
impl<T: Serialize> Dated<T> {
    fn _ref(&mut self) {
        if self.1 > 0 {
            if self.2.0 != gxhash::gxhash64(&serde_json::to_vec(&self.0).unwrap(), 0) {
                self.2.1 = self.1;
            }
            self.1 = 0;
        }
    }
    fn _mut(&mut self) {
        self._ref();
        self.1 = now().timestamp() as u64;
    }
}
impl<T: Serialize> From<T> for Dated<T> {
    fn from(t: T) -> Dated<T> {
        let hash = gxhash::gxhash64(&serde_json::to_vec(&t).unwrap(), 0);
        Dated(t, 0, DateState(now().timestamp() as u64, hash))
    }
}
impl<T: Serialize> Deref for Dated<T> {
    type Target = T; fn deref(&self) -> &T {&self.0}
}
impl<T: Serialize> DerefMut for Dated<T> {
    fn deref_mut(&mut self) -> &mut T {self._mut(); &mut self.0}
}
impl<T: Serialize + Debug> Atomic for Dated<T> {
    //fn id(&self) -> Id {Id::hash(&std::any::type_name_of_val(self).to_string())}
    fn state(&self) -> StateVector {StateVector::Field(Box::new(self.2))}
}
//      fn merge(&mut self, mut other: Self) {
//          self._ref();
//          other._ref();
//          match self.2.cmp(&other.2) {
//              Ordering::Less => {
//                  self.2 = other.2;
//                  self.3 = other.3;
//              },
//              Ordering::Equal if self.3.cmp(&other.3) == Ordering::Less => {
//                  self.2 = other.2;
//                  self.3 = other.3;
//              },
//              _ => {}
//          }
//      }
//  }

//  struct Set<
//      K: Serialize + for<'a> Deserialize<'a>,
//      V: Serialize + for<'a> Deserialize<'a>, 
//  >(BTreeMap<K, V>, Box<dyn Fn(&V) -> K>);
//  impl<
//      K: Serialize + for<'a> Deserialize<'a> + Ord,
//      V: Serialize + for<'a> Deserialize<'a> + Atomic, 
//  > Set<K, V> {
//      fn new(key: impl Fn(&V) -> K + 'static) -> Self {Set(BTreeMap::default(), Box::new(key))}
//      fn insert(&mut self, value: impl Into<V>) {
//          let value = value.into();
//          match self.0.entry((self.1)(&value)) {
//              Entry::Occupied(mut entry) => entry.get_mut().merge(value),
//              Entry::Vacant(entry) => {entry.insert(value);}
//          }
//      }
//  }


//#[derive(Atomic)]
struct Room {
    id: Static<Id>,
    name: Dated<String>,
    messages: BTreeMap<Id, Message>
}

//#[derive(Atomic)]
#[derive(Debug)]
struct Message {
    id: Static<Id>,
    author: Static<String>,
    body: Dated<String> 
}
impl Message {
    fn new(id: Id, author: String, body: String) -> Message {
        Message {
            id: id.into(),
            author: author.into(),
            body: body.into()
        }
    }
}

impl Atomic for Message {
    //fn id(&self) -> Id {*self.id}//Message does not need id because its contaained by either the
    //sync call or the messages map in room
    fn state(&self) -> StateVector {
        StateVector::Map(BTreeMap::from([
            ("id".to_string(), self.id.state()),
            ("author".to_string(), self.author.state()),
            ("body".to_string(), self.body.state()),
        ]), false)
    }
}


mod test {
    use super::{Cache, RecordPath, Id, Message, Atomic};

    #[tokio::test]
    async fn test() {

        let mut message = Message::new(
            Id::random(),
            "me".to_string(),
            "Hello".to_string(),
        );
        //Cache::new().sync(RecordPath::root(), &mut message).await.unwrap();

      //*message.body = "Goodbye".to_string();
      //println!("atomic: {:?}", message);

        panic!("done");
    }
}

//  // Single String Example 
//  //[derive(Atomic)]
//  //struct Wrapper(Id, #[atomic(DateState)] String);
//  //Atomic<Wrapper>
//  //
//  //Cache
//  //Table Id 
//  //String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper(Id)
//  // /index/0 -> DateState 
//  // /index/0/0 -> String 

//  // HashSet of Strings
//  //#[derive(Atomic)]
//  //struct Wrapper2(Id, #[atomic(DateState)] HashSet<String>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper2(Id)
//  // /index/u64 -> DateState
//  // /index/u64/0 -> String 

//  // HashSet of Strings + bool
//  //#[derive(Atomic)]
//  //struct Wrapper2(Id, #[atomic(DateState)] HashSet<String>, bool)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //Hash Tabel Id, bool
//  //
//  //Tabel Id/Hash Tabel Id
//  //u64, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper2(Id)
//  // /index/0 -> HashSet<String>
//  // /index/0/u64 -> DateState
//  // /index/0/u64/0 -> String 
//  // /index/1/ -> DateState
//  // /index/1/0 -> bool 


//  // HashSet of Wrapper as field
//  //#[derive(Atomic)]
//  //struct Wrapper3(Id, #[atomic(DateState)] HashSet<Wrapper>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, Wrapper, DateState
//  //
//  //Air Server
//  // /index -> Wrapper3(Id)
//  // /index/u64 -> DateState
//  // /index/u64/0 -> Wrapper 

//  // HashSet of Wrapper as child
//  //#[derive(Atomic)]
//  //struct Wrapper3(#[atomic_id] Id, #[atomic_child] HashSet<Wrapper>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, Id, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper3(Id)
//  // /index/u64/ -> Wrapper(id)
//  // /index/u64/0 -> DateState
//  // /index/u64/0/0 -> String 

//  // HashSet of Wrapper as child and self field
//  //#[derive(Atomic)]
//  //struct Wrapper3(#[atomic_id] Id, #[atomic_self] String, #[atomic_child] HashSet<Wrapper>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, Id, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper3(Id)
//  // /index/u64/ -> Wrapper(id)
//  // /index/u64/0 -> DateState
//  // /index/u64/0/0 -> String 




//  //Room efg
//  //efg/0 -> Fields (ent)
//  //efg/ent/0 -> RoomName
//  //efg/ent/1 -> RoomNameState
//  //efg/ent/0 -> RoomName
//  //efg/ent/1 -> RoomNameState
//  //efg/1 -> Authors (hed)
//  //efg/hed/0 -> Author
//  //efg/2 -> Messages (xyz)
//  //efg/xyz/0 -> Message
//  //
//  //
//  //Cache
//  //0 -> alpha
//  //1 -> bravo
//  //2 -> charlie
//  //
//  //
//  //Self
//  //0 -> echo
//  //1 -> alpha
//  //2 -> delta
//  //
//  //Cache has priority easy to reorder locally
//  //Result
//  //0 -> alpha
//  //1 -> bravo
//  //2 -> charlie
//  //3 -> echo
//  //4 -> delta
//  //
//  //
//  //
//  //HashSet<String> -> Field 
//  //HashSet<DateTime<String>> -> Map<u64, String/DateState>
//  //HashSet<Message> -> Map<u64, Message>//Hash to value
//  //
//  //Vec<String> -> Map<u64, Field<String>>//Index to value
//  //
//  //
//  //Assume field unless specified
//  //
//  //
//  //
//  //Mapings are what determines how fields get reconsiled
//  //Fields are useless on their own
//  //
//  //
//  //Mappings
//  //Vec -> Index to value
//  //HashSet -> Hash to value
//  //HashMap -> HashedKey to value
//  //BTreeMap -> Key to value
//  //KeyedMap -> (Function on value gets key) to value
//  //Struct -> String to value
//  //UnnamedStruct -> Index to value
//  //
//  //A State for a map is Key -> Field State
//  //
//  //DateState(u64, u64) ->
//  //  Diff/Change: Hash value
//  //  Merge: Greater date : Greater Hash
//  //  Tomb: Zeroed Hash
//  //
//  //Not a real state since it needs T itself
//  //MaxState(Option<T>) ->
//  //  Diff/Change: Ord value
//  //  Merge: Max value
//  //  Tomb: None 
//  //
//  //
//  //  Hash Set implies the state of its fields are only determined by hash
//  //  Merging of a HashSet
//  //
//  // Maps need to know if they changed to see if new keys need to be added or removed
//  // Sets do the same but on the values themselves, HashSet of object with interior mutability is not
//  // good because the if the Hash changes the object needs to "move" partial edits are useless
//  //
//  //Detect Changes:
//  //  Hash: Hash the the object and compare with previous hash
//  //  Ord: Compare object with previous object
//  //  Diff: Run diff algorithm
//  //  
//  //
//  //Order:
//  //  Hash: Hash the objects cmp hashes
//  //  Ord: Run cmp on the objects
//  //  Date: Compare the Date Modified
//  //  Len: cmp the len() of objects
//  //  Diff: cmp the number of changes to get from one state to the other
//  //  
//  //
//  //Merge:
//  //  Replace replace the lesser with the greater
//  //  Additive Add any the lesser to the greater
//  //  Extend modify the lesser to extend the greater
//  //  Max choose the greater
//  //  Min choose the lesser 
//  //  Panic if there is a merge
//  //
//  //
//  //  Maps:
//  //      Panic: On key differences panic
//  //      Extend: From the first Key and Value that differes Extend values onto the Map
//  //      Default: Add new keys keep track of removals as tombstones, On key conflict merge field
//  //
//  //  Field:
//  //      Diff/Order:
//  //          Hash,
//  //          Len,
//  //          Ord,
//  //          Diff
//  //
//  //      Merge:
//  //          Max,
//  //          Min,
//  //          Panic,
//  //          Child: Use AsAtomic Merge

//  trait HasId {fn id(&self) -> Id}
//  struct IdSet<T: HasId>(BTreeMap<Id, T>);
//  impl<T: HasId> IdSet<T> {}

//  #[derive(Atomic(Map(Panic, HashDateReplace)))]
//  struct Room {
//      #[atomic(Id)]
//      id: Id,
//      #[atomic(Field(HashDateReplace))]
//      name: String,
//      #[atomic(Map(Extend, Field(Hash, Panic)))]//Extends so never Merges
//      tags_prio: Vec<String>,
//      #[atomic(Map(Extend, Field(Hash, Child)))]//Extends so never Merges
//      tags_test: Vec<Messages>,
//      #[atomic(Map(Default, Panic))]
//      authors: HashSet<String>,
//      #[atomic(Map(Default, Map(Default, Map(Panic, HashDateReplace))))]
//      tags_id: BTreeMap<Id, BTreeMap<String, (u64, u64)>>,
//      #[atomic(HashHashAdditive)]//Hash keys to check for diff, Cmp hashes for Ord, Add any missing
//      messages: IdSet<Message>
//  }
//  //Structure 
//  //Room(Id):
//  //  name: String,
//  //  tags_prio:
//  //      index -> String
//  //  authors:
//  //      hash -> String
//  //  messages:
//  //      id -> Message
//  //
//  //State: Diff-Ord-Merge
//  //name: Hash-Date-Replace
//  //tags_prio: 
//  //  Index -> Hash-Date-Replace
//  //authors:
//  //  Hash -> Hash-Date-Replace
//  //messages:
//  //  Id -> Id-Date-Replace

//  struct Message {
//      id: Id,
//      author: String,
//      body: String
//  }

//  impl AsAtomic for Message {
//      fn state(&self) -> StateVector {
//          
//      }
//  }
//  //Structure
//  //Message(Id):
//  //  author: String,
//  //  body: String
//  //
//  //State: Diff-Ord-Merge
//  //author: Hash-Date-Replace


//  struct KeyedMap<K, T>(std::collections::BTreeMap<K, T>, Box<dyn Fn(&T) -> &K>);
//  impl<K, T> KeyedMap<K, T> {
//      fn new(key: impl Fn(&T) -> &K + 'static) -> Self {
//          KeyedMap(std::collections::BTreeMap::default(), Box::new(key))
//      }
//  }
//  impl<K, T: Serialize> Deref for KeyedMap<K, T> {
//      type Target = std::collections::BTreeMap<K, T>;
//      fn deref(&self) -> &Self::Target {&self.0}
//  }
//  impl<K, T: Serialize> DerefMut for KeyedMap<K, T> {
//      fn deref_mut(&mut self) -> &mut Self::Target {&mut self.0}
//  }

//  #[derive(Debug)]
//  struct Atomic<T>(T, StateVector);
//  impl<T: AsAtomic> Atomic<T> {
//      fn new(mut t: T) -> Self {
//          let s = t.state();
//          Atomic(t, s)
//      }

//      fn get_mut(&mut self) -> AtomicGuard<'_, T> {AtomicGuard(&mut self.0, &mut self.1)}
//  }
//  impl<T> Deref for Atomic<T> {
//      type Target = T;
//      fn deref(&self) -> &Self::Target {&self.0}
//  }

//  //  trait Merge: Debug {
//  //      fn merge(left: String, right: String, ordering: Ordering) -> String;
//  //  }

//  //  ///Equal should mean that left and right are the same
//  //  #[derive(Debug)]
//  //  pub struct ReplaceMerge;
//  //  impl ReplaceMerge {
//  //      fn merge<T>(left: T, right: T, ordering: Ordering) -> T {
//  //          match ordering {
//  //              Ordering::Greater | Ordering::Equal => left,
//  //              Ordering::Less => right,
//  //          }
//  //      }
//  //  }

//  //  #[derive(Debug)]
//  //  pub struct PanicMerge;
//  //  impl Merge for PanicMerge {
//  //      fn merge(left: String, right: String, ordering: Ordering) -> Result<String {
//  //          if ordering != Ordering::Equal {panic!("Panic Merge");} else {left}
//  //      }
//  //  }

//  trait State: Debug + Any {
//      //fn new(field: &T) -> Self where Self: Sized;

//      fn as_any(&self) -> &dyn Any;
//      fn as_any_mut(&mut self) -> &mut dyn Any;

//      fn cmp(&self, other: &dyn State) -> Ordering;
//      //fn update(&mut self, field: &T) -> Result<(), Error>;

//      //fn sync_local(&mut self, a: Box<dyn State>);
//      //fn sync<T: AsAtomic>(&mut self, a: &mut T, o: &mut dyn State, );
//  }

//  //  trait NTState: Debug + Any {
//  //      fn as_any(&self) -> &dyn Any;
//  //      fn as_any_mut(&mut self) -> &mut dyn Any;

//  //    //fn cmp(&self, other: &dyn NTState) -> Ordering;
//  //  }

//  //  impl<T: 'static> NTState for Box<dyn State<T>> {
//  //      fn as_any(&self) -> &dyn Any {(&**self as &dyn State<T>).as_any()}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {(&mut **self as &mut dyn State<T>).as_any_mut()}

//  //    //fn cmp(&self, other: &dyn State<T>) -> Ordering {S::cmp(self, other)}
//  //  }

//  ///Arrays are concidered to be a field since they have no unique way to identity items for merging
//  ///Maps can contain a single field that belongs to itself since folders double as files themselves 
//  #[derive(Debug)]
//  pub enum StateVector {
//      Field(Box<dyn State>),
//      Map(Option<Box<dyn State>>, BTreeMap<String, Self>)
//  }

//  impl<S: State> From<S> for StateVector {
//      fn from(state: S) -> Self {StateVector::Field(Box::new(state))}
//  }

//  impl StateVector {
//    //fn field<T, S: State<T>>(&self) -> Result<&S, Error> {
//    //    match self {
//    //        Self::Field(s) => Ok(s.as_any().downcast_ref::<S>().unwrap()),
//    //        Self::Map(_, _) => Err(Error::InvalidStateVector)
//    //    }
//    //}
//    //fn field_mut<T, S: State<T>>(&mut self) -> Result<&mut S, Error> {
//    //    match self {
//    //        Self::Field(s) => Ok(s.as_any_mut().downcast_mut::<S>().unwrap()),
//    //        Self::Map(_, _) => Err(Error::InvalidStateVector)
//    //    }
//    //}

//    //fn map_field<T, S: State<T>>(&self) -> Result<&S, Error> {
//    //    match self {
//    //        Self::Field(_) => Err(Error::InvalidStateVector),
//    //        Self::Map(state, _) => state.as_ref().ok_or(Error::InvalidStateVector)?.as_any().downcast_ref::<S>().ok_or(Error::InvalidStateVector)
//    //    }
//    //}
//      fn map_field_mut<S: State>(&mut self) -> Result<&mut S, Error> {
//          match self {
//              Self::Field(_) => Err(Error::InvalidStateVector),
//              Self::Map(state, _) => state.as_mut().ok_or(Error::InvalidStateVector)?.as_any_mut().downcast_mut::<S>().ok_or(Error::InvalidStateVector)
//          }
//      }

//    //fn map(&self, field: &str) -> Result<&Self, Error> {
//    //    match self {
//    //        Self::Field(_) => Err(Error::InvalidStateVector),
//    //        Self::Map(_, map) => map.get(field).ok_or(Error::InvalidStateVector)
//    //    }
//    //}
//    //fn map_mut(&mut self, field: &str) -> Result<&mut Self, Error> {
//    //    match self {
//    //        Self::Field(_) => Err(Error::InvalidStateVector),
//    //        Self::Map(_, map) => map.get_mut(field).ok_or(Error::InvalidStateVector)
//    //    }
//    //}

//    //fn get_map_mut(&mut self) -> Result<&mut BTreeMap<String, Self>, Error> {
//    //    match self {
//    //        Self::Field(_) => Err(Error::InvalidStateVector),
//    //        Self::Map(_, map) => Ok(map)
//    //    }
//    //}

//    //fn from_value<D: State>(value: &Value) -> Self {
//    //    match value {
//    //        Value::Object(map) => StateVector::Map(map.into_iter().map(|(k, v)| (k.clone(), StateVector::from_value::<D>(v))).collect()),
//    //        value => StateVector::Field(Box::new(D::new(value))),
//    //    }
//    //}
//  }

//  //  #[derive(Debug, Eq, PartialEq)]//DateHashState
//  //  struct DateState(u64, u64);
//  //  impl State for DateState {
//  //      fn as_any(&self) -> &dyn Any {self}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {self}

//  //      fn cmp(&self, other: &dyn State) -> Ordering {
//  //          let other: &Self = *(other as &dyn Any).downcast_ref().unwrap();
//  //          match self.0.cmp(&other.0) {
//  //              Ordering::Equal => self.1.cmp(&other.1),
//  //              ordering => ordering
//  //          }
//  //      }
//  //  }
//  //  impl<T: Serialize> From<&T> for DateState {
//  //      fn from(field: &T) -> Self {
//  //          DateState(now().timestamp_nanos() as u64, gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0))
//  //      }
//  //  }
//  //  impl<T: Serialize> AddAssign<&T> for DateState {
//  //      fn add_assign(&mut self, field: &T) {
//  //          let hash = gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0); 
//  //          if hash != self.1 {
//  //              self.0 = now().timestamp_nanos() as u64;
//  //              self.1 = hash;
//  //          }
//  //      }
//  //  }


//  //  struct<T> MutCheck<T>(T, bool);
//  //  impl<T> MutCheck<T> {
//  //      fn new(t: T, pre: impl Fn(&T), post: impl Fn(&T)) -> Self {
//  //          
//  //      }
//  //  }


//  #[derive(Debug, Eq, PartialEq)]//DateHashState
//  struct<T: Serialize> DateState<T>(T, u64, u64);
//  impl State for DateState {
//      fn as_any(&self) -> &dyn Any {self}
//      fn as_any_mut(&mut self) -> &mut dyn Any {self}

//      fn cmp(&self, other: &dyn State) -> Ordering {
//          let other: &Self = *(other as &dyn Any).downcast_ref().unwrap();
//          match self.0.cmp(&other.0) {
//              Ordering::Equal => self.1.cmp(&other.1),
//              ordering => ordering
//          }
//      }
//  }
//  impl<T: Serialize> From<&T> for DateState {
//      fn from(field: &T) -> Self {
//          DateState(now().timestamp_nanos() as u64, gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0))
//      }
//  }
//  impl<T: Serialize> AddAssign<&T> for DateState {
//      fn add_assign(&mut self, field: &T) {
//          let hash = gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0); 
//          if hash != self.1 {
//              self.0 = now().timestamp_nanos() as u64;
//              self.1 = hash;
//          }
//      }
//  }




//  //  pub struct HashState(u64);
//  //  fn state(&self) -> StateVector {
//  //      StateVector::Map(None, self.iter().map(|x| {
//  //          let mut hasher = gxhash::GxHasher::with_seed(0);
//  //          x.hash(&mut hasher);
//  //          ((hasher.finish_u128() as u64).to_string(), StateVector::Field(Box::new(Box::new(EmptyState) as Box<dyn State<T>>) as Box<dyn NTState>))
//  //      }).collect())
//  //  }
//  //  impl<T: Serialize> State<T> for DateState {
//  //      ///Create a new state based on some object
//  //      fn new(field: &T) -> Self {
//  //          DateState(now().timestamp_nanos() as u64, gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0))
//  //      }

//  //      ///If the object for this state could have been changed check and update the state
//  //      fn update(&mut self, field: &T) -> Result<(), Error> {
//  //          let hash = gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0); 
//  //          if hash != self.1 {
//  //              self.0 = now().timestamp_nanos() as u64;
//  //              self.1 = hash;
//  //          }
//  //          Ok(())
//  //      }

//  //      fn as_any(&self) -> &dyn Any {self}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {self}
//  //      ///If hashes are the same the lesser date is greater(This could cause issues if an object is
//  //      ///changed back and forth so the hashes match but the user decided on this at a later date
//  //      ///Otherwise the date determines the order if the dates are the same order by hash
//  //      fn cmp(&self, other: &dyn State<T>) -> Ordering {
//  //          let other: &Self = *(other as &dyn Any).downcast_ref().unwrap();
//  //          if self.1 == other.1 {other.0.cmp(&self.0)} else {match self.0.cmp(&other.0) {
//  //              Ordering::Equal => self.1.cmp(&other.1),
//  //              ordering => ordering
//  //          }}
//  //      }
//  //  }



//  //  #[derive(Debug, Eq, PartialEq)]//DateHashState
//  //  struct HashState(u64);
//  //  impl<T: Hash> State<T> for HashState {
//  //      fn new(field: &T) -> Self {
//  //          HashState(gxhash::gxhash64(&field, 0))
//  //      }

//  //      ///If the object for this state could have been changed check and update the state
//  //      fn update<T: Serialize>(&mut self, field: &T) -> Result<(), Error> {
//  //          let hash = gxhash::gxhash64(&serde_json::to_vec(field).unwrap(), 0); 
//  //          if hash != self.1 {
//  //              self.0 = now().timestamp_nanos() as u64;
//  //              self.1 = hash;
//  //          }
//  //          Ok(())
//  //      }

//  //      fn as_any(&self) -> &dyn Any {self}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {self}
//  //      ///If hashes are the same the lesser date is greater(This could cause issues if an object is
//  //      ///changed back and forth so the hashes match but the user decided on this at a later date
//  //      ///Otherwise the date determines the order if the dates are the same order by hash
//  //      fn cmp(&self, other: &dyn State) -> Ordering {
//  //          let other: &Self = *(other as &dyn Any).downcast_ref().unwrap();
//  //          if self.1 == other.1 {other.0.cmp(&self.0)} else {match self.0.cmp(&other.0) {
//  //              Ordering::Equal => self.1.cmp(&other.1),
//  //              ordering => ordering
//  //          }}
//  //      }
//  //  }

//  //  #[derive(Debug)]
//  //  struct MaxState<T: Debug + 'static>(T); //Only works on cmp objects
//  //  impl<T: Ord + Copy + Debug> State<T> for MaxState<T> {
//  //      fn new(field: &T) -> Self {MaxState(*field)}
//  //      fn update(&mut self, field: &T) -> Result<(), Error> {
//  //          self.0 = self.0.max(*field);
//  //          Ok(())
//  //      }
//  //      fn as_any(&self) -> &dyn Any {self}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {self}
//  //      ///If the other state is greater than this is lesser
//  //      fn cmp(&self, other: &dyn State<T>) -> Ordering {
//  //          let other: &Self = *(other as &dyn Any).downcast_ref().unwrap();
//  //          other.0.cmp(&self.0) 
//  //      }
//  //  }

//  //  #[derive(Debug)]
//  //  struct EmptyState;
//  //  impl<T: Debug> State<T> for EmptyState {
//  //      fn new(field: &T) -> Self {EmptyState}
//  //      fn update(&mut self, field: &T) -> Result<(), Error> {Ok(())}
//  //      fn as_any(&self) -> &dyn Any {self}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {self}
//  //      ///If the other state is greater than this is lesser
//  //      fn cmp(&self, other: &dyn State<T>) -> Ordering {Ordering::Equal}
//  //  }

//  pub enum AtomicRef<'a> {
//      Field(Field<'a>),
//      Map(Option<Field<'a>>, BTreeMap<String, Self>)
//  }

//  pub trait SyncLoc {

//  }

//  pub trait AsAtomic {
//      fn id(&self) -> Id;
//      ///Get initial state for object
//      fn state(&self) -> StateVector;
//      ///Update an exsiting old state
//      fn update(&self, state: &mut StateVector) -> Result<(), Error>;
//      //Sync this object to another location
//      //
//      //1. Read remote state and figure diff
//      //2. Read needed changes from remote
//      //3. Write needed changes to remote
//      //fn sync<E, L: SyncLoc<E>>(&self, sync_Loc: &mut L) -> Result<(), E>;
//  }

//  struct CacheSync;
//  impl SyncLoc for CacheSync {}


//  //  struct UAVecState<T>(Vec<T>);
//  //  impl<T> State for UAVecState<T> {
//  //      fn as_any(&self) -> &dyn Any {self}
//  //      fn as_any_mut(&mut self) -> &mut dyn Any {self}
//  //      ///If the other state is greater than this is lesser
//  //      fn cmp(&self, other: &dyn State) -> Ordering {
//  //          let other: &Self = *(other as &dyn Any).downcast_ref().unwrap();
//  //          
//  //          other.0.cmp(&self.0) 
//  //      }
//  //  }

//  //TESTS-------------------------------------------------------------

//  //#[derive(Serialize, Deserialize)] //derive(Atomic(DateState))
//  struct RoomMeta {
//      other_static: u64,
//      tags: Vec<String>
//  }

//  #[derive(Serialize, Deserialize)] //derive(Atomic)
//  struct Room {
//      //#[atomic(id)]
//      id: Id,
//      name: DateState<String>,
//      #[atomic(DateState)]
//      authors: HashSet<String>,
//      messages: BTreeMap<Id, Message>,
//  }

//  impl AsAtomic for Room {
//      fn id(&self) -> Id {self.id}
//      fn state(&self) -> StateVector {
//          StateVector::Map(None, BTreeMap::from([
//              ("name".to_string(), StateVector::Field(Box::new(DateState::from(&self.name)) as Box<dyn State>)),
//              //("authors".to_string(), HashState::from(&self.authors)),
//              //("messages".to_string(), StateVector::Field(Box::new(MaxState::new(&self.messages)))),
//          ]))
//      }

//      fn update(&self, state: &mut StateVector) -> Result<(), Error> {
//          //*state.map_field_mut::<DateState>().unwrap() += &self.authors;
//        //state.map_mut("id").unwrap().field_mut::<DateState>()?.update(&self.id).unwrap();
//        //state.map_mut("body").unwrap().field_mut::<DateState>()?.update(&self.body).unwrap();
//        //state.map_mut("change_idx").unwrap().field_mut::<MaxState<_>>()?.update(&self.change_idx).unwrap();
//          Ok(())
//      }
//  }

//  //Default impl for a HashSet of fields
//  //
//  impl<T: Hash + Debug + 'static> AsAtomic for HashSet<T> {
//      fn id(&self) -> Id {Id::hash(&"HashSet<T>".to_string())}
//      fn state(&self) -> StateVector {
//          StateVector::Map(self.iter().map(|x| {
//              let mut hasher = gxhash::GxHasher::with_seed(0);
//              x.hash(&mut hasher);
//              ((hasher.finish_u128() as u64).to_string(), StateVector::Field(Box::new(Box::new(EmptyState) as Box<dyn State<T>>) as Box<dyn NTState>))
//          }).collect())
//      }

//      fn update(&self, state: &mut StateVector) -> Result<(), Error> {
//          state.get_map_mut().unwrap()
//              //TODO: Tombstone removed states and add new ones
//        //state.map_field_mut::<DateState>().unwrap().update(&self.author).unwrap();
//        //state.map_mut("id").unwrap().field_mut::<DateState>()?.update(&self.id).unwrap();
//        //state.map_mut("body").unwrap().field_mut::<DateState>()?.update(&self.body).unwrap();
//        //state.map_mut("change_idx").unwrap().field_mut::<MaxState<_>>()?.update(&self.change_idx).unwrap();
//          Ok(())
//      }
//  }

//  #[derive(Serialize, Deserialize, Debug)] //derive(Atomic(DateState))
//  struct Message {
//      id: Id,
//      author: DateState<String>,
//      body: DateState<String>,
//      change_idx: MaxState<u64>
//  }

//  impl AsAtomic for Message {
//      fn id(&self) -> Id {self.id}
//      fn state(&self) -> StateVector {
//          StateVector::Map(Some(Box::new(DateState::from(&self.author)) as Box<dyn State>), BTreeMap::from([
//            //("id".to_string(), StateVector::Field(Box::new(Box::new(DateState::new(&self.id)) as Box<dyn State<Id>>) as Box<dyn NTState>)),
//            //("body".to_string(), StateVector::Field(Box::new(Box::new(DateState::new(&self.body)) as Box<dyn State<String>>))),
//            //("change_idx".to_string(), StateVector::Field(Box::new(Box::new(MaxState::new(&self.change_idx)) as Box<dyn State<u64>>))),
//          ]))
//      }

//      fn update(&self, state: &mut StateVector) -> Result<(), Error> {
//        //state.map_field_mut::<String, DateState>().unwrap().update(&self.author).unwrap();
//        //state.map_mut("id").unwrap().field_mut::<Id, DateState>().unwrap().update(&self.id).unwrap();
//        //state.map_mut("body").unwrap().field_mut::<String, DateState>().unwrap().update(&self.body).unwrap();
//        //state.map_mut("change_idx").unwrap().field_mut::<u64, MaxState<u64>>().unwrap().update(&self.change_idx).unwrap();
//          Ok(())
//      }
//  }

//  mod test {
//      use super::{Id, Message, Atomic};

//      #[test]
//      fn test() {

//          let message = Message{
//              author: "me".to_string(),
//              id: Id::random(),
//              body: "Hello".to_string(),
//              change_idx: 0
//          };

//          let mut atomic = Atomic::new(message);
//          let mut m = atomic.get_mut();
//          m.body = "Goodbye".to_string();
//          m.change_idx = 1;
//          drop(m);
//          println!("atomic: {:?}", atomic);


//        //let mut room = Atomic::new(Room{
//        //    id: Id::random(),
//        //    other_static: 290,
//        //    name: "Hello".to_string(),
//        //    message: Message {
//        //        id: Id::random(),
//        //        body: "Goodbye".to_string(),
//        //        author: "Me".to_string() 
//        //    }
//        //});


//        //let mut r = room.get_mut();

//        //r.id = Id::random();

//        //let mut wrapper = Atomic::new(Wrapper("Hello".to_string()));
//        //println!("wrapper: {:?}", wrapper);
//        //let mut w = wrapper.get_mut();
//        //(*w).0 = "Goodbye".to_string();
//        //drop(w);
//        //println!("wrapper: {:?}", wrapper);

//        //let state = room.state();
//        //let mut atomic = _Atomic(room, state);
//        //std::thread::sleep(std::time::Duration::from_secs(1));
//        //let mut a = atomic.get_mut();
//        //a.name = "test".to_string();
//        //drop(a);
//        //println!("atomic state: {:#?}", atomic.1);
//          panic!("done");
//          //state.update(a.state());
//      }
//  }

//  // Single String Example 
//  //[derive(Atomic)]
//  //struct Wrapper(Id, #[atomic(DateState)] String);
//  //Atomic<Wrapper>
//  //
//  //Cache
//  //Table Id 
//  //String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper(Id)
//  // /index/0 -> DateState 
//  // /index/0/0 -> String 

//  // HashSet of Strings
//  //#[derive(Atomic)]
//  //struct Wrapper2(Id, #[atomic(DateState)] HashSet<String>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper2(Id)
//  // /index/u64 -> DateState
//  // /index/u64/0 -> String 

//  // HashSet of Strings + bool
//  //#[derive(Atomic)]
//  //struct Wrapper2(Id, #[atomic(DateState)] HashSet<String>, bool)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //Hash Tabel Id, bool
//  //
//  //Tabel Id/Hash Tabel Id
//  //u64, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper2(Id)
//  // /index/0 -> HashSet<String>
//  // /index/0/u64 -> DateState
//  // /index/0/u64/0 -> String 
//  // /index/1/ -> DateState
//  // /index/1/0 -> bool 


//  // HashSet of Wrapper as field
//  //#[derive(Atomic)]
//  //struct Wrapper3(Id, #[atomic(DateState)] HashSet<Wrapper>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, Wrapper, DateState
//  //
//  //Air Server
//  // /index -> Wrapper3(Id)
//  // /index/u64 -> DateState
//  // /index/u64/0 -> Wrapper 

//  // HashSet of Wrapper as child
//  //#[derive(Atomic)]
//  //struct Wrapper3(#[atomic_id] Id, #[atomic_child] HashSet<Wrapper>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, Id, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper3(Id)
//  // /index/u64/ -> Wrapper(id)
//  // /index/u64/0 -> DateState
//  // /index/u64/0/0 -> String 

//  // HashSet of Wrapper as child and self field
//  //#[derive(Atomic)]
//  //struct Wrapper3(#[atomic_id] Id, #[atomic_self] String, #[atomic_child] HashSet<Wrapper>)
//  //Atomic<Wrapper2>
//  //
//  //Cache
//  //Tabel Id
//  //u64, Id, String, DateState
//  //
//  //Air Server
//  // /index -> Wrapper3(Id)
//  // /index/u64/ -> Wrapper(id)
//  // /index/u64/0 -> DateState
//  // /index/u64/0/0 -> String 




//  //Room efg
//  //efg/0 -> Fields (ent)
//  //efg/ent/0 -> RoomName
//  //efg/ent/1 -> RoomNameState
//  //efg/ent/0 -> RoomName
//  //efg/ent/1 -> RoomNameState
//  //efg/1 -> Authors (hed)
//  //efg/hed/0 -> Author
//  //efg/2 -> Messages (xyz)
//  //efg/xyz/0 -> Message
//  //
//  //
//  //Cache
//  //0 -> alpha
//  //1 -> bravo
//  //2 -> charlie
//  //
//  //
//  //Self
//  //0 -> echo
//  //1 -> alpha
//  //2 -> delta
//  //
//  //Cache has priority easy to reorder locally
//  //Result
//  //0 -> alpha
//  //1 -> bravo
//  //2 -> charlie
//  //3 -> echo
//  //4 -> delta
//  //
//  //
//  //
//  //HashSet<String> -> Field 
//  //HashSet<DateTime<String>> -> Map<u64, String/DateState>
//  //HashSet<Message> -> Map<u64, Message>//Hash to value
//  //
//  //Vec<String> -> Map<u64, Field<String>>//Index to value
//  //
//  //
//  //Assume field unless specified
//  //
//  //
//  //
//  //Mapings are what determines how fields get reconsiled
//  //Fields are useless on their own
//  //
//  //
//  //Mappings
//  //Vec -> Index to value
//  //HashSet -> Hash to value
//  //HashMap -> HashedKey to value
//  //BTreeMap -> Key to value
//  //KeyedMap -> (Function on value gets key) to value
//  //Struct -> String to value
//  //UnnamedStruct -> Index to value
//  //
//  //A State for a map is Key -> Field State
//  //
//  //DateState(u64, u64) ->
//  //  Diff/Change: Hash value
//  //  Merge: Greater date : Greater Hash
//  //  Tomb: Zeroed Hash
//  //
//  //Not a real state since it needs T itself
//  //MaxState(Option<T>) ->
//  //  Diff/Change: Ord value
//  //  Merge: Max value
//  //  Tomb: None 
//  //
//  //
//  //  Hash Set implies the state of its fields are only determined by hash
//  //  Merging of a HashSet
//  //
//  // Maps need to know if they changed to see if new keys need to be added or removed
//  // Sets do the same but on the values themselves, HashSet of object with interior mutability is not
//  // good because the if the Hash changes the object needs to "move" partial edits are useless
//  //
//  //Detect Changes:
//  //  Hash: Hash the the object and compare with previous hash
//  //  Ord: Compare object with previous object
//  //  Diff: Run diff algorithm
//  //  
//  //
//  //Order:
//  //  Hash: Hash the objects cmp hashes
//  //  Ord: Run cmp on the objects
//  //  Date: Compare the Date Modified
//  //  Len: cmp the len() of objects
//  //  Diff: cmp the number of changes to get from one state to the other
//  //  
//  //
//  //Merge:
//  //  Replace replace the lesser with the greater
//  //  Additive Add any the lesser to the greater
//  //  Extend modify the lesser to extend the greater
//  //  Max choose the greater
//  //  Min choose the lesser 
//  //  Panic if there is a merge
//  //
//  //
//  //  Maps:
//  //      Panic: On key differences panic
//  //      Extend: From the first Key and Value that differes Extend values onto the Map
//  //      Default: Add new keys keep track of removals as tombstones, On key conflict merge field
//  //
//  //  Field:
//  //      Diff/Order:
//  //          Hash,
//  //          Len,
//  //          Ord,
//  //          Diff
//  //
//  //      Merge:
//  //          Max,
//  //          Min,
//  //          Panic,
//  //          Child: Use AsAtomic Merge

//  trait HasId {fn id(&self) -> Id}
//  struct IdSet<T: HasId>(BTreeMap<Id, T>);
//  impl<T: HasId> IdSet<T> {}

//  #[derive(Atomic(Map(Panic, HashDateReplace)))]
//  struct Room {
//      #[atomic(Id)]
//      id: Id,
//      #[atomic(Field(HashDateReplace))]
//      name: String,
//      #[atomic(Map(Extend, Field(Hash, Panic)))]//Extends so never Merges
//      tags_prio: Vec<String>,
//      #[atomic(Map(Extend, Field(Hash, Child)))]//Extends so never Merges
//      tags_test: Vec<Messages>,
//      #[atomic(Map(Default, Panic))]
//      authors: HashSet<String>,
//      #[atomic(Map(Default, Map(Default, Map(Panic, HashDateReplace))))]
//      tags_id: BTreeMap<Id, BTreeMap<String, (u64, u64)>>,
//      #[atomic(HashHashAdditive)]//Hash keys to check for diff, Cmp hashes for Ord, Add any missing
//      messages: IdSet<Message>
//  }
//  //Structure 
//  //Room(Id):
//  //  name: String,
//  //  tags_prio:
//  //      index -> String
//  //  authors:
//  //      hash -> String
//  //  messages:
//  //      id -> Message
//  //
//  //State: Diff-Ord-Merge
//  //name: Hash-Date-Replace
//  //tags_prio: 
//  //  Index -> Hash-Date-Replace
//  //authors:
//  //  Hash -> Hash-Date-Replace
//  //messages:
//  //  Id -> Id-Date-Replace

//  struct Message {
//      id: Id,
//      author: String,
//      body: String
//  }

//  impl AsAtomic for Message {
//      fn state(&self) -> StateVector {
//          
//      }
//  }
//  //Structure
//  //Message(Id):
//  //  author: String,
//  //  body: String
//  //
//  //State: Diff-Ord-Merge
//  //author: Hash-Date-Replace
//  //body: Hash-Date-Replace
