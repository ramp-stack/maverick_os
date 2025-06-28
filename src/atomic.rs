
#[derive(Debug)]
struct Error;
impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{:?}", self)}
}

pub trait Atomic {
    fn merge(&mut self, other: Self) -> Result<(), Error> where Self: Sized;

    fn state(&self) -> BTreeMap<String, u64>;
}

pub struct State(BTreeMap<String, u64>);

//Create wallet, Default values required for structure before it loads but should accept requests
//for things like get new address

//Create room, Empty by default but queues created messages and sends off asap

//Most objects synced into multiple ways Cloud/Cache, Cache/Air
//Objects made up of multiple "fields" Can be updated independantly


//Sending/Storing Changes VS UpdatedState

#[derive(Atomic)]
pub struct Room {
    #[atomic(latest)]
    pub name: String,
    atomic_name: u64,
    #[atomic(static)]
    pub id: u32,
}

impl Room {
    pub fn atomic_new(name: String, id: u32) -> Self {
        Room{name, __name: Utc::now().timestamp(), id}
    }
}

impl Atomic for Room {
    fn merge(&mut self, other: Self) -> Result<(), Error> {
        if self.id != other.id {return Err(Error);}
        if self.__name < other.__name {
            self.name = other.name;
        }    
        Ok(())
    }
}

#[derive(Atomic)]
pub struct Cache {
    #[atomic(latest)]
    rooms: BTreeMap<RecordPath, Room>
    #[atomic(max)]
    latest: u32
}

pub struct State {
    Max(u64)
}



struct Profile {
    name: String,
    abtme: String,
    age: u32
}

struct StoredProfile {
    name: (String, DateTime),
    abtme: (String, DateTime),
    age: (String, DateTime),
}

struct ProfileState {
    name: DateTime,
    abtme: DateTime,
    age: DateTime,
}
