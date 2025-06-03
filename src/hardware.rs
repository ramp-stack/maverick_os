mod cache;
pub use cache::Cache;           

///Hardware context contains interfaces to various hardware all interfaces are clonable and should
///not panic when invoked from different places or times
#[derive(Clone)]
pub struct Context {
    pub cache: Cache,
}

impl Context {
    pub(crate) fn new() -> Self {
        Context{
            cache: Cache::new()
        }
    }
}
