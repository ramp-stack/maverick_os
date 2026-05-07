use image::RgbaImage;

#[derive(Clone)]
pub struct OsShare;

impl OsShare {
    pub fn new() -> Self {
        Self
    }

    pub fn share(&self, text: &str) {

        use std::process::Command;

        let wayland = Command::new("wl-copy")
            .arg(text)
            .status();

        if wayland.is_err() || !wayland.unwrap().success() {
            let xclip = Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn();

            if let Ok(mut child) = xclip {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
            } else {
                eprintln!("Failed to share on Linux: neither wl-copy nor xclip available");
            }
        }
    }

    pub fn share_image(&self, rgba_image: RgbaImage) {
        use std::process::Command;
        use std::io::Write;

        let mut png_bytes: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        if let Err(e) = rgba_image.write_to(&mut cursor, image::ImageFormat::Png) {
            eprintln!("Failed to encode image: {}", e);
            return;
        }

        let wayland = Command::new("wl-copy")
            .args(["--type", "image/png"])
            .stdin(std::process::Stdio::piped())
            .spawn();

        if let Ok(mut child) = wayland {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(&png_bytes);
            }
            let _ = child.wait();
        } else {
            let xclip = Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "image/png"])
                .stdin(std::process::Stdio::piped())
                .spawn();

            if let Ok(mut child) = xclip {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(&png_bytes);
                }
                let _ = child.wait();
            } else {
                eprintln!("Failed to share image on Linux: neither wl-copy nor xclip available");
            }
        }
    }
}