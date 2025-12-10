/// `true` if the target platform is iOS or Android, otherwise `false`.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub const IS_MOBILE: bool = true;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub const IS_MOBILE: bool = false;

/// `true` if the target architecture is WebAssembly (`wasm32`), otherwise `false`.
#[cfg(target_arch = "wasm32")]
pub const IS_WEB: bool = true;
#[cfg(not(target_arch = "wasm32"))]
pub const IS_WEB: bool = false;
