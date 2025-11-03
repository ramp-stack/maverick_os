use std::sync::mpsc::Sender;
use super::ImageOrientation;
use std::process::Command;
use std::path::Path;
use std::fs;
use std::thread;

#[derive(Clone)]
pub struct OsPhotoPicker;

impl OsPhotoPicker {
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        thread::spawn(move || {
            let result = Self::try_zenity()
                .or_else(|| Self::try_kdialog())
                .or_else(|| Self::try_xdg_open());

            match result {
                Some(file_path) => {
                    if let Ok(image_data) = fs::read(&file_path) {
                        let orientation = Self::get_image_orientation(&file_path);
                        let _ = sender.send((image_data, orientation));
                    } else {
                        let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    }
                }
                None => {
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                }
            }
        });
    }

    fn try_zenity() -> Option<String> {
        let result = Command::new("zenity")
            .args(&[
                "--file-selection",
                "--file-filter=Image files | *.jpg *.jpeg *.png",
                "--file-filter=All files | *",
                "--title=Select an Image",
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !file_path.is_empty() && Path::new(&file_path).exists() {
                    Some(file_path)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn try_kdialog() -> Option<String> {
        let result = Command::new("kdialog")
            .args(&[
                "--getopenfilename",
                ".",
                "*.jpg *.jpeg *.png *.webp|Image files",
                "--title",
                "Select an Image",
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !file_path.is_empty() && Path::new(&file_path).exists() {
                    Some(file_path)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn try_xdg_open() -> Option<String> {
        None
    }

    fn get_image_orientation(file_path: &str) -> ImageOrientation {
        let path = Path::new(file_path);
        if let Some(extension) = path.extension() {
            match extension.to_str().unwrap_or("").to_lowercase().as_str() {
                "jpg" | "jpeg" => {
                    ImageOrientation::Up
                }
                _ => ImageOrientation::Up,
            }
        } else {
            ImageOrientation::Up
        }
    }
}