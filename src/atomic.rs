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
use serde_json::Value;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use air::{DateTime, now, storage::records::RecordPath};

use crate::hardware::cache::{RustSqlite, Cache, Connection};
use crate::Id;

#[derive(Debug)]
pub enum Error {
    InvalidStateVector,
    SqliteError(rusqlite::Error),
    FromSqliteError(rusqlite::types::FromSqlError)
}
impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{:?}", self)}
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Error {Error::SqliteError(error)}
}
impl From<rusqlite::types::FromSqlError> for Error {
    fn from(error: rusqlite::types::FromSqlError) -> Error {Error::FromSqliteError(error)}
}

macro_rules! smatch {
    ($v:expr, $p:pat, $e:expr) => {
        match $v {
            $p => Some($e),
            _ => None
        }
    }
}

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

impl<'a> Debug for Field<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Field<'a>")
    }
}


#[async_trait::async_trait(?Send)]
trait Location {
    async fn sync_remote<A: Atomic>(&mut self, name: String, atomic: &mut A) -> Result<(), Error>;
    async fn get_remote<A: Atomic>(&mut self, name: String) -> Result<Option<A>, Error>;
}
#[async_trait::async_trait(?Send)]
impl Location for Cache {
    async fn sync_remote<A: Atomic>(&mut self, name: String, atomic: &mut A) -> Result<(), Error> {
        self.lock(|db| db.sync_state(name, "0".to_string(), atomic.state())).await
    }

    async fn get_remote<A: Atomic>(&mut self, name: String) -> Result<Option<A>, Error> {
        Ok(Some(A::from_raw(self.lock(|db| db.get_raw(name, "0".to_string(), A::atomic_type())).await?)))
    }
}

trait _Location {
    fn sync_state(&self, path: String, idx: String, state: StateVector<'_>) -> Result<(), Error>;
    fn get_raw(&self, path: String, idx: String, ty: AtomicType) -> Result<RawAtomic, Error>;
}
impl _Location for Connection {
    fn get_raw(&self, path: String, idx: String, ty: AtomicType) -> Result<RawAtomic, Error> {
        Ok(match ty {
            AtomicType::Field => {
                let cmd = format!("SELECT __state__, data FROM {} where __idx__ = '{}'", path, idx);

                self.query_row(
                    &cmd, [], |row| Ok(RawAtomic::Field(
                        row.get::<_, String>("__state__").unwrap(),
                        row.get::<_, String>("data").unwrap()
                    ))
                ).unwrap()
            }
            AtomicType::Map(map) => {
                let sub_type = *map;
                match matches!(sub_type, AtomicType::Map(_)) {
                    true => {todo!()},
                    false => {
                        let cmd = format!("SELECT __idx__ FROM {}", path);
                        RawAtomic::Map(
                            self.prepare(&cmd).unwrap().query([]).unwrap()
                            .map(|row| row.get(0)).collect::<Vec<String>>().unwrap()
                            .into_iter().map(|k| 
                                Ok((k.clone(), self.get_raw(path.clone(), k, sub_type.clone())?))
                            ).collect::<Result<_, Error>>()?
                        )
                    }
                }
            },
            AtomicType::Struct(map) => {
                let mut results = BTreeMap::default();
                let mut fields = Vec::new();
                for (k, v) in map.into_iter() {match v {
                    AtomicType::Field => {fields.push(k.clone());},
                    other => {results.insert(k.to_string(), self.get_raw(path.clone()+&k, "0".to_string(), other)?);}
                }}
                let cmd = format!(
                    "SELECT {} FROM {} where __idx__ = '{}'",
                    fields.iter().map(|k| format!("__state__{}, {}", k, k)).collect::<Vec<_>>().join(", "),
                    path, idx
                );
                results.extend(self.query_row(
                    &cmd, [], |row| Ok(fields.into_iter().map(|k| (k.clone(), RawAtomic::Field(
                        row.get::<_, String>(format!("__state__{}", k).as_str()).unwrap(),
                        row.get::<_, String>(k.as_str()).unwrap()
                    ))).collect::<Vec<_>>())
                ).unwrap());
                RawAtomic::Struct(results)
            }
        })
    }

    fn sync_state(&self, path: String, idx: String, state: StateVector<'_>) -> Result<(), Error> {
        match state {
            StateVector::Field(state, mut field) => {
                let cmd = format!("CREATE TABLE if not exists {}(__idx__ TEXT UNIQUE, data TEXT, __state__ TEXT);", path);
                self.execute(&cmd, []).unwrap();

                let cmd = format!("SELECT __state__ FROM {} where __idx__ = '{}'", path, idx);

                let order = self.query_row(
                    &cmd, [], |row| Ok(state.merge(Some(row.get_ref("__state__").unwrap().as_bytes().unwrap())))
                ).optional().unwrap().unwrap_or_else(|| state.merge(None));
                match order {
                    Ordering::Greater => {
                        let cmd = format!("INSERT INTO {}(__idx__, data, __state__) VALUES (?1, ?2, ?3) ON CONFLICT(__idx__) DO UPDATE SET data=excluded.data, __state__=excluded.__state__;", path);
                        self.execute(&cmd, [idx, field.read(), state.serialize()]).unwrap();
                    }
                    Ordering::Equal => {}
                    Ordering::Less => {
                        let cmd = format!("SELECT data FROM {} where __idx__ = '{}'", path, idx);
                        self.query_row(
                            &cmd, [], |row| Ok(field.write(row.get_ref("data").unwrap().as_str().unwrap()))
                        ).optional().unwrap().unwrap();
                    }
                }
                Ok(())
            }
            StateVector::Map(mut map) => {
                let sub_type = map.sub_type();
                match matches!(sub_type, AtomicType::Map(_)) {
                    true => {
                        let cmd = format!("CREATE TABLE if not exists {}(__idx__ TEXT UNIQUE, key TEXT);", path);
                        self.execute(&cmd, []).unwrap();
                        let state = map.state_vector();
                        let keys = state.keys().map(|k| k.clone()).collect::<Vec<_>>();
                        state.into_iter().map(|(k, v)|
                            self.sync_state(path.clone()+&k, "0".to_string(), v)
                        ).collect::<Result<(), Error>>()?;

                        let cmd = format!("SELECT __idx__ FROM {}", path.clone());
                        let keys = self.prepare(&cmd).unwrap().query([]).unwrap()
                            .map(|row| row.get(0))
                            .filter(|idx| Ok(!keys.contains(idx)))
                            .collect::<Vec<_>>().unwrap();
                        for key in keys {
                            map.insert(key.clone(), self.get_raw(path.clone()+&key, "0".to_string(), sub_type.clone())?);
                        }
                        Ok(())
                    },
                    false => {
                        let mut state = map.state_vector();
                        let keys = state.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();

                        state.into_iter().map(|(k, v)| self.sync_state(path.clone(), k, v)).collect::<Result<(), Error>>()?;

                        let cmd = format!("SELECT __idx__ FROM {}", path);
                        let keys = self.prepare(&cmd).unwrap().query([]).unwrap()
                            .map(|row| row.get(0))
                            .filter(|idx| Ok(!keys.contains(idx)))
                            .collect::<Vec<_>>().unwrap();
                        
                        for key in keys {
                            map.insert(key.clone(), self.get_raw(path.clone(), key, sub_type.clone())?);
                        }
                        Ok(())
                    }
                }
            },
            StateVector::Struct(mut map) => {
                let cmd = format!(
                    "CREATE TABLE if not exists {}(__idx__ TEXT UNIQUE, {});", path,
                    map.iter().filter_map(|(k, v)| match v {
                        StateVector::Field(_, _) => Some(format!("{} TEXT, __state__{} TEXT", k, k)),
                        _ => None
                    }).collect::<Vec<_>>().join(", ")
                );

                println!("cmd: {}", cmd);
                self.execute(&cmd, []).unwrap();

                let mut fields = map.iter_mut().filter_map(|(k, v)|
                    smatch!(v, StateVector::Field(s,f), (k, s, f))
                ).collect::<Vec<_>>();

                let cmd = format!(
                    "SELECT {} FROM {} where __idx__ = '{}'",
                    fields.iter().map(|k| format!("__state__{}", k.0)).collect::<Vec<_>>().join(", "),
                    path, idx
                );

                let ordering = self.query_row(&cmd, [], |row| Ok(fields.iter_mut().map(|(k, s, f)| {
                    s.merge(Some(
                        row.get_ref(format!("__state__{}", k).as_str()).unwrap().as_bytes().unwrap()
                    ))
                }).collect::<Vec<_>>())).optional().unwrap().unwrap_or_else(|| {
                    println!("NONE FOUND");
                    fields.iter_mut().map(|(k, s, f)| s.merge(None)).collect::<Vec<_>>()
                });
                println!("ordering: {:?}", ordering);
                fields.iter_mut().zip(ordering).for_each(|((k, s, f), order)| match order {
                    Ordering::Greater => {
                        let cmd = format!("INSERT INTO {}(__idx__, {}, __state__{}) VALUES (?1, ?2, ?3) ON CONFLICT(__idx__) DO UPDATE SET {}=excluded.{}, __state__{}=excluded.__state__{};", path, k, k, k, k, k, k);
                        self.execute(&cmd, [idx.clone(), f.read(), s.serialize()]).unwrap();
                    }
                    Ordering::Equal => {}
                    Ordering::Less => {
                        let cmd = format!("SELECT {} FROM {} where __idx__ = '{}'", k, path, idx);
                        self.query_row(
                            &cmd, [], |row| Ok(f.write(row.get_ref(k.as_str()).unwrap().as_str().unwrap()))
                        ).optional().unwrap().unwrap();
                    }
                });
                map.into_iter().map(|(k, v)| match v {
                    StateVector::Field(_, _) => Ok(()),
                    child => self.sync_state(format!("{}{}", path, k), "0".to_string(), child)
                }).collect::<Result<(), Error>>()
            }
        }
    }
}

trait Atomic: Debug {
    fn name() -> String where Self: Sized {"Atomic".to_string()}
    fn state(&mut self) -> StateVector;
    fn from_raw(raw: RawAtomic) -> Self where Self: Sized;
    fn atomic_type() -> AtomicType where Self: Sized;
}

trait State: Debug + Any + Sync + Send {
    fn merge(&mut self, bytes: Option<&[u8]>) -> Ordering;
    fn serialize(&self) -> String;
}

trait AtomicMap: Debug {
    fn state_vector(&mut self) -> BTreeMap<String, StateVector<'_>>;
    fn sub_type(&self) -> AtomicType;
    fn insert(&mut self, key: String, value: RawAtomic);
}

#[derive(Debug)]
pub enum RawAtomic {
    Field(String, String),
    Struct(BTreeMap<String, Self>),//Struct well defined finite keys
    Map(BTreeMap<String, Self>),
}

#[derive(Debug)]
pub enum StateVector<'a> {
    Field(&'a mut dyn State, Field<'a>),
    Struct(BTreeMap<String, Self>),//Struct well defined finite keys
    Map(&'a mut dyn AtomicMap),
}

#[derive(Debug, Clone)]
pub enum AtomicType {
    Field,
    Struct(BTreeMap<String, Self>),//Struct well defined finite keys
    Map(Box<Self>),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
struct StaticState(u64);
impl State for StaticState {
    fn merge(&mut self, bytes: Option<&[u8]>) -> Ordering {
        match bytes {
            Some(bytes) => {
                let remote = serde_json::from_slice::<Self>(bytes).unwrap();
                if self.0 == remote.0 {Ordering::Equal} else {
                    panic!("Static State Object Mutated");
                }
            },
            None => Ordering::Greater
        }
    }

    fn serialize(&self) -> String {serde_json::to_string(self).unwrap()}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
impl<T: Serialize + for<'a> Deserialize<'a> + Debug> Atomic for Static<T> {
    //fn id(&self) -> Id {Id::hash(&std::any::type_name_of_val(self).to_string())}
    fn state(&mut self) -> StateVector {StateVector::Field(&mut self.1, field!(self.0))}
    fn from_raw(raw: RawAtomic) -> Self where Self: Sized {
        if let RawAtomic::Field(state, data) = raw {
            Static(serde_json::from_str(&data).unwrap(), serde_json::from_str(&state).unwrap())
        } else {panic!("Raw Atomic Not Field")}
    }

    fn atomic_type() -> AtomicType where Self: Sized {
        AtomicType::Field
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
struct DateState(u64, u64);
impl State for DateState {
    fn merge(&mut self, bytes: Option<&[u8]>) -> Ordering {
        match bytes {
            Some(bytes) => {
                let remote = serde_json::from_slice::<Self>(bytes).unwrap();
                let mut order = self.0.cmp(&remote.0);
                if matches!(order, Ordering::Equal) {
                    order = self.1.cmp(&remote.1);
                }
                if order == Ordering::Less {
                    self.0 = remote.0;
                    self.1 = remote.1;
                }
                order
            },
            None => Ordering::Greater
        }
    }

    fn serialize(&self) -> String {serde_json::to_string(self).unwrap()}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Dated<T>(T, u64, DateState);
impl<T: Serialize> Dated<T> {
    fn _ref(&mut self) {
        if self.1 > 0 {
            let hash = gxhash::gxhash64(&serde_json::to_vec(&self.0).unwrap(), 0);
            if self.2.1 != hash {
                self.2.0 = self.1;
                self.2.1 = hash;
            }
            self.1 = 0;
        }
    }
    fn _mut(&mut self) {
        self._ref();
        self.1 = now().timestamp_nanos() as u64;
    }
}
impl<T: Serialize> From<T> for Dated<T> {
    fn from(t: T) -> Dated<T> {
        let hash = gxhash::gxhash64(&serde_json::to_vec(&t).unwrap(), 0);
        Dated(t, 0, DateState(now().timestamp_nanos() as u64, hash))
    }
}
impl<T: Serialize> Deref for Dated<T> {
    type Target = T; fn deref(&self) -> &T {&self.0}
}
impl<T: Serialize> DerefMut for Dated<T> {
    fn deref_mut(&mut self) -> &mut T {self._mut(); &mut self.0}
}
impl<T: Serialize + for<'a> Deserialize<'a> + Debug> Atomic for Dated<T> {
    //fn id(&self) -> Id {Id::hash(&std::any::type_name_of_val(self).to_string())}
    
    fn atomic_type() -> AtomicType where Self: Sized {
        AtomicType::Field
    }

    fn state(&mut self) -> StateVector {self._ref(); StateVector::Field(&mut self.2, field!(self.0))}

    fn from_raw(raw: RawAtomic) -> Self where Self: Sized {
        if let RawAtomic::Field(state, data) = raw {
            Dated(serde_json::from_str(&data).unwrap(), 0, serde_json::from_str(&state).unwrap())
        } else {panic!("Raw Atomic Not Field")}
    }
}

impl<K: Serialize + for<'a> Deserialize<'a> + Debug + Ord, V: Atomic + Serialize + for<'a> Deserialize<'a>> AtomicMap for BTreeMap<K, V> {
    fn state_vector(&mut self) -> BTreeMap<String, StateVector<'_>> {
        self.iter_mut().map(|(k, v)| (hex::encode(serde_json::to_vec(&k).unwrap()), v.state())).collect()
    }

    fn sub_type(&self) -> AtomicType {V::atomic_type()}


    fn insert(&mut self, key: String, raw: RawAtomic) {
        self.insert(serde_json::from_slice(&hex::decode(key).unwrap()).unwrap(), V::from_raw(raw));
    }
}

impl<K: Serialize + for<'a> Deserialize<'a> + Debug + Ord, V: Atomic + Serialize + for<'a> Deserialize<'a>> Atomic for BTreeMap<K, V> {
    fn state(&mut self) -> StateVector {
        StateVector::Map(self)
    }

    fn from_raw(raw: RawAtomic) -> Self where Self: Sized {
        if let RawAtomic::Map(raw) = raw {
            raw.into_iter().map(|(k, v)| (serde_json::from_slice(&hex::decode(k).unwrap()).unwrap(), V::from_raw(v))).collect()
        } else {panic!("Raw Atomic Not Field")}
    }

    fn atomic_type() -> AtomicType where Self: Sized {
        AtomicType::Map(Box::new(V::atomic_type()))
    }
}

//#[derive(Atomic)]
#[derive(Debug, Clone)]
struct Room {
    id: Static<Id>,
    name: Dated<String>,
    messages: BTreeMap<Id, Message>
}

impl Atomic for Room {
    //fn id(&self) -> Id {*self.id}//Message does not need id because its contaained by either the
    //sync call or the messages map in room
    fn state(&mut self) -> StateVector {
        StateVector::Struct(BTreeMap::from([
            ("id".to_string(), self.id.state()),
            ("name".to_string(), self.name.state()),
            ("messages".to_string(), self.messages.state()),
        ]))
    }

    fn from_raw(raw: RawAtomic) -> Self where Self: Sized {
        if let RawAtomic::Struct(mut raw) = raw {
            Room{
                id: Static::<Id>::from_raw(raw.remove("id").unwrap()),
                name: Dated::<String>::from_raw(raw.remove("name").unwrap()),
                messages: BTreeMap::<Id, Message>::from_raw(raw.remove("messages").unwrap()),
            }
        } else {panic!("Raw Atomic Not Field")}
    }

    fn atomic_type() -> AtomicType where Self: Sized {
        AtomicType::Struct(BTreeMap::from([
            ("id".to_string(), Static::<Id>::atomic_type()),
            ("name".to_string(), Dated::<String>::atomic_type()),
            ("messages".to_string(), BTreeMap::<Id, Message>::atomic_type()),
        ]))
    }
}

//#[derive(Atomic)]
#[derive(Serialize, Deserialize, Debug, Clone)]
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
    fn state(&mut self) -> StateVector {
        StateVector::Struct(BTreeMap::from([
            ("id".to_string(), self.id.state()),
            ("author".to_string(), self.author.state()),
            ("body".to_string(), self.body.state()),
        ]))
    }

    fn from_raw(raw: RawAtomic) -> Self where Self: Sized {
        if let RawAtomic::Struct(mut raw) = raw {
            Message{
                id: Static::<Id>::from_raw(raw.remove("id").unwrap()),
                author: Static::<String>::from_raw(raw.remove("author").unwrap()),
                body: Dated::<String>::from_raw(raw.remove("body").unwrap()),
            }
        } else {panic!("Raw Atomic Not Field")}
    }

    fn atomic_type() -> AtomicType where Self: Sized {
        AtomicType::Struct(BTreeMap::from([
            ("id".to_string(), Static::<Id>::atomic_type()),
            ("author".to_string(), Static::<String>::atomic_type()),
            ("body".to_string(), Dated::<String>::atomic_type()),
        ]))
    }
}


mod test {
    use super::*;

    #[tokio::test]
    async fn test() {
        let mut cache = Cache::new();
        let mut map: BTreeMap<String, BTreeMap<String, Dated<u32>>> = BTreeMap::default();
        map.insert("Hello".to_string(), BTreeMap::from([("a".to_string(), 29.into())]));
        cache.sync_remote("mymessage".to_string(), &mut map).await.unwrap();
        println!("map: {:?}", map);

        map.insert("Goodbye".to_string(), BTreeMap::from([("b".to_string(), 73.into())]));
        cache.sync_remote("mymessage".to_string(), &mut map).await.unwrap();
        println!("map: {:?}", map);

        map.remove("Goodbye");
        map.insert("Goodbye".to_string(), BTreeMap::from([("a".to_string(), 20.into())]));
        cache.sync_remote("mymessage".to_string(), &mut map).await.unwrap();

        println!("map: {:?}", map);

        panic!("done");
    }
}
