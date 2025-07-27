use std::fmt::Debug;
use std::collections::{BTreeMap, HashSet};
use std::hash::Hash;
use serde::{Serialize, Deserialize};
use serde_json::{Value, Map};

use super::{RecordMut, Tableize, Error};

pub trait RecordMap: Debug {
    fn from_value(value: Map<String, Value>) -> Result<Self, Error> where Self: Sized;
    //fn get_mut(&mut self) -> Result<BTreeMap<String, RecordMut<'_>>, serde_json::Error>;
    //fn children(&mut self) -> BTreeMap<String, RecordChildren<'_>>;
    //fn sub_type(&self) -> AtomicType;
    //fn insert(&mut self, key: String, value: RawAtomic);
}

impl<K: Debug + Serialize + for<'a> Deserialize<'a> + Ord, V: Debug + Serialize + for<'a> Deserialize<'a> + Tableize> RecordMap for BTreeMap<K, V> {
    fn from_value(map: Map<String, Value>) -> Result<Self, Error> where Self: Sized {
        map.into_iter().map(|(k, v)| Ok((serde_json::from_str(&k)?, V::from_value(v)?))).collect()
    }
  //fn get_mut(&mut self) -> Result<BTreeMap<String, RecordMut<'_>>, serde_json::Error> {
  //    self.iter_mut().map(|(k, v)| Ok((serde_json::to_string(&k)?, v.get_mut()?))).collect()
  //} 
}

//  impl<V: Hash + Debug + Serialize + for<'a> Deserialize<'a> + Tableize> RecordMap for HashSet<V> {
//      fn get_mut(&mut self) -> Result<BTreeMap<String, RecordMut<'_>>, serde_json::Error> {
//          self.iter_mut().map(|(k, v)| Ok((hex::encode(gxhash::gxhash64(&serde_json::to_vec(&v)?, 0).to_le_bytes()), v.get_mut()?))).collect()
//      } 
//  }

//Create IdSet implementation
