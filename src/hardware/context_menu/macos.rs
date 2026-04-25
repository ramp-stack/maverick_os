use objc2::rc::Retained;
use objc2::sel;
use objc2::MainThreadOnly;
use objc2_foundation::{NSAutoreleasePool, NSString};

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;

use super::ContextMenuAction;

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

    #[cfg(target_os = "macos")]
    fn selector(&self) -> objc2::runtime::Sel {
        match self {
            ContextMenuAction::Copy => sel!(copy:),
            ContextMenuAction::Paste => sel!(paste:),
            ContextMenuAction::Cut => sel!(cut:),
            ContextMenuAction::SelectAll => sel!(selectAll:),
            ContextMenuAction::Delete => sel!(delete:),
            ContextMenuAction::Custom(_) => sel!(customAction:),
        }
    }
}

#[derive(Clone)]
pub struct OsContextMenu;

impl OsContextMenu {
    pub fn new() -> Self {
        Self
    }

    pub fn show(&self, x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            #[cfg(target_os = "macos")]
            return self.show_macos(x, y, actions);

            #[cfg(target_os = "ios")]
            return self.show_ios(x, y, actions);

            #[allow(unreachable_code)]
            None
        }
    }

    #[cfg(target_os = "macos")]
    unsafe fn show_macos(&self, x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        use objc2_foundation::NSPoint;

        let mtm = MainThreadMarker::new()?;

        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);

        for (i, action) in actions.iter().enumerate() {
            let title = NSString::from_str(action.label());
            let key = NSString::from_str("");
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &title,
                Some(action.selector()),
                &key,
            );
            item.setTag(i as isize);
            item.setEnabled(true);
            menu.addItem(&item);
        }

        let app = NSApplication::sharedApplication(mtm);
        let current_event = app.currentEvent()?;
        let window = current_event.window(mtm)?;
        let content_view = window.contentView()?;

        let location = NSPoint::new(x as f64, y as f64);

        menu.popUpMenuPositioningItem_atLocation_inView(None, location, Some(&content_view));

        let highlighted = menu.highlightedItem()?;
        let tag = highlighted.tag();
        actions.get(tag as usize).cloned()
    }

    #[cfg(target_os = "ios")]
    unsafe fn show_ios(&self, _x: f32, _y: f32, _actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        None
    }
}

