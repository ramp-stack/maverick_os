use std::sync::mpsc;

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

/// Callback type for when a menu item is selected.
pub type ContextMenuCallback = Box<dyn FnOnce(Option<ContextMenuAction>) + Send + 'static>;

pub struct ContextMenu;

impl ContextMenu {
    /// Show a context menu. On Linux, this dispatches to whichever
    /// toolkit the app is using. The default implementation uses
    /// a channel-based approach that your UI layer hooks into.
    pub fn show(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        // For a pure-Rust app without GTK, we expose the menu data
        // and let the rendering layer (egui, iced, etc.) handle display.
        // This stores the pending menu so the UI can poll it.
        let mut pending = PENDING_MENU.lock().ok()?;
        let (tx, rx) = mpsc::channel();

        *pending = Some(PendingMenu {
            x,
            y,
            actions: actions.to_vec(),
            response: tx,
        });

        drop(pending);

        // Block until the UI layer sends back a selection.
        // In an async app, you'd use an async channel instead.
        rx.recv().ok().flatten()
    }

    /// Non-blocking variant: returns a receiver that your UI event
    /// loop can poll for the user's selection.
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

    /// Called by the UI layer to retrieve the pending context menu request.
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
    /// The UI layer calls this once the user picks an item (or dismisses).
    pub fn respond(self, selected: Option<ContextMenuAction>) {
        let _ = self.response.send(selected);
    }
}

use std::sync::Mutex;

static PENDING_MENU: Mutex<Option<PendingMenu>> = Mutex::new(None);