use rusqlite::{params, Connection, Error, Result, TransactionBehavior};
use rusqlite::OptionalExtension;
use serde::Serialize;

use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::io::Cursor;

use crate::hardware::{Data, Format, Image, Media};

#[derive(Clone, Debug)]
pub struct Cache(Arc<Mutex<Connection>>);
impl Cache {
    pub fn new(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS Cache(key TEXT NOT NULL PRIMARY KEY, value BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS Shared_v1(key TEXT NOT NULL PRIMARY KEY, value BLOB NOT NULL);",
        )?;
        transaction.commit()?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }
    pub fn load_bytes(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.0.lock().unwrap().query_row(
            "SELECT value FROM Shared_v1 WHERE key = ?1", [key], |row| row.get(0),
        ).optional()
    }
    pub fn save<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let bytes = postcard::to_allocvec(value).unwrap();
        self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO Shared_v1 (key, value) VALUES (?1, ?2)", params![key, bytes],
        ).map(|_| ())
    }
    pub fn set(&self, key: &str, data: &Data) -> Result<(), String> {
        let bytes = Self::serialize(data)?;
        self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO Cache (key, value) VALUES (?1, ?2)", params![key, bytes],
        ).map(|_| ()).map_err(|error| error.to_string())
    }
    pub fn get(&self, key: &str) -> Option<Data> {
        let bytes: Result<Vec<u8>> = self.0.lock().unwrap().query_row(
            "SELECT value FROM Cache WHERE key = ?1", [key], |row| row.get(0),
        );
        match bytes.optional().and_then(|bytes| bytes.map(Self::deserialize).transpose()) {
            Ok(data) => data,
            Err(error) => {log::warn!("Could not read cached value {key:?}: {error}"); None}
        }
    }

    fn serialize(data: &Data) -> Result<Vec<u8>, String> {
        Ok(match data {
            Data::Text(s) => {
                let mut v = Vec::with_capacity(1 + s.len());
                v.push(0);
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
                let (width, height) = img.dimensions;
                let raw = img.as_ref();
                let mut v = Vec::with_capacity(1 + 8 + raw.len());
                v.push(1);
                v.extend_from_slice(&width.to_le_bytes());
                v.extend_from_slice(&height.to_le_bytes());
                v.extend_from_slice(raw);
                v
            }
            Data::Media(media) => {
                let mut bytes = vec![4];
                for chunk in media.encode(Format::Mkv_Ffv1_PcmF32le)? {bytes.extend(chunk?);}
                bytes
            }
        })
    }

    fn deserialize(mut blob: Vec<u8>) -> Result<Data> {
        if blob.is_empty() {
            return Err(Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob));
        }

        let tag = blob.remove(0);
        match tag {
            0 => {
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
            1 => {
                if blob.len() < 8 {
                    return Err(Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob));
                }
                let width = u32::from_le_bytes(blob[0..4].try_into().unwrap());
                let height = u32::from_le_bytes(blob[4..8].try_into().unwrap());
                if width == 0 || height == 0 {
                    return Err(Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob));
                }
                let pixel_data = blob.split_off(8);

                let expected_len = (width as usize)
                    .checked_mul(height as usize)
                    .and_then(|n| n.checked_mul(4))
                    .ok_or_else(|| {
                        Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)
                    })?;

                if pixel_data.len() != expected_len {
                    return Err(Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob));
                }

                Ok(Data::Image(Image::new(
                    (width, height), bytemuck::cast_slice(&pixel_data).to_vec(),
                )))
            }
            3 | 4 | 5 => {
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
                Media::read(Cursor::new(blob)).map(Data::Media).map_err(|error| {
                    Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, error.into())
                })
            }
            _ => Err(Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Blob)),
        }
    }
}
