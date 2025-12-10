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
            let result = Command::new("powershell")
                .args(&[
                    "-Command",
                    r#"
                    Add-Type -AssemblyName System.Windows.Forms;
                    $dialog = New-Object System.Windows.Forms.OpenFileDialog;
                    $dialog.Filter = 'Image Files|*.jpg;*.jpeg;*.png;*.gif;*.bmp;*.tiff;*.webp|All Files|*.*';
                    $dialog.Title = 'Select an Image';
                    $dialog.Multiselect = $false;
                    if ($dialog.ShowDialog() -eq 'OK') {
                        Write-Output $dialog.FileName
                    }
                    "#,
                ])
                .output();

            match result {
                Ok(output) if output.status.success() => {
                    let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !file_path.is_empty() && Path::new(&file_path).exists() {
                        if let Ok(image_data) = fs::read(&file_path) {
                            let orientation = Self::get_image_orientation(&file_path);
                            let _ = sender.send((image_data, orientation));
                        } else {
                            let _ = sender.send((Vec::new(), ImageOrientation::Up));
                        }
                    } else {
                        let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    }
                }
                _ => {
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                }
            }
        });
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