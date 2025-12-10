use std::path::PathBuf;
use std::env;
use std::fs;

pub struct OsApplicationSupport;

impl OsApplicationSupport {
    pub fn get() -> Option<PathBuf> {
        Self::get_app_name("org.ramp.orange")
    }

    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
            let path = PathBuf::from(xdg_data_home).join(app_name);
            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(app_name);

            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        None
    }
}