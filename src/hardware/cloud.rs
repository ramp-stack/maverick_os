#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use apple::OsCloudStorage;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
use android::OsCloudStorage;

#[derive(Clone)]
pub struct CloudStorage(
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    OsCloudStorage
);

impl CloudStorage {
    pub(crate) fn new(
        #[cfg(target_os = "android")]
        vm: &jni::JavaVM
    ) -> Self {
        #[cfg(target_os = "android")]
        OsCloudStorage::init_java_vm(vm);
        
        Self(
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
            OsCloudStorage::new()
        )
    }

    pub fn save(&self, key: &str, value: &str) {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        self.0.save(key, value);

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        panic!("Not supported on this OS");
    }

    pub fn get(&self, key: &str) -> Option<String> {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        {self.0.get(key)}

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        panic!("Not supported on this OS");
    }

    pub fn remove(&self, key: &str) {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        self.0.remove(key);

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        panic!("Not supported on this OS");
    }

    pub fn clear(&self) {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        self.0.clear();

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        panic!("not supported on this OS");
    }
}

impl Default for CloudStorage {
    fn default() -> Self {
        Self::new(
            #[cfg(target_os = "android")]
            panic!("CloudStorage::default() cannot be used on Android. Use CloudStorage::new(vm) instead.")
        )
    }
}