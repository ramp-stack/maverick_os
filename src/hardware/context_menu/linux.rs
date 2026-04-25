use std::sync::mpsc;
use std::sync::Mutex;


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    Cut,
    SelectAll,
    Delete,
    Custom(String),
}

impl ContextMenuAction {
    fn label(&self) -> &str {
        match self {
            ContextMenuAction::Copy => "Copy",
            ContextMenuAction::Paste => "Paste",
            ContextMenuAction::Cut => "Cut",
            ContextMenuAction::SelectAll => "Select All",
            ContextMenuAction::Delete => "Delete",
            ContextMenuAction::Custom(label) => label.as_str(),
        }
    }

    fn from_index(index: usize, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        actions.get(index).cloned()
    }
}

pub type ContextMenuCallback = Box<dyn FnOnce(Option<ContextMenuAction>) + Send + 'static>;

pub struct ContextMenu;

impl ContextMenu {
    pub fn show(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        let mut pending = PENDING_MENU.lock().ok()?;
        let (tx, rx) = mpsc::channel();

        *pending = Some(PendingMenu {
            x,
            y,
            actions: actions.to_vec(),
            response: tx,
        });

        drop(pending);

        rx.recv().ok().flatten()
    }

    pub fn show_async(
        x: f32,
        y: f32,
        actions: &[ContextMenuAction],
    ) -> mpsc::Receiver<Option<ContextMenuAction>> {
        let (tx, rx) = mpsc::channel();

        if let Ok(mut pending) = PENDING_MENU.lock() {
            *pending = Some(PendingMenu {
                x,
                y,
                actions: actions.to_vec(),
                response: tx,
            });
        }

        rx
    }

    pub fn take_pending() -> Option<PendingMenu> {
        PENDING_MENU.lock().ok()?.take()
    }
}

pub struct PendingMenu {
    pub x: f32,
    pub y: f32,
    pub actions: Vec<ContextMenuAction>,
    pub response: mpsc::Sender<Option<ContextMenuAction>>,
}

impl PendingMenu {
    pub fn respond(self, selected: Option<ContextMenuAction>) {
        let _ = self.response.send(selected);
    }
}



static PENDING_MENU: Mutex<Option<PendingMenu>> = Mutex::new(None);