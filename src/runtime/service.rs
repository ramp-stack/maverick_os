

//  pub trait Services {
//      fn services() -> ServiceList {BTreeMap::new()}
//  }

//  pub type ServiceList = BTreeMap<TypeId, Box<dyn for<'a> FnOnce(&'a mut hardware::Context) -> Pin<Box<dyn Future<Output = Box<dyn Service>> + 'a>>>>;



//Lives on the active thread, Services can talk to each other through the runtime ctx which lives
//on the active thread.
#[async_trait]
pub trait Service: Send {
    async fn run(&mut self, ctx: &mut ServiceContext, channel: &mut Channel) -> Result<Duration, Error>;
    fn callback(state: &mut State, payload: String); //-> Box<Callback> {Box::new(|_state: &mut State, _response: String| {})}

    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;

    fn background_tasks(&self) -> Vec<Box<dyn BackgroundTask>> {vec![]}
    fn services(&self) -> ServiceList {BTreeMap::new()}
}

#[async_trait]
impl Thread for Service {

}
