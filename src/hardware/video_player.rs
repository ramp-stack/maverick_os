#[cfg(target_os = "ios")]
mod ios;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

/// Plays videos full screen: on iOS in the system player, elsewhere in
/// whatever the computer opens videos with.
#[derive(Clone, Default)]
pub struct VideoPlayer;

impl VideoPlayer {
    pub fn new() -> Self { VideoPlayer }

    /// Plays `mp4`. It's written to a file in the app's temporary folder
    /// first (the same file each time it's played).
    pub fn play(&self, mp4: &[u8]) {
        let Some(path) = Self::write(mp4) else { return };

        #[cfg(target_os = "ios")]
        ios::play(path);

        #[cfg(target_os = "macos")]
        { let _ = std::process::Command::new("open").arg(&path).spawn(); }

        #[cfg(target_os = "linux")]
        { let _ = std::process::Command::new("xdg-open").arg(&path).spawn(); }

        #[cfg(target_os = "windows")]
        { let _ = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(&path).spawn(); }

        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows")))]
        let _ = path;
    }

    fn write(mp4: &[u8]) -> Option<PathBuf> {
        let mut hasher = DefaultHasher::new();
        mp4.hash(&mut hasher);
        let path = std::env::temp_dir().join(format!("video-{:016x}.mp4", hasher.finish()));
        if !path.exists() { std::fs::write(&path, mp4).ok()?; }
        Some(path)
    }
}
