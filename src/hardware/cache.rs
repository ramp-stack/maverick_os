use rusqlite::{params, Connection, Error, Result, OpenFlags, TransactionBehavior};
use rusqlite::OptionalExtension;
use image::RgbaImage;
use serde::{Serialize, de::DeserializeOwned};

use crate::hardware::{Data, Video};

use std::sync::{Arc, LazyLock, Mutex};
use std::path::PathBuf;


const TAG_TEXT: u8 = 0;
const TAG_IMAGE: u8 = 1;

#[derive(Clone)]
pub struct Cache;
impl Cache {
    pub fn new() -> Self {
        LazyLock::force(&DATABASE);
        Self
    }

    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let bytes: Option<Vec<u8>> = DATABASE.lock().unwrap().query_row(
            "SELECT value FROM Shared_v1 WHERE key = ?1", [key], |row| row.get(0),
        ).optional().unwrap();
        bytes.map(|bytes| postcard::from_bytes(&bytes).expect("Invalid cached shared state"))
    }

    pub fn save<T: Serialize>(&self, key: &str, value: &T) {
        let bytes = postcard::to_allocvec(value).expect("Could not serialize shared state");
        DATABASE.lock().unwrap().execute(
            "INSERT OR REPLACE INTO Shared_v1 (key, value) VALUES (?1, ?2)", params![key, bytes],
        ).unwrap();
    }

    /// Stores `data` under `key` (overwrites if the key already exists).
    pub fn set(&self, key: &str, data: &Data) {
        let bytes = Self::serialize(data);
        DATABASE.lock().unwrap().execute(
            "INSERT OR REPLACE INTO Cache (key, value) VALUES (?1, ?2)", params![key, bytes],
        ).unwrap();
    }

    /// Retrieves the value for `key`, or `None` if the key is missing.
    /// Returns an error only on SQLite failures or corrupted blob data.
    pub fn get(&self, key: &str) -> Option<Data> {
        let bytes: Option<Vec<u8>> = DATABASE.lock().unwrap().query_row(
            "SELECT value FROM Cache WHERE key = ?1", [key], |row| row.get(0),
        ).optional().unwrap();
        bytes.map(|bytes| Self::deserialize(bytes).unwrap())
    }


    fn serialize(data: &Data) -> Vec<u8> {
        match data {
            Data::Text(s) => {
                let mut v = Vec::with_capacity(1 + s.len());
                v.push(TAG_TEXT);
                v.extend_from_slice(s.as_bytes());
                v
            }
            Data::File(path) => {
                let path = path.to_string_lossy();
                let mut v = Vec::with_capacity(1 + path.len());
                v.push(2);
                v.extend_from_slice(path.as_bytes());
                v
            }
            Data::Image(img) => {
                let raw = img.as_raw(); // &[u8] / &Vec<u8>
                let mut v = Vec::with_capacity(1 + 8 + raw.len());
                v.push(TAG_IMAGE);
                v.extend_from_slice(&img.width().to_le_bytes());
                v.extend_from_slice(&img.height().to_le_bytes());
                v.extend_from_slice(raw);
                v
            }
            Data::Video(video) => {
                let movie = video.bytes();
                let mut v = Vec::with_capacity(1 + movie.len());
                v.push(4);
                v.extend_from_slice(movie);
                v
            }
        }
    }

    fn deserialize(mut blob: Vec<u8>) -> Result<Data> {
        if blob.is_empty() {
            return Err(Error::InvalidColumnType(
                0,
                "value".into(),
                rusqlite::types::Type::Blob,
            ));
        }

        let tag = blob.remove(0);
        match tag {
            TAG_TEXT => {
                let s = String::from_utf8(blob).map_err(|_| {
                    Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)
                })?;
                Ok(Data::Text(s))
            }
            2 => {
                let path = String::from_utf8(blob).map_err(|_| {
                    Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)
                })?;
                Ok(Data::File(PathBuf::from(path)))
            }
            TAG_IMAGE => {
                if blob.len() < 8 {
                    return Err(Error::InvalidColumnType(
                        0,
                        "value".into(),
                        rusqlite::types::Type::Blob,
                    ));
                }
                let width = u32::from_le_bytes(blob[0..4].try_into().unwrap());
                let height = u32::from_le_bytes(blob[4..8].try_into().unwrap());
                let pixel_data = blob.split_off(8);

                let expected_len = (width as usize)
                    .checked_mul(height as usize)
                    .and_then(|n| n.checked_mul(4))
                    .ok_or_else(|| {
                        Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)
                    })?;

                if pixel_data.len() != expected_len {
                    return Err(Error::InvalidColumnType(
                        0,
                        "value".into(),
                        rusqlite::types::Type::Blob,
                    ));
                }

                let img = RgbaImage::from_raw(width, height, pixel_data).ok_or_else(|| {
                    Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)
                })?;

                Ok(Data::Image(Arc::new(img)))
            }
            3 | 4 => {
                // Older records carry a length-prefixed platform type before the movie bytes.
                if tag == 3 {
                    if blob.is_empty() {
                        return Err(Error::InvalidColumnType(
                            0, "value".into(), rusqlite::types::Type::Blob,
                        ));
                    }
                    let len = blob[0] as usize;
                    if blob.len() < 1 + len {
                        return Err(Error::InvalidColumnType(
                            0, "value".into(), rusqlite::types::Type::Blob,
                        ));
                    }
                    blob = blob.split_off(1 + len);
                }
                Video::new(blob).map(Data::Video).ok_or_else(|| {
                    Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)
                })
            }
            _ => Err(Error::InvalidColumnType(
                0,
                "value".into(),
                rusqlite::types::Type::Blob,
            )),
        }
    }
}

static DATABASE: LazyLock<Mutex<Connection>> = LazyLock::new(|| {
    let path = PathBuf::from("./CACHE");
    let mut connection = if cfg!(test) {
        Connection::open_with_flags(
            format!("file:mem_{}?mode=memory", path.to_string_lossy()),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
        ).unwrap()
    } else {Connection::open(path).unwrap()};
    connection.pragma_update(None, "locking_mode", "EXCLUSIVE").unwrap();
    connection.pragma_update(None, "journal_mode", "WAL").unwrap();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive).unwrap();
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS Cache(key TEXT NOT NULL PRIMARY KEY, value BLOB NOT NULL);
         CREATE TABLE IF NOT EXISTS Shared_v1(key TEXT NOT NULL PRIMARY KEY, value BLOB NOT NULL);",
    ).unwrap();
    transaction.commit().unwrap();
    Mutex::new(connection)
});
