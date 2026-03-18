use maverick_os::{Application, Context, Services, window::Event, start};

pub struct DemoApplication;
impl Services for DemoApplication {}
impl Application for DemoApplication {
    async fn new(_context: &mut Context) -> Self {DemoApplication}
    async fn on_event(&mut self, _context: &mut Context, event: Event) {
        log::info!("Event Occured: {event:?}")
    }
}

start!(DemoApplication);
