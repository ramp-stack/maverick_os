use std::sync::{Arc, Mutex, OnceLock};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{NSAutoreleasePool, NSString, NSArray};

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

    #[cfg(target_os = "macos")]
    fn selector(&self) -> &str {
        match self {
            ContextMenuAction::Copy => "copy:",
            ContextMenuAction::Paste => "paste:",
            ContextMenuAction::Cut => "cut:",
            ContextMenuAction::SelectAll => "selectAll:",
            ContextMenuAction::Delete => "delete:",
            ContextMenuAction::Custom(_) => "customAction:",
        }
    }

    fn from_index(index: usize, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        actions.get(index).cloned()
    }
}

static SELECTED_INDEX: OnceLock<Arc<Mutex<Option<usize>>>> = OnceLock::new();

fn selected_index() -> &'static Arc<Mutex<Option<usize>>> {
    SELECTED_INDEX.get_or_init(|| Arc::new(Mutex::new(None)))
}

pub struct ContextMenu;

impl ContextMenu {
    /// Show a native context menu at screen position (x, y).
    /// On macOS this uses NSMenu, on iOS this uses UIMenuController.
    pub fn show(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            #[cfg(target_os = "macos")]
            return Self::show_macos(x, y, actions);

            #[cfg(target_os = "ios")]
            return Self::show_ios(x, y, actions);
        }
    }

    #[cfg(target_os = "macos")]
    unsafe fn show_macos(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        // NSMenu
        let menu_class = objc2::runtime::AnyClass::get("NSMenu")?;
        let menu: Retained<objc2::runtime::AnyObject> = msg_send![menu_class, new];
        let _: () = msg_send![&*menu, setAutoenablesItems: false];

        let menu_item_class = objc2::runtime::AnyClass::get("NSMenuItem")?;

        for (i, action) in actions.iter().enumerate() {
            let title = NSString::from_str(action.label());
            let sel_name = NSString::from_str(action.selector());

            let selector = objc2::sel!(contextMenuItemClicked:);

            let item: Retained<objc2::runtime::AnyObject> = msg_send![
                menu_item_class,
                alloc
            ];
            let item: Retained<objc2::runtime::AnyObject> = msg_send![
                item,
                initWithTitle: &*title,
                action: selector,
                keyEquivalent: &*NSString::from_str("")
            ];
            let _: () = msg_send![&*item, setTag: i as isize];
            let _: () = msg_send![&*item, setEnabled: true];
            let _: () = msg_send![&*menu, addItem: &*item];
        }

        // Create an NSEvent-based location and pop up the menu
        let ns_app_class = objc2::runtime::AnyClass::get("NSApplication")?;
        let app: Retained<objc2::runtime::AnyObject> = msg_send![ns_app_class, sharedApplication];
        let current_event: *mut objc2::runtime::AnyObject = msg_send![&*app, currentEvent];

        if current_event.is_null() {
            return None;
        }

        let window: *mut objc2::runtime::AnyObject = msg_send![current_event, window];
        if window.is_null() {
            return None;
        }

        let content_view: *mut objc2::runtime::AnyObject = msg_send![window, contentView];
        if content_view.is_null() {
            return None;
        }

        // NSPoint for the location
        #[repr(C)]
        struct NSPoint {
            x: f64,
            y: f64,
        }
        let location = NSPoint {
            x: x as f64,
            y: y as f64,
        };

        let _: Bool = msg_send![
            &*menu,
            popUpMenuPositioningItem: std::ptr::null::<objc2::runtime::AnyObject>(),
            atLocation: location,
            inView: content_view
        ];

        // After the menu closes, check which item was highlighted/selected
        let highlighted: *mut objc2::runtime::AnyObject = msg_send![&*menu, highlightedItem];
        if highlighted.is_null() {
            return None;
        }

        let tag: isize = msg_send![highlighted, tag];
        ContextMenuAction::from_index(tag as usize, actions)
    }

    #[cfg(target_os = "ios")]
    unsafe fn show_ios(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        // On iOS 16+, use UIEditMenuInteraction style approach
        // For broader compatibility, build UIMenuController items

        let controller_class = objc2::runtime::AnyClass::get("UIMenuController")?;
        let controller: Retained<objc2::runtime::AnyObject> =
            msg_send![controller_class, sharedMenuController];

        let menu_item_class = objc2::runtime::AnyClass::get("UIMenuItem")?;

        let mut items: Vec<Retained<objc2::runtime::AnyObject>> = Vec::new();

        for (i, action) in actions.iter().enumerate() {
            let title = NSString::from_str(action.label());
            let sel_str = format!("contextMenuAction{}:", i);
            let sel = objc2::runtime::Sel::register(sel_str.as_str());

            let item: Retained<objc2::runtime::AnyObject> = msg_send![
                menu_item_class,
                alloc
            ];
            let item: Retained<objc2::runtime::AnyObject> = msg_send![
                item,
                initWithTitle: &*title,
                action: sel
            ];
            items.push(item);
        }

        // Convert to NSArray
        let ns_array_class = objc2::runtime::AnyClass::get("NSArray")?;
        let items_refs: Vec<&objc2::runtime::AnyObject> = items.iter().map(|i| &**i).collect();
        let array: Retained<objc2::runtime::AnyObject> = msg_send![
            ns_array_class,
            arrayWithObjects: items_refs.as_ptr(),
            count: items_refs.len()
        ];
        let _: () = msg_send![&*controller, setMenuItems: &*array];

        #[repr(C)]
        struct CGRect {
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        }

        let target_rect = CGRect {
            x: x as f64,
            y: y as f64,
            width: 1.0,
            height: 1.0,
        };

        // The menu will be shown relative to a UIView — the caller
        // is responsible for passing a valid view context in production.
        // This is a simplified version.
        let _: () = msg_send![&*controller, setMenuVisible: true, animated: true];

        // iOS context menus are async; in practice you'd handle selection
        // through the responder chain. Return None here — wire up
        // a callback/channel in your actual integration.
        None
    }
}