use super::ImageOrientation;
use std::process::Command;
use std::path::Path;
use std::fs;
use std::thread;

#[derive(Clone)]
pub struct OsPhotoPicker;

impl OsPhotoPicker {
    pub fn open(callback: impl FnOnce(Vec<u8>, ImageOrientation) + Send + 'static) {
        thread::spawn(move || {
            let result = Self::try_zenity()
                .or_else(|| Self::try_kdialog());

            match result {
                Some(file_path) => {
                    if let Ok(image_data) = fs::read(&file_path) {
                        callback(image_data, ImageOrientation::Up);
                    } else {
                        callback(Vec::new(), ImageOrientation::Up);
                    }
                }
                None => callback(Vec::new(), ImageOrientation::Up),
            }
        });
    }

    fn try_zenity() -> Option<String> {
        let output = Command::new("zenity")
            .args(&[
                "--file-selection",
                "--file-filter=Image files | *.jpg *.jpeg *.png",
                "--file-filter=All files | *",
                "--title=Select an Image",
            ])
            .output().ok()?;

        if output.status.success() {
            let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !file_path.is_empty() && Path::new(&file_path).exists() {
                return Some(file_path);
            }
        }
        None
    }

    fn try_kdialog() -> Option<String> {
        let output = Command::new("kdialog")
            .args(&[
                "--getopenfilename",
                ".",
                "*.jpg *.jpeg *.png|Image files",
                "--title",
                "Select an Image",
            ])
            .output().ok()?;

        if output.status.success() {
            let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !file_path.is_empty() && Path::new(&file_path).exists() {
                return Some(file_path);
            }
        }
        None
    }
}