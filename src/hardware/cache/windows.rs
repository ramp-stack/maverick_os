use std::path::PathBuf;
use std::env;
use std::fs;

pub struct OsApplicationSupport;

impl OsApplicationSupport {
    pub fn get() -> Option<PathBuf> {
        Self::get_app_name("org.ramp.orange")
    }

    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        if let Ok(appdata) = env::var("APPDATA") {
            let path = PathBuf::from(appdata).join(app_name);
            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        if let Ok(userprofile) = env::var("USERPROFILE") {
            let path = PathBuf::from(userprofile)
                .join("AppData")
                .join("Roaming")
                .join(app_name);

            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        None
    }
}