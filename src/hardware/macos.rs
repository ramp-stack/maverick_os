use std::cell::RefCell;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use image::{RgbaImage, Rgba, ImageBuffer, ImageEncoder};
use image::codecs::png::PngEncoder;
use crossfire::{spsc, AsyncRx, AsyncTx};
use dispatch2::MainThreadBound;

use crate::hardware::{Data, Image, Pickable, Picked, PickResult, Video};
use super::apple::{MovieType, PickRequest, PickState};

use block2::RcBlock;
use objc2::{
    define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
    AnyThread, Message,
};
use objc2::runtime::{ProtocolObject, AnyObject};
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
use objc2_core_foundation::{CGSize, CGPoint, CGRect};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorSpace, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGDataProvider, CGImage, CGColorRenderingIntent,
    CGBitmapContextGetData, CGBitmapContextCreate, CGContext
};
use objc2_foundation::{
    NSArray, NSData, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol,
    NSString, NSRectEdge, NSURL,
};
use objc2_photos_ui::{
    PHPickerResult, PHPickerViewController,
    PHPickerViewControllerDelegate,
};

pub struct SafeArea;
impl SafeArea {
    pub fn get() -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }
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
                let mut buf = Cursor::new(Vec::new());
                let encoder = PngEncoder::new(&mut buf);
                encoder.write_image(
                    img.as_raw(),
                    img.width(),
                    img.height(),
                    image::ColorType::Rgba8,
                ).unwrap();
                let bytes = NSData::with_bytes(&buf.into_inner());
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
            Data::Video(video) => {
                let bytes = NSData::with_bytes(video.bytes());
                let ty = MovieType::get(video).identifier();
                pb.setData_forType(Some(&bytes), &ty)
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
                if MovieType::matches(&ty) && let Some(data) = pb.dataForType(&ty)
                    && let Some(video) = Video::new(data.to_vec())
                {
                    return Some(Data::Video(video));
                }
            }
        }
        for ty in [unsafe { NSPasteboardTypePNG }, unsafe { NSPasteboardTypeTIFF }] {
            if let Some(data) = pb.dataForType(ty)
                && let Ok(img) = image::load_from_memory(&data.to_vec()) {
                return Some(Data::Image(Arc::new(img.into_rgba8())));
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

pub struct Share {
    movie: super::apple::ShareMovie,
}

impl Share {
    pub fn new() -> Self {Share{movie: super::apple::ShareMovie::new()}}

    /// Shows the macOS share picker for a single item.
    /// Automatically anchors to the key window's content view.
    /// Call on the main thread.
    pub fn share(&self, data: &Data) {
        let app = NSApplication::sharedApplication(objc2::MainThreadMarker::new().unwrap());

        let window = app.keyWindow().or_else(|| app.mainWindow()).unwrap();
        let view = window.contentView().unwrap();

        let item: Retained<AnyObject> = match data {
            Data::Text(text) => NSString::from_str(text).into(),
            Data::Image(image) => image_to_nsimage(image.clone()).into(),
            Data::File(path) => {
                let path = NSString::from_str(&path.to_string_lossy());
                NSURL::fileURLWithPath(&path).into()
            }
            Data::Video(video) => {
                let Some(path) = self.movie.path(video) else {return};
                let path = path.to_string_lossy();
                NSURL::fileURLWithPath(&NSString::from_str(&path)).into()
            }
        };

        let picker = unsafe {
            NSSharingServicePicker::initWithItems(
                NSSharingServicePicker::alloc(),
                &NSArray::from_retained_slice(&[item]),
            )
        };

        let rect = view.bounds();
        picker.showRelativeToRect_ofView_preferredEdge(
            rect,
            &view,
            NSRectEdge::MinY,
        );
    }
}

pub struct DataPicker {
    tx: AsyncTx<spsc::List<PickRequest>>,
    delegate: Retained<LibraryPickerDelegate>,
}

impl DataPicker {
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

    pub fn pick<P: Pickable>(self: &Rc<Self>) -> Picked<P> {
        let (tx, rx): (AsyncTx<spsc::One<PickResult>>, _) = spsc::build(spsc::One::new());
        self.tx.try_send(P::request(tx)).unwrap();
        self.delegate.show();
        Picked::new(rx, self.clone())
    }

    pub fn cancel(&self) {self.delegate.cancel();}
}

impl Drop for DataPicker {
    fn drop(&mut self) {
        unsafe {NSNotificationCenter::defaultCenter().removeObserver(&self.delegate);}
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
            request.load_media(results, move |items| {
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

    fn show(&self) {
        if matches!(*self.ivars().current.borrow(), Some(
            PickState::Selecting(_) | PickState::Loading(_) | PickState::Alerting(_)
                | PickState::Dismissing(_)
        )) {return}
        let app = NSApplication::sharedApplication(self.mtm());
        if !app.isActive() {return}
        let Some(host) = app.keyWindow()
            .filter(|window| window.isVisible() && !window.isMiniaturized())
            .or_else(|| app.mainWindow()
                .filter(|window| window.isVisible() && !window.isMiniaturized())) else {return};
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
        window.makeKeyAndOrderFront(None);
        window.setFrame_display(frame, true);
        view.setFrame(root.view().bounds());
    }
}

fn image_to_nsimage(image: Image) -> Retained<NSImage> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let bytes_per_row = width * 4;
    let raw = image.as_raw();
    let data_ptr = raw.as_ptr() as *const std::ffi::c_void;
    let data_len = raw.len();

    let info = Arc::into_raw(image) as *mut std::ffi::c_void;

    unsafe extern "C-unwind" fn release_callback(
        info: *mut std::ffi::c_void,
        _data: std::ptr::NonNull<std::ffi::c_void>,
        _size: usize,
    ) {
        if !info.is_null() {
            drop(unsafe { Arc::from_raw(info as *const RgbaImage) });
        }
    }

    let provider = unsafe {
        CGDataProvider::with_data(info, data_ptr, data_len, Some(release_callback)).unwrap()
    };
    let color_space = CGColorSpace::new_device_rgb().unwrap();
    // image::Rgba is straight (not premultiplied) RGBA, top row first.
    let bitmap_info = CGBitmapInfo(
        CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0,
    );

    let cg_image = unsafe {
        CGImage::new(
            width,
            height,
            8,
            32,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
            Some(&provider),
            std::ptr::null(),
            true,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
        .unwrap()
    };

    NSImage::initWithCGImage_size(
        NSImage::alloc(),
        &cg_image,
        CGSize::new(width as f64, height as f64),
    )
}

pub fn nsimage_to_image(image: &NSImage) -> Option<Image> {
    let cg = unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }?;

    let width = CGImage::width(Some(&*cg)) as u32;
    let height = CGImage::height(Some(&*cg)) as u32;
    let row = width as usize * 4;

    let color_space = CGColorSpace::new_device_rgb().expect("Could not create an RGB color space");
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width as usize,
            height as usize,
            8,
            row,
            Some(&color_space),
            info,
        )
    }.expect("Could not create an RGBA bitmap context");

    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        ),
        Some(&*cg),
    );

    let ptr = CGBitmapContextGetData(Some(&context)) as *const u8;
    let mut bytes = unsafe { std::slice::from_raw_parts(ptr, row * height as usize) }.to_vec();
    unpremultiply(&mut bytes);
    Some(Arc::new(ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bytes)
        .expect("RGBA bitmap dimensions did not match its buffer")))
}

fn unpremultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            px[0] = (px[0] as u32 * 255 / a).min(255) as u8;
            px[1] = (px[1] as u32 * 255 / a).min(255) as u8;
            px[2] = (px[2] as u32 * 255 / a).min(255) as u8;
        }
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
