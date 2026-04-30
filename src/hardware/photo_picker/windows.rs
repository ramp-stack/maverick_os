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
                            callback(image_data, ImageOrientation::Up);
                        } else {
                            callback(Vec::new(), ImageOrientation::Up);
                        }
                    } else {
                        callback(Vec::new(), ImageOrientation::Up);
                    }
                }
                _ => callback(Vec::new(), ImageOrientation::Up),
            }
        });
    }
}