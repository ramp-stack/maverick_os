use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, NamedKey};
use objc2_media_player::{MPNowPlayingInfoCenter, MPNowPlayingPlaybackState};
use objc2_core_audio::{
    AudioObjectPropertyAddress, AudioObjectGetPropertyData, AudioObjectAddPropertyListenerBlock,
    AudioObjectRemovePropertyListenerBlock, kAudioObjectSystemObject,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput,
    kAudioObjectPropertyElementWildcard, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioDevicePropertyVolumeScalar, kAudioDevicePropertyPreferredChannelsForStereo,
};

use crossfire::{spsc, AsyncRx, AsyncTx};
use dispatch2::{MainThreadBound, DispatchQueue};

use block2::RcBlock;
use objc2::{
    define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
    AnyThread, Message,
};
use objc2::runtime::{ProtocolObject, AnyObject, Imp, Sel};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSImage, NSSharingServicePicker, NSWindow, NSViewController,
    NSBackingStoreType, NSAutoresizingMaskOptions,
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSPasteboard,
    NSPasteboardTypeString, NSPasteboardTypePNG, NSPasteboardTypeTIFF,
    NSPasteboardTypeFileURL, NSWindowDelegate, NSOpenPanel, NSModalResponse,
    NSModalResponseOK, NSModalResponseCancel, NSAlert,
    NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSAlertThirdButtonReturn,
    NSWindowDidBecomeKeyNotification, NSApplicationDidBecomeActiveNotification,
};
use objc2_core_foundation::CGSize;
use objc2_foundation::{
    NSBundle, NSError,
    NSArray, NSData, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol,
    NSString, NSRectEdge, NSURL,
};
use objc2_photos_ui::{
    PHPickerResult, PHPickerViewController,
    PHPickerViewControllerDelegate,
};

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::ptr::NonNull;

use crate::inputs::{Input, Media as MediaInput};
use crate::events::Output;
use crate::hardware::{
    Data, Hardware, Image, Media, Pickable, Picked, PickResult, PushToken,
};
use super::apple::{AudioType, MovieType, PickRequest, PickState, MediaCommands};

thread_local! {static PUSH: Option<_Push> = _Push::new();}

#[derive(Clone)]
pub struct Keyboard;
impl Keyboard {
    pub(crate) fn new(_proxy: EventLoopProxy<Output>, _inset: Arc<KeyboardInset>) -> Self {Self}
    pub fn show(&self, _padding: f32) {}
    pub fn hide(&self) {}
    pub fn visible(&self) -> bool {false}
}

pub struct SafeArea;
impl SafeArea {
    pub fn is_mobile() -> bool {false}
    pub fn physical(window: &winit::window::Window) -> crate::PhysicalWindow {
        let size = window.inner_size();
        crate::PhysicalWindow {
            width: size.width, height: size.height, scale_factor: window.scale_factor(),
        }
    }
    pub fn get(_window: &winit::window::Window) -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }
}

pub struct LaunchScreen;
impl LaunchScreen {
    pub fn new(_window: &crate::Window) -> Self {Self}
    pub fn dismiss(&mut self) {}
}

pub struct KeyboardInset;
impl KeyboardInset {
    pub fn new(_window: &winit::window::Window, _proxy: EventLoopProxy<Output>) -> Self {Self}
    pub fn get(&self) -> (f32, bool) {(0.0, false)}
}

pub struct Events {
    _volume: Rc<RefCell<DeviceVolume>>,
    _commands: MediaCommands,
}
impl Events {
    pub fn new(_window: &crate::Window, proxy: EventLoopProxy<Output>) -> Self {
        Self {_volume: DeviceVolume::new(proxy.clone()), _commands: MediaCommands::new(proxy)}
    }
    pub fn process(&mut self, event: WindowEvent, _scale: f64) -> Option<WindowEvent> {
        match &event {
            WindowEvent::KeyboardInput {event, ..} if matches!(
                event.logical_key, Key::Named(NamedKey::AudioVolumeUp | NamedKey::AudioVolumeDown)
            ) => None,
            _ => Some(event),
        }
    }
    pub fn suspend(&mut self) {}
    pub async fn next(&mut self) -> Input {std::future::pending().await}
}

pub struct AudioSession;
impl AudioSession {
    pub fn recording() -> Result<Self, String> {Ok(Self)}
    pub fn configure_capture(_session: &objc2_av_foundation::AVCaptureSession) {}

    pub fn playback_state(playing: Option<bool>) {
        let state = match playing {
            Some(true) => MPNowPlayingPlaybackState::Playing,
            Some(false) => MPNowPlayingPlaybackState::Paused,
            None => MPNowPlayingPlaybackState::Stopped,
        };
        unsafe {MPNowPlayingInfoCenter::defaultCenter().setPlaybackState(state);}
    }
    pub fn new() -> Result<Self, String> {Ok(Self)}
}

pub struct Clipboard;
impl Clipboard {
    pub fn new() -> Self {Self}

    pub fn set(&self, data: &Data) {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        // NSPasteboardType is an alias of NSString. NSString::string() is empty,
        // so the pasteboard type has to be the UTI constant itself.
        let ok = match data {
            Data::Text(text) => {
                let s = NSString::from_str(text);
                pb.setString_forType(&s, unsafe { NSPasteboardTypeString })
            }
            Data::Image(img) => {
                let mut bytes = Vec::new();
                img.write(&mut bytes, super::ImageFormat::Png).unwrap();
                let bytes = NSData::with_bytes(&bytes);
                pb.setData_forType(Some(&bytes), unsafe { NSPasteboardTypePNG })
            }
            Data::File(path) => {
                let path = NSString::from_str(&path.to_string_lossy());
                let url = NSURL::fileURLWithPath(&path);
                match url.absoluteString() {
                    Some(text) => pb.setString_forType(
                        &text, unsafe { NSPasteboardTypeFileURL },
                    ),
                    None => false,
                }
            }
            Data::Media(media) => {
                let (bytes, ty) = match super::apple::ShareMedia::export(media) {
                    Ok(export) => export,
                    Err(error) => {log::error!("Clipboard export failed: {error}"); return}
                };
                let bytes = NSData::with_bytes(&bytes);
                pb.setData_forType(Some(&bytes), &ty.identifier())
            }
        };
        if !ok {log::error!("clipboard write failed");}
    }

    pub fn get(&self) -> Option<Data> {
        let pb = NSPasteboard::generalPasteboard();
        if let Some(text) = pb.stringForType(unsafe { NSPasteboardTypeFileURL })
            && let Some(url) = NSURL::URLWithString(&text)
            && url.isFileURL()
            && let Some(path) = url.path()
        {
            return Some(Data::File(PathBuf::from(path.to_string())));
        }
        if let Some(s) = pb.stringForType(unsafe { NSPasteboardTypeString }) {
            return Some(Data::Text(s.to_string()));
        }
        if let Some(types) = pb.types() {
            for ty in types.iter() {
                if (AudioType::matches(&ty) || MovieType::matches(&ty))
                    && let Some(data) = pb.dataForType(&ty)
                    && let Ok(media) = Media::read(Cursor::new(data.to_vec()))
                {
                    return Some(Data::Media(media));
                }
            }
        }
        for ty in [unsafe { NSPasteboardTypePNG }, unsafe { NSPasteboardTypeTIFF }] {
            if let Some(data) = pb.dataForType(ty)
                && let Ok(image) = Image::read(data.to_vec().as_slice()) {
                return Some(Data::Image(image));
            }
        }
        None
    }
}

pub struct Haptics(Retained<ProtocolObject<dyn NSHapticFeedbackPerformer>>);
impl Haptics {
    pub fn new() -> Self {Haptics(NSHapticFeedbackManager::defaultPerformer())}

    pub fn trigger(&self) { 
        self.0.performFeedbackPattern_performanceTime(
            NSHapticFeedbackPattern::Generic,
            NSHapticFeedbackPerformanceTime::Now,
        );
    }
}

pub struct Push {
    destination: Option<tokio::sync::watch::Receiver<Option<String>>>,
    previous: Option<String>,
}

impl Push {
    pub fn new() -> Self {Self {destination: None, previous: None}}
    pub async fn next(&mut self) -> PushToken {
        if self.destination.is_none() {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            DispatchQueue::main().exec_async(move || {
                let destination = PUSH.with(|push| push.as_ref().map(|push| {
                    push.destination.subscribe()
                }));
                let _ = sender.send(destination);
            });
            self.destination = receiver.await.unwrap();
        }
        let Some(destination) = self.destination.as_mut() else {
            return futures::future::pending().await
        };
        loop {
            let current = destination.borrow_and_update().clone();
            if let Some(current) = current {
                if self.previous.as_ref() != Some(&current) {
                    self.previous = Some(current.clone());
                    return PushToken::Apns(current)
                }
                destination.changed().await.unwrap();
                continue;
            }
            tokio::select! {
                biased;
                changed = destination.changed() => changed.unwrap(),
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    DispatchQueue::main().exec_async(|| PUSH.with(|push| {
                        let push = push.as_ref().unwrap();
                        if push.destination.borrow().is_none()
                            && push.requested.get().elapsed() >= Duration::from_secs(30) {
                            push.requested.set(Instant::now());
                            NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
                                .registerForRemoteNotifications();
                        }
                    }));
                }
            }
        }
    }
    pub fn notify(proxy: EventLoopProxy<Output>, header: String, body: String) {
        DispatchQueue::main().exec_async(move || {
            let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            if app.isActive() && !app.isHidden() {
                let _ = proxy.send_event(Output::Input(Input::Notification(header, body)));
            } else {super::apple::Notifications::send(&header, &body);}
        });
    }
}

pub struct Share {
    media: super::apple::ShareMedia,
}

impl Share {
    pub fn new() -> Self {Share{media: super::apple::ShareMedia::new()}}

    #[allow(deprecated)]
    pub fn share(&self, data: &Data) {
        let mtm = MainThreadMarker::new().unwrap();
        let item: Retained<AnyObject> = match data {
            Data::Text(text) => NSString::from_str(text).into(),
            Data::Image(image) => {
                let (width, height) = image.dimensions;
                let size = CGSize::new(width as f64, height as f64);
                let image = super::apple::ShareMedia::native_image(image);
                NSImage::initWithCGImage_size(NSImage::alloc(), &image, size).into()
            }
            Data::File(path) => {
                let path = NSString::from_str(&path.to_string_lossy());
                NSURL::fileURLWithPath(&path).into()
            }
            Data::Media(_) => {
                let Some(path) = self.media.path(data) else {return};
                let path = path.to_string_lossy();
                NSURL::fileURLWithPath(&NSString::from_str(&path)).into()
            }
        };

        let item = MainThreadBound::new(item, mtm);
        DispatchQueue::main().exec_async(move || {
            let mtm = MainThreadMarker::new().unwrap();
            let window = Presentation::window(mtm).unwrap();
            let view = window.contentView().unwrap();
            let picker = unsafe {NSSharingServicePicker::initWithItems(
                NSSharingServicePicker::alloc(),
                &NSArray::from_retained_slice(&[item.get(mtm).clone()]),
            )};
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            window.makeKeyAndOrderFront(None);
            picker.showRelativeToRect_ofView_preferredEdge(view.bounds(), &view, NSRectEdge::MinY);
        });
    }
}

pub struct DataPicker {
    tx: AsyncTx<spsc::List<PickRequest>>,
    delegate: Retained<LibraryPickerDelegate>,
}

impl DataPicker {
    pub fn image(data: &NSData) -> Result<Image, String> {
        if let Ok(image) = Image::read(data.to_vec().as_slice()) {return Ok(image)}
        let image = NSImage::initWithData(NSImage::alloc(), data).and_then(|image| unsafe {
            image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
        }).ok_or_else(|| "The image could not be decoded.".to_owned())?;
        Ok(super::apple::ShareMedia::image(&image))
    }
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("main thread");
        let (tx, rx) = spsc::build(spsc::List::new());
        let ivars = PickerIvars {
            rx, ui: RefCell::new(None), current: RefCell::new(None),
        };
        let delegate = LibraryPickerDelegate::alloc(mtm).set_ivars(ivars);
        let delegate: Retained<LibraryPickerDelegate> =
            unsafe { msg_send![super(delegate), init] };
        let center = NSNotificationCenter::defaultCenter();
        unsafe {
            for name in [NSWindowDidBecomeKeyNotification, NSApplicationDidBecomeActiveNotification] {
                center.addObserver_selector_name_object(&delegate, sel!(pickerReady:), Some(name), None);
            }
        }
        DataPicker { tx, delegate }
    }

    pub fn pick<P: Pickable>(&self, hardware: &Hardware) -> Picked<P> {
        let (tx, rx): (AsyncTx<spsc::One<PickResult>>, _) = spsc::build(spsc::One::new());
        self.tx.try_send(P::request(tx)).unwrap();
        let delegate = MainThreadBound::new(self.delegate.clone(), self.delegate.mtm());
        DispatchQueue::main().exec_async(move || {
            delegate.get(MainThreadMarker::new().unwrap()).show();
        });
        Picked::new(rx, hardware)
    }

    pub fn cancel(&self) {self.delegate.cancel();}
}

impl Drop for DataPicker {
    fn drop(&mut self) {
        unsafe {NSNotificationCenter::defaultCenter().removeObserver(&self.delegate);}
        if let Some(state) = self.delegate.ivars().current.borrow_mut().take() {
            match state {
                PickState::Waiting(request) | PickState::Selecting(request) |
                    PickState::Dismissing(request) | PickState::Loading(request) |
                    PickState::Rejected(request, _) | PickState::Alerting(request) => request.cancel(),
            }
        }
        while let Ok(request) = self.delegate.ivars().rx.try_recv() {request.cancel();}
        let ui = self.delegate.ivars().ui.borrow_mut().take();
        match ui {
            Some(PickerUI::Photos(window, picker)) => {
                window.setDelegate(None);
                unsafe {picker.setDelegate(None);}
                window.close();
            }
            Some(PickerUI::Documents(panel)) => unsafe {panel.cancel(None);},
            Some(PickerUI::Alert(alert)) => {
                let window = alert.window();
                if let Some(parent) = window.sheetParent() {
                    parent.endSheet_returnCode(&window, NSModalResponseCancel);
                }
                window.orderOut(None);
            }
            None => {},
        }
    }
}

#[derive(Clone)]
enum PickerUI {
    Photos(Retained<NSWindow>, Retained<PHPickerViewController>),
    Documents(Retained<NSOpenPanel>),
    Alert(Retained<NSAlert>),
}

struct PickerIvars {
    rx: AsyncRx<spsc::List<PickRequest>>,
    ui: RefCell<Option<PickerUI>>,
    current: RefCell<Option<PickState>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PickerIvars]
    struct LibraryPickerDelegate;

    unsafe impl NSObjectProtocol for LibraryPickerDelegate {}

    impl LibraryPickerDelegate {
        #[unsafe(method(pickerReady:))]
        fn picker_ready(&self, _notification: &NSNotification) {self.show();}
    }

    unsafe impl PHPickerViewControllerDelegate for LibraryPickerDelegate {
        #[unsafe(method(picker:didFinishPicking:))]
        fn picker_did_finish_picking(&self, picker: &PHPickerViewController, results: &NSArray<PHPickerResult>) {
            if !matches!(self.ivars().ui.borrow().as_ref(),
                Some(PickerUI::Photos(_, shown)) if std::ptr::eq(&**shown, picker)) {return}
            if !matches!(*self.ivars().current.borrow(), Some(PickState::Selecting(_))) {return}
            let Some(PickState::Selecting(request)) = self.ivars().current.borrow_mut().take()
                else {unreachable!()};
            let id = request.id();
            *self.ivars().current.borrow_mut() = Some(if results.is_empty() {
                PickState::Dismissing(request)
            } else {PickState::Loading(request)});
            let ui = self.ivars().ui.borrow_mut().take();
            let Some(PickerUI::Photos(window, picker)) = ui else {unreachable!()};
            window.setDelegate(None);
            unsafe {picker.setDelegate(None);}
            window.close();
            if results.is_empty() {self.dismissed(&id); return}
            let delegate = MainThreadBound::new(self.retain(), self.mtm());
            let current = self.ivars().current.borrow();
            let Some(PickState::Loading(request)) = current.as_ref()
                .filter(|state| state.request().matches(&id)) else {return};
            request.load_photos(results, move |items| {
                delegate.get(MainThreadMarker::new().expect("main thread")).loaded(&id, items);
            });
        }
    }

    unsafe impl NSWindowDelegate for LibraryPickerDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, notification: &NSNotification) {
            let Some(closing) = notification.object().and_then(|object| object.downcast::<NSWindow>().ok())
                else {return};
            if !matches!(self.ivars().ui.borrow().as_ref(),
                Some(PickerUI::Photos(window, _)) if std::ptr::eq(&**window, &*closing)) {return}
            if !matches!(*self.ivars().current.borrow(), Some(PickState::Selecting(_))) {return}
            let Some(PickState::Selecting(request)) = self.ivars().current.borrow_mut().take()
                else {unreachable!()};
            let id = request.id();
            *self.ivars().current.borrow_mut() = Some(PickState::Dismissing(request));
            let ui = self.ivars().ui.borrow_mut().take();
            let Some(PickerUI::Photos(window, picker)) = ui else {unreachable!()};
            window.setDelegate(None);
            unsafe {picker.setDelegate(None);}
            window.orderOut(None);
            self.dismissed(&id);
        }
    }
);

impl LibraryPickerDelegate {
    fn cancel(&self) {
        let state = {
            let mut current = self.ivars().current.borrow_mut();
            if current.as_ref().is_some_and(|state| {
                !state.request().disconnected() || matches!(state, PickState::Dismissing(_))
            }) {return}
            current.take()
        };
        match state {
            Some(PickState::Selecting(request) | PickState::Alerting(request)) => {
                let id = request.id();
                *self.ivars().current.borrow_mut() = Some(PickState::Dismissing(request));
                let ui = self.ivars().ui.borrow().clone().expect("Active picker has no native UI");
                match ui {
                    PickerUI::Photos(window, picker) => {
                        window.setDelegate(None);
                        unsafe {picker.setDelegate(None);}
                        window.close();
                        self.dismissed(&id);
                    }
                    PickerUI::Documents(panel) => unsafe {panel.cancel(None);},
                    PickerUI::Alert(alert) => {
                        let window = alert.window();
                        if let Some(parent) = window.sheetParent() {
                            parent.endSheet_returnCode(&window, NSModalResponseCancel);
                        } else {
                            window.orderOut(None);
                            self.dismissed(&id);
                        }
                    }
                }
            }
            Some(PickState::Dismissing(_)) => unreachable!(),
            _ => self.show(),
        }
    }

    fn dismissed(&self, id: &Arc<()>) {
        if !matches!(self.ivars().current.borrow().as_ref(),
            Some(PickState::Dismissing(request)) if request.matches(id)) {return}
        let Some(PickState::Dismissing(request)) = self.ivars().current.borrow_mut().take()
            else {unreachable!()};
        self.ivars().ui.borrow_mut().take();
        request.cancel();
        self.show();
    }

    fn loaded(&self, id: &Arc<()>, items: Vec<Result<Data, String>>) {
        if !matches!(self.ivars().current.borrow().as_ref(),
            Some(PickState::Loading(request)) if request.matches(id)) {return}
        let Some(PickState::Loading(mut request)) = self.ivars().current.borrow_mut().take()
            else {unreachable!()};
        if request.disconnected() {self.show(); return}
        if let Some(message) = request.accept(items) {
            *self.ivars().current.borrow_mut() = Some(PickState::Rejected(request, message));
        } else {request.finish();}
        self.show();
    }

    #[allow(deprecated)]
    fn show(&self) {
        if matches!(*self.ivars().current.borrow(), Some(
            PickState::Selecting(_) | PickState::Loading(_) | PickState::Alerting(_)
                | PickState::Dismissing(_)
        )) {return}
        let Some(host) = Presentation::window(self.mtm()) else {return};
        let app = NSApplication::sharedApplication(self.mtm());
        let state = loop {
            let state = self.ivars().current.borrow_mut().take()
                .or_else(|| self.ivars().rx.try_recv().ok().map(PickState::Waiting));
            if state.as_ref().is_some_and(|state| state.request().disconnected()) {continue}
            break state;
        };
        let request = match state {
            Some(PickState::Waiting(request)) => request,
            Some(PickState::Rejected(request, message)) => {
                let alert = NSAlert::new(self.mtm());
                alert.setMessageText(&NSString::from_str("Some items could not be opened"));
                alert.setInformativeText(&NSString::from_str(&message));
                alert.addButtonWithTitle(&NSString::from_str("Choose Again"));
                let has_items = request.has_items();
                if has_items {alert.addButtonWithTitle(&NSString::from_str("Use Valid Items"));}
                alert.addButtonWithTitle(&NSString::from_str("Cancel"));
                let id = request.id();
                *self.ivars().current.borrow_mut() = Some(PickState::Alerting(request));
                *self.ivars().ui.borrow_mut() = Some(PickerUI::Alert(alert.clone()));
                let delegate = self.retain();
                let shown = alert.clone();
                let block = RcBlock::new(move |response: NSModalResponse| {
                    if !matches!(delegate.ivars().ui.borrow().as_ref(),
                        Some(PickerUI::Alert(alert)) if std::ptr::eq(&**alert, &*shown)) {return}
                    if matches!(delegate.ivars().current.borrow().as_ref(),
                        Some(PickState::Dismissing(request)) if request.matches(&id)) {
                        shown.window().orderOut(None);
                        delegate.dismissed(&id);
                        return;
                    }
                    if !matches!(delegate.ivars().current.borrow().as_ref(),
                        Some(PickState::Alerting(request)) if request.matches(&id)) {return}
                    shown.window().orderOut(None);
                    if !matches!(delegate.ivars().current.borrow().as_ref(),
                        Some(PickState::Alerting(request)) if request.matches(&id)) {return}
                    let Some(PickState::Alerting(request)) =
                        delegate.ivars().current.borrow_mut().take() else {unreachable!()};
                    delegate.ivars().ui.borrow_mut().take();
                    if request.disconnected() {
                        request.cancel();
                    } else if response == NSAlertFirstButtonReturn {
                        *delegate.ivars().current.borrow_mut() = Some(PickState::Waiting(request));
                    } else if has_items && response == NSAlertSecondButtonReturn {
                        request.finish();
                    } else {
                        let cancel = if has_items {
                            NSAlertThirdButtonReturn
                        } else {NSAlertSecondButtonReturn};
                        assert_eq!(response, cancel, "Apple returned an unexpected alert response");
                        request.cancel();
                    }
                    delegate.show();
                });
                app.activateIgnoringOtherApps(true);
                alert.beginSheetModalForWindow_completionHandler(&host, Some(&block));
                return;
            }
            None => return,
            _ => unreachable!(),
        };
        if request.documents() {
            let panel = NSOpenPanel::openPanel(self.mtm());
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(false);
            panel.setAllowsMultipleSelection(request.multiple());
            let save = panel.as_super();
            let types = request.file_types();
            save.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
            let id = request.id();
            *self.ivars().current.borrow_mut() = Some(PickState::Selecting(request));
            *self.ivars().ui.borrow_mut() = Some(PickerUI::Documents(panel.clone()));
            let delegate = self.retain();
            let shown = panel.clone();
            let block = RcBlock::new(move |response: NSModalResponse| {
                if !matches!(delegate.ivars().ui.borrow().as_ref(),
                    Some(PickerUI::Documents(panel)) if std::ptr::eq(&**panel, &*shown)) {return}
                if matches!(delegate.ivars().current.borrow().as_ref(),
                    Some(PickState::Dismissing(request)) if request.matches(&id)) {
                    shown.orderOut(None);
                    delegate.dismissed(&id);
                    return;
                }
                if !matches!(delegate.ivars().current.borrow().as_ref(),
                    Some(PickState::Selecting(request)) if request.matches(&id)) {return}
                let Some(PickState::Selecting(request)) = delegate.ivars().current.borrow_mut().take()
                    else {unreachable!()};
                if response == NSModalResponseCancel || request.disconnected() {
                    *delegate.ivars().current.borrow_mut() = Some(PickState::Dismissing(request));
                    shown.orderOut(None);
                    delegate.dismissed(&id);
                    return;
                }
                assert_eq!(response, NSModalResponseOK, "Apple failed to display the file picker");
                *delegate.ivars().current.borrow_mut() = Some(PickState::Loading(request));
                delegate.ivars().ui.borrow_mut().take();
                shown.orderOut(None);
                let completion = MainThreadBound::new(delegate.clone(), delegate.mtm());
                let current = delegate.ivars().current.borrow();
                let Some(PickState::Loading(request)) = current.as_ref()
                    .filter(|state| state.request().matches(&id)) else {return};
                let id = id.clone();
                request.load_files(&shown.URLs(), move |items| {
                    completion.get(MainThreadMarker::new().expect("main thread")).loaded(&id, items);
                });
            });
            app.activateIgnoringOtherApps(true);
            save.beginWithCompletionHandler(&block);
            return;
        }
        let config = request.photos();
        let picker = unsafe {
            PHPickerViewController::initWithConfiguration(
                PHPickerViewController::alloc(self.mtm()), &config,
            )
        };
        unsafe { picker.setDelegate(Some(ProtocolObject::from_ref(self))); }

        let frame = host.frame();
        let content = host.contentRectForFrameRect(frame);
        // A window whose content view controller is the picker follows the
        // picker's preferredContentSize, which PHPicker sets tiny once loaded.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(self.mtm()), content, host.styleMask(),
                NSBackingStoreType::Buffered, false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false); }
        window.setContentMinSize(content.size);
        window.setFrame_display(frame, true);
        let root = NSViewController::new(self.mtm());
        root.setPreferredContentSize(content.size);
        window.setContentViewController(Some(&root));
        root.addChildViewController(&picker);
        let view = picker.view();
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        view.setFrame(root.view().bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        root.view().addSubview(&view);
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.setFrame_display(frame, true);
        *self.ivars().current.borrow_mut() = Some(PickState::Selecting(request));
        *self.ivars().ui.borrow_mut() = Some(PickerUI::Photos(window.clone(), picker));
        app.activateIgnoringOtherApps(true);
        window.makeKeyAndOrderFront(None);
        window.setFrame_display(frame, true);
        view.setFrame(root.view().bounds());
    }
}

#[link(name = "Cocoa", kind = "framework")]
unsafe extern "C" {}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {}

struct DeviceVolume {
    device: u32,
    levels: Vec<(u32, f32)>,
    listener: RcBlock<dyn Fn(u32, NonNull<AudioObjectPropertyAddress>)>,
}
impl DeviceVolume {
    fn new(proxy: EventLoopProxy<Output>) -> Rc<RefCell<Self>> {
        let volume: Rc<RefCell<Self>> = Rc::new_cyclic(|weak: &std::rc::Weak<RefCell<Self>>| {
            let weak = weak.clone();
            let listener = RcBlock::new(move |count, addresses: NonNull<AudioObjectPropertyAddress>| {
                let Some(volume) = weak.upgrade() else {return};
                let addresses = unsafe {std::slice::from_raw_parts(addresses.as_ptr(), count as usize)};
                let mut volume = volume.borrow_mut();
                if addresses.iter().any(|a| a.mSelector == kAudioHardwarePropertyDefaultOutputDevice) {
                    volume.connect();
                    return
                }
                let device = volume.device;
                let old: f32 = volume.levels.iter().map(|(_, level)| level).sum();
                for (channel, level) in &mut volume.levels {
                    if let Some(value) = Self::read(device, AudioObjectPropertyAddress {
                        mSelector: kAudioDevicePropertyVolumeScalar,
                        mScope: kAudioObjectPropertyScopeOutput, mElement: *channel,
                    }) {*level = value;}
                }
                let new: f32 = volume.levels.iter().map(|(_, level)| level).sum();
                if new > old {
                    let _ = proxy.send_event(Output::Input(Input::Media(MediaInput::VolumeUp)));
                }
                if new < old {
                    let _ = proxy.send_event(Output::Input(Input::Media(MediaInput::VolumeDown)));
                }
            });
            RefCell::new(Self {device: 0, levels: Vec::new(), listener})
        });
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal, mElement: 0,
        };
        let result = unsafe {AudioObjectAddPropertyListenerBlock(
            kAudioObjectSystemObject as u32, NonNull::from(&mut address), Some(&DispatchQueue::main()),
            RcBlock::as_ptr(&volume.borrow().listener),
        )};
        if result != 0 {log::warn!("Could not observe default output: {result}");}
        volume.borrow_mut().connect();
        volume
    }
    fn read<T: Default + Copy>(device: u32, mut address: AudioObjectPropertyAddress) -> Option<T> {
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        let result = unsafe {AudioObjectGetPropertyData(
            device, NonNull::from(&mut address), 0, std::ptr::null(),
            NonNull::from(&mut size), NonNull::from(&mut value).cast(),
        )};
        (result == 0).then_some(value)
    }
    fn connect(&mut self) {
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyVolumeScalar,
            mScope: kAudioObjectPropertyScopeOutput, mElement: kAudioObjectPropertyElementWildcard,
        };
        if self.device != 0 {unsafe {AudioObjectRemovePropertyListenerBlock(
            self.device, NonNull::from(&mut address), Some(&DispatchQueue::main()),
            RcBlock::as_ptr(&self.listener),
        );}}
        self.levels.clear();
        self.device = Self::read(kAudioObjectSystemObject as u32, AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal, mElement: 0,
        }).unwrap_or(0);
        if self.device == 0 {return}
        if let Some(level) = Self::read(self.device, AudioObjectPropertyAddress {
            mElement: 0, ..address
        }) {self.levels.push((0, level));} else {
            let channels = Self::read::<[u32; 2]>(self.device, AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyPreferredChannelsForStereo,
                mScope: kAudioObjectPropertyScopeOutput, mElement: 0,
            });
            for channel in channels.into_iter().flatten() {
                if let Some(level) = Self::read(self.device, AudioObjectPropertyAddress {
                    mElement: channel, ..address
                }) {self.levels.push((channel, level));}
            }
        }
        if self.levels.is_empty() {return}
        let result = unsafe {AudioObjectAddPropertyListenerBlock(
            self.device, NonNull::from(&mut address), Some(&DispatchQueue::main()),
            RcBlock::as_ptr(&self.listener),
        )};
        if result != 0 {log::warn!("Could not observe output volume: {result}");}
    }
}
impl Drop for DeviceVolume {
    fn drop(&mut self) {
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal, mElement: 0,
        };
        unsafe {AudioObjectRemovePropertyListenerBlock(
            kAudioObjectSystemObject as u32, NonNull::from(&mut address), Some(&DispatchQueue::main()),
            RcBlock::as_ptr(&self.listener),
        );}
        if self.device == 0 {return}
        address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyVolumeScalar,
            mScope: kAudioObjectPropertyScopeOutput, mElement: kAudioObjectPropertyElementWildcard,
        };
        unsafe {AudioObjectRemovePropertyListenerBlock(
            self.device, NonNull::from(&mut address), Some(&DispatchQueue::main()),
            RcBlock::as_ptr(&self.listener),
        );}
    }
}

struct Presentation;

impl Presentation {
    fn window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        let app = NSApplication::sharedApplication(mtm);
        app.keyWindow().into_iter().chain(app.mainWindow()).chain(app.windows())
            .find(|window| window.isVisible() && !window.isMiniaturized()
                && window.canBecomeMainWindow() && window.contentView().is_some())
    }
}


struct _Push {
    destination: tokio::sync::watch::Sender<Option<String>>,
    requested: Cell<Instant>,
}

impl _Push {
    fn new() -> Option<Self> {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {return None}
        let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
        let delegate = app.delegate().unwrap();
        let delegate: &AnyObject = (*delegate).as_ref();
        let class = delegate.class() as *const _ as *mut _;
        // Winit 0.30 requires its delegate's identity and ivars to remain intact. Add only
        // the APNs selectors it does not implement, with void(id, SEL, id, id) signatures.
        let callbacks: [(Sel, Imp); 2] = unsafe {[
            (sel!(application:didRegisterForRemoteNotificationsWithDeviceToken:),
                std::mem::transmute(Self::registered as extern "C-unwind" fn(_, _, _, _))),
            (sel!(application:didFailToRegisterForRemoteNotificationsWithError:),
                std::mem::transmute(Self::failed as extern "C-unwind" fn(_, _, _, _))),
        ]};
        for (selector, callback) in callbacks {
            assert!(unsafe {objc2::ffi::class_addMethod(class, selector, callback, c"v@:@@".as_ptr())}.as_bool(),
                "The application delegate already implements {selector}");
        }
        // Initialize the thread-local state before AppKit can call back into it.
        DispatchQueue::main().exec_async(|| {
            super::apple::Notifications::authorize();
            NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
                .registerForRemoteNotifications();
        });
        Some(Self {
            destination: tokio::sync::watch::channel(None).0,
            requested: Cell::new(Instant::now()),
        })
    }

    extern "C-unwind" fn registered(_delegate: &AnyObject, _selector: Sel, _app: &NSApplication, token: &NSData) {
        let destination = hex::encode(token.to_vec());
        PUSH.with(|push| push.as_ref().unwrap().destination.send_replace(Some(destination)));
    }

    extern "C-unwind" fn failed(_delegate: &AnyObject, _selector: Sel, _app: &NSApplication, _error: &NSError) {
        PUSH.with(|push| push.as_ref().unwrap().destination.send_replace(None));
    }
}
