use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::hardware::{Data, Image, Pickable, Picked, PickResult, Video};
use super::apple::{MovieType, PickRequest, PickState};
use image::{ImageBuffer, RgbaImage};
use crossfire::{spsc, AsyncRx, AsyncTx};

use block2::RcBlock;
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::{
    MainThreadOnly, MainThreadMarker, extern_class, extern_methods,
    define_class, msg_send, sel, DefinedClass, Message
};
use objc2_foundation::{
    NSString, NSArray, NSSet, NSData, NSNotification, NSNotificationCenter, NSObject,
    NSObjectProtocol, NSURL,
};
use objc2_core_foundation::{CGPoint, CGSize, CGRect};
use objc2::runtime::ProtocolObject;
use objc2::rc::{Retained, Allocated, Weak};
use objc2_ui_kit::{
    UIPasteboard, UIApplication, UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UIImage,
    UIViewController, UIWindow, UIWindowScene, UISceneActivationState, UIScene,
    UIActivityViewController, UIResponder, UIDocumentPickerViewController,
    UIDocumentPickerDelegate, UIApplicationState, UIApplicationDidBecomeActiveNotification,
    UIWindowDidBecomeKeyNotification, UIWindowDidBecomeVisibleNotification,
    UIViewControllerTransitionCoordinator, UIViewControllerTransitionCoordinatorContext,
    UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
    UIAdaptivePresentationControllerDelegate, UIPresentationController,
};
use objc2_core_graphics::{
    CGImage, CGColorRenderingIntent, CGBitmapInfo, CGColorSpace, CGDataProvider,
    CGImageAlphaInfo, CGContext, CGBitmapContextCreate, CGImageByteOrderInfo
};
use objc2_photos_ui::{
    PHPickerConfiguration, PHPickerResult,
    PHPickerViewControllerDelegate,
};

pub struct SafeArea;
impl SafeArea {
    pub fn get() -> (f32, f32, f32, f32) {
        let insets = get_key_window().safeAreaInsets();
        (insets.top as f32, insets.left as f32, insets.bottom as f32, insets.right as f32)
    }
}

pub struct Clipboard(Retained<UIPasteboard>);
impl Clipboard {
    pub fn new() -> Self {Self(UIPasteboard::generalPasteboard())}

    pub fn get(&self) -> Option<Data> {
        unsafe {
            if let Some(url) = self.0.URL()
                && url.isFileURL()
                && let Some(path) = url.path()
            {
                return Some(Data::File(PathBuf::from(path.to_string())));
            }
            if let Some(s) = self.0.string() {
                return Some(Data::Text(s.to_string()));
            }
            for ty in self.0.pasteboardTypes().iter() {
                if MovieType::matches(&ty) && let Some(data) = self.0.dataForPasteboardType(&ty)
                    && let Some(video) = Video::new(data.to_vec())
                {
                    return Some(Data::Video(video));
                }
            }
            if let Some(i) = self.0.image() && let Some(image) = uiimage_to_image(&i) {
                return Some(Data::Image(image));
            }
            None
        }
    }

    pub fn set(&self, data: &Data) {
        unsafe {
            match data {
                Data::Text(text) => {
                    let ns_string = NSString::from_str(text);
                    self.0.setString(Some(&ns_string));
                }
                Data::Image(image) => {
                    self.0.setImage(Some(&image_to_uiimage(image.clone())));
                }
                Data::File(path) => {
                    let path = NSString::from_str(&path.to_string_lossy());
                    self.0.setURL(Some(&NSURL::fileURLWithPath(&path)));
                }
                Data::Video(video) => {
                    let bytes = NSData::with_bytes(video.bytes());
                    self.0.setData_forPasteboardType(&bytes, &MovieType::get(video).identifier());
                }
            }
        }
    }
}

pub struct Haptics(std::cell::RefCell<Option<Retained<UIImpactFeedbackGenerator>>>);
impl Haptics {
    pub fn new() -> Self {Self(std::cell::RefCell::new(None))}

    pub fn trigger(&self) {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            let root = get_key_window().rootViewController().expect("no root view controller");
            let generator = UIImpactFeedbackGenerator::feedbackGeneratorWithStyle_forView(
                UIImpactFeedbackStyle::Medium,
                &root.view().unwrap(),
            );
            generator.prepare();
            *slot = Some(generator);
        }
        slot.as_ref().unwrap().impactOccurred();
    }
}

pub struct Share {
    movie: super::apple::ShareMovie,
}
impl Share {
    pub fn new() -> Self {Share{movie: super::apple::ShareMovie::new()}}

    /// Opens the system share sheet for a single item.
    /// Must be called on the main thread.
    pub fn share(&self, data: &Data) {
        let item: Retained<_> = match data {
            Data::Text(text) => NSString::from_str(text).into_super(),
            Data::Image(image) => image_to_uiimage(image.clone()).into_super(),
            Data::File(path) => {
                let path = NSString::from_str(&path.to_string_lossy());
                NSURL::fileURLWithPath(&path).into_super()
            }
            Data::Video(video) => {
                let Some(path) = self.movie.path(video) else {return};
                let path = path.to_string_lossy();
                NSURL::fileURLWithPath(&NSString::from_str(&path)).into_super()
            }
        };

        let activity_vc = unsafe {
            UIActivityViewController::initWithActivityItems_applicationActivities(
                UIActivityViewController::alloc(MainThreadMarker::new().unwrap()),
                &NSArray::from_retained_slice(&[item.into()]),
                None,
            )
        };

        let vc = get_view_controller();
        vc.presentViewController_animated_completion(&activity_vc, true, None);
    }
}

extern_class!(
    #[unsafe(super(UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct PHPickerViewController;
);

impl PHPickerViewController {
    extern_methods!(
        #[unsafe(method(initWithConfiguration:))]
        #[unsafe(method_family = init)]
        pub unsafe fn init_with_configuration(
            this: Allocated<Self>,
            configuration: &PHPickerConfiguration,
        ) -> Retained<Self>;

        #[unsafe(method(setDelegate:))]
        #[unsafe(method_family = none)]
        pub unsafe fn set_delegate(
            &self,
            delegate: Option<&ProtocolObject<dyn PHPickerViewControllerDelegate>>,
        );
    );
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
            rx, current: RefCell::new(None), controller: RefCell::new(None),
        };
        let delegate = LibraryPickerDelegate::alloc(mtm).set_ivars(ivars);
        let delegate: Retained<LibraryPickerDelegate> =
            unsafe { msg_send![super(delegate), init] };
        let center = NSNotificationCenter::defaultCenter();
        for name in unsafe {[
            UIApplicationDidBecomeActiveNotification, UIWindowDidBecomeKeyNotification,
            UIWindowDidBecomeVisibleNotification,
        ]} {
            unsafe {
                center.addObserver_selector_name_object(
                    &delegate, sel!(presenterReady:), Some(name), None,
                );
            }
        }
        DataPicker{tx, delegate}
    }

    pub fn pick<P: Pickable>(self: &Rc<Self>) -> Picked<P> {
        let (tx, rx): (AsyncTx<spsc::One<PickResult>>, _) = spsc::build(spsc::One::new());
        self.tx.try_send(P::request(tx)).expect("Picker queue disconnected");
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

struct PickerIvars {
    rx: AsyncRx<spsc::List<PickRequest>>,
    current: RefCell<Option<PickState>>,
    controller: RefCell<Option<Retained<UIViewController>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PickerIvars]
    struct LibraryPickerDelegate;

    unsafe impl NSObjectProtocol for LibraryPickerDelegate {}

    // Trait has no methods on iOS in objc2-photos-ui 0.3.2. Empty impl still conforms.
    unsafe impl PHPickerViewControllerDelegate for LibraryPickerDelegate {}

    unsafe impl UIAdaptivePresentationControllerDelegate for LibraryPickerDelegate {
        #[unsafe(method(presentationControllerDidDismiss:))]
        fn presentation_controller_did_dismiss(&self, presentation: &UIPresentationController) {
            let Some(id) = self.claim_selection(&presentation.presentedViewController()) else {return};
            let Some(request) = self.dismissed(&id) else {return};
            request.cancel();
            self.show();
        }
    }

    unsafe impl UIDocumentPickerDelegate for LibraryPickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn document_picker_did_pick(&self, controller: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let Some(id) = self.claim_selection(controller) else {return};
            let delegate = self.retain();
            let urls = urls.retain();
            let completion = RcBlock::new(move || {
                let Some(request) = delegate.dismissed(&id) else {return};
                if request.disconnected() || urls.is_empty() {
                    request.cancel();
                    delegate.show();
                    return;
                }
                *delegate.ivars().current.borrow_mut() = Some(PickState::Loading(request));
                let owner = MainThreadBound::new(Weak::new(&*delegate), delegate.mtm());
                let id = id.clone();
                let current = delegate.ivars().current.borrow();
                let Some(PickState::Loading(request)) = current.as_ref() else {unreachable!()};
                request.load_files(&urls, move |items| {
                    let delegate = owner.into_inner(
                        MainThreadMarker::new().expect("Picker completion off main thread"),
                    );
                    if let Some(delegate) = delegate.load() {delegate.loaded(&id, items);}
                });
            });
            Self::dismiss(controller, completion);
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn document_picker_was_cancelled(&self, controller: &UIDocumentPickerViewController) {
            let Some(id) = self.claim_selection(controller) else {return};
            let delegate = self.retain();
            let completion = RcBlock::new(move || {
                let Some(request) = delegate.dismissed(&id) else {return};
                request.cancel();
                delegate.show();
            });
            Self::dismiss(controller, completion);
        }
    }

    impl LibraryPickerDelegate {
        #[unsafe(method(presenterReady:))]
        fn presenter_ready(&self, _notification: &NSNotification) {self.show();}

        #[unsafe(method(picker:didFinishPicking:))]
        fn picker_did_finish_picking(&self, picker: &PHPickerViewController, results: &NSArray<PHPickerResult>) {
            let Some(id) = self.claim_selection(picker) else {return};
            let delegate = self.retain();
            let results = results.retain();
            let completion = RcBlock::new(move || {
                let Some(request) = delegate.dismissed(&id) else {return};
                if request.disconnected() || results.is_empty() {
                    request.cancel();
                    delegate.show();
                    return;
                }
                *delegate.ivars().current.borrow_mut() = Some(PickState::Loading(request));
                let owner = MainThreadBound::new(Weak::new(&*delegate), delegate.mtm());
                let id = id.clone();
                let current = delegate.ivars().current.borrow();
                let Some(PickState::Loading(request)) = current.as_ref() else {unreachable!()};
                request.load_media(&results, move |items| {
                    let delegate = owner.into_inner(
                        MainThreadMarker::new().expect("Picker completion off main thread"),
                    );
                    if let Some(delegate) = delegate.load() {delegate.loaded(&id, items);}
                });
            });
            Self::dismiss(picker, completion);
        }
    }
);

impl LibraryPickerDelegate {
    fn claim_selection(&self, controller: &UIViewController) -> Option<Arc<()>> {
        if !self.ivars().controller.borrow().as_ref()
            .is_some_and(|active| std::ptr::eq(&**active, controller)) {return None}
        let mut current = self.ivars().current.borrow_mut();
        if !matches!(current.as_ref(), Some(PickState::Selecting(_))) {return None}
        let Some(PickState::Selecting(request)) = current.take() else {unreachable!()};
        let id = request.id();
        *current = Some(PickState::Dismissing(request));
        Some(id)
    }

    fn dismissed(&self, id: &Arc<()>) -> Option<PickRequest> {
        let mut current = self.ivars().current.borrow_mut();
        if !matches!(current.as_ref(), Some(PickState::Dismissing(request)) if request.matches(id)) {
            return None;
        }
        let Some(PickState::Dismissing(request)) = current.take() else {unreachable!()};
        drop(current);
        self.ivars().controller.borrow_mut().take();
        Some(request)
    }

    fn cancel(&self) {
        let mut current = self.ivars().current.borrow_mut();
        let Some(state) = current.as_ref() else {
            drop(current);
            self.show();
            return;
        };
        if !state.request().disconnected() || matches!(state, PickState::Dismissing(_)) {return}
        let state = current.take().unwrap();
        match state {
            PickState::Selecting(request) | PickState::Alerting(request) => {
                let id = request.id();
                *current = Some(PickState::Dismissing(request));
                drop(current);
                let controller = self.ivars().controller.borrow().as_ref()
                    .expect("Active picker has no view controller").clone();
                let delegate = self.retain();
                let completion = RcBlock::new(move || {
                    let Some(request) = delegate.dismissed(&id) else {return};
                    request.cancel();
                    delegate.show();
                });
                Self::dismiss(&controller, completion);
            }
            PickState::Waiting(request) | PickState::Loading(request) |
                PickState::Rejected(request, _) => {
                drop(current);
                request.cancel();
                self.show();
            }
            PickState::Dismissing(_) => unreachable!(),
        }
    }

    fn dismiss(controller: &UIViewController, completion: RcBlock<dyn Fn()>) {
        let owner = MainThreadBound::new((controller.retain(), completion), controller.mtm());
        DispatchQueue::main().exec_async(move || {
            let (controller, completion) = owner.into_inner(
                MainThreadMarker::new().expect("Picker dismissal off main thread"),
            );
            let dismissing = controller.isBeingDismissed();
            if (dismissing || controller.isBeingPresented())
                && let Some(transition) = controller.transitionCoordinator()
            {
                let finished = RcBlock::new(
                    move |context: std::ptr::NonNull<ProtocolObject<dyn UIViewControllerTransitionCoordinatorContext>>| {
                        if dismissing && !unsafe {context.as_ref()}.isCancelled() {
                            completion.call(());
                        } else {Self::dismiss(&controller, completion.clone());}
                    },
                );
                transition.animateAlongsideTransition_completion(None, Some(&finished));
            } else if controller.presentingViewController().is_some() {
                controller.dismissViewControllerAnimated_completion(true, Some(&completion));
            } else {completion.call(());}
        });
    }

    fn loaded(&self, id: &Arc<()>, items: Vec<Result<Data, String>>) {
        let mut current = self.ivars().current.borrow_mut();
        if !matches!(current.as_ref(), Some(PickState::Loading(request)) if request.matches(id)) {
            return;
        }
        let Some(PickState::Loading(mut request)) = current.take() else {unreachable!()};
        drop(current);
        if request.disconnected() {request.cancel();}
        else if let Some(message) = request.accept(items) {
            *self.ivars().current.borrow_mut() = Some(PickState::Rejected(request, message));
        } else {request.finish();}
        self.show();
    }

    fn show(&self) {
        {
            let mut current = self.ivars().current.borrow_mut();
            while current.is_none() {
                let Ok(request) = self.ivars().rx.try_recv() else {return};
                if !request.disconnected() {*current = Some(PickState::Waiting(request));}
            }
        }
        let disconnected = self.ivars().current.borrow().as_ref().unwrap().request().disconnected();
        if disconnected {self.cancel(); return}
        if matches!(self.ivars().current.borrow().as_ref(),
            Some(PickState::Selecting(_) | PickState::Loading(_) |
                PickState::Alerting(_) | PickState::Dismissing(_))) {return}
        let app = UIApplication::sharedApplication(self.mtm());
        if app.applicationState() != UIApplicationState::Active {return}
        let window = app.connectedScenes().into_iter().find_map(|scene| {
            if scene.activationState() != UISceneActivationState::ForegroundActive {return None}
            scene.downcast::<UIWindowScene>().ok()?.keyWindow()
        });
        #[allow(deprecated)]
        let window = window.or_else(|| app.keyWindow().filter(|window| window.windowScene().is_none()));
        let Some(window) = window else {return};
        if window.isHidden() {return}
        let Some(mut presenter) = window.rootViewController() else {return};
        while let Some(presented) = presenter.presentedViewController() {presenter = presented;}
        if presenter.isBeingPresented() || presenter.isBeingDismissed() {
            if let Some(transition) = presenter.transitionCoordinator() {
                let delegate = Weak::new(self);
                let id = self.ivars().current.borrow().as_ref().unwrap().request().id();
                let completion = RcBlock::new(
                    move |_: std::ptr::NonNull<ProtocolObject<dyn UIViewControllerTransitionCoordinatorContext>>| {
                        let Some(delegate) = delegate.load() else {return};
                        let matches = delegate.ivars().current.borrow().as_ref()
                            .is_some_and(|state| state.request().matches(&id));
                        if matches {delegate.show();}
                    },
                );
                transition.animateAlongsideTransition_completion(None, Some(&completion));
            }
            return;
        }
        if presenter.viewIfLoaded().and_then(|view| view.window()).is_none() {return}
        let state = self.ivars().current.borrow_mut().take();
        let request = match state {
            Some(PickState::Waiting(request)) => request,
            Some(PickState::Rejected(request, message)) => {
                let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
                    Some(&NSString::from_str("Unable to Open Selection")),
                    Some(&NSString::from_str(&message)), UIAlertControllerStyle::Alert, self.mtm(),
                );
                for (choice, title, style) in [
                    (0, "Choose Again", UIAlertActionStyle::Default),
                    (1, "Use Valid Items", UIAlertActionStyle::Default),
                    (2, "Cancel", UIAlertActionStyle::Cancel),
                ] {
                    if choice == 1 && !request.has_items() {continue}
                    let dialog = Weak::new(&*alert);
                    let delegate = Weak::new(self);
                    let id = request.id();
                    let handler = RcBlock::new(move |_: std::ptr::NonNull<UIAlertAction>| {
                        let Some(delegate) = delegate.load() else {return};
                        let Some(alert) = dialog.load() else {return};
                        if !delegate.ivars().controller.borrow().as_ref()
                            .is_some_and(|active| std::ptr::eq(&**active, &**alert)) {return}
                        let mut current = delegate.ivars().current.borrow_mut();
                        if !matches!(current.as_ref(),
                            Some(PickState::Alerting(request)) if request.matches(&id)) {return}
                        let Some(PickState::Alerting(request)) = current.take() else {unreachable!()};
                        *current = Some(PickState::Dismissing(request));
                        drop(current);
                        let id = id.clone();
                        let completion = RcBlock::new(move || {
                            let Some(request) = delegate.dismissed(&id) else {return};
                            if request.disconnected() {request.cancel();}
                            else {
                                match choice {
                                    0 => *delegate.ivars().current.borrow_mut() =
                                        Some(PickState::Waiting(request)),
                                    1 => request.finish(),
                                    2 => request.cancel(),
                                    _ => unreachable!(),
                                }
                            }
                            delegate.show();
                        });
                        Self::dismiss(&alert, completion);
                    });
                    alert.addAction(&UIAlertAction::actionWithTitle_style_handler(
                        Some(&NSString::from_str(title)), style, Some(&handler), self.mtm(),
                    ));
                }
                *self.ivars().controller.borrow_mut() = Some(alert.clone().into_super());
                *self.ivars().current.borrow_mut() = Some(PickState::Alerting(request));
                presenter.presentViewController_animated_completion(&alert, true, None);
                return;
            }
            None => return,
            _ => unreachable!("An active picker cannot present another picker"),
        };
        if request.documents() {
            let types = request.file_types();
            let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
                UIDocumentPickerViewController::alloc(self.mtm()),
                &NSArray::from_retained_slice(&types), true,
            );
            picker.setAllowsMultipleSelection(request.multiple());
            picker.setDelegate(Some(ProtocolObject::from_ref(self)));
            self.present(&presenter, &picker, request);
            return;
        }
        let config = request.photos();
        let picker = unsafe {
            PHPickerViewController::init_with_configuration(
                PHPickerViewController::alloc(self.mtm()), &config,
            )
        };
        unsafe {picker.set_delegate(Some(ProtocolObject::from_ref(self)));}
        self.present(&presenter, &picker, request);
    }

    fn present(&self, presenter: &UIViewController, picker: &UIViewController, request: PickRequest) {
        let presentation = picker.presentationController()
            .expect("Picker has no presentation controller");
        unsafe {presentation.setDelegate(Some(ProtocolObject::from_ref(self)));}
        *self.ivars().controller.borrow_mut() = Some(picker.retain());
        *self.ivars().current.borrow_mut() = Some(PickState::Selecting(request));
        presenter.presentViewController_animated_completion(picker, true, None);
    }
}

fn get_key_window() -> Retained<UIWindow> {
    let app = UIApplication::sharedApplication(MainThreadMarker::new().unwrap());
    let scenes: Retained<NSSet<UIScene>> = app.connectedScenes();

    let window = scenes.into_iter().find_map(|scene| {
        if scene.activationState() != UISceneActivationState::ForegroundActive {return None}
        scene.downcast::<UIWindowScene>().ok()?.keyWindow()
    });
    #[allow(deprecated)]
    window.or_else(|| app.keyWindow().filter(|window| window.windowScene().is_none()))
        .expect("No application window is available")
}

fn get_view_controller() -> Retained<UIViewController> {
    let window = get_key_window();
    let mut vc = window.rootViewController().unwrap();
    while let Some(presented) = vc.presentedViewController() {
        vc = presented;
    }
    vc
}

fn image_to_uiimage(image: Image) -> Retained<UIImage> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let bytes_per_row = width * 4;
    let raw = image.as_raw();
    let data_ptr = raw.as_ptr() as *const std::ffi::c_void;
    let data_len = raw.len();

    let info = Arc::into_raw(image) as *mut std::ffi::c_void;

    // Release callback: drop the Arc when CG is done with the data
    unsafe extern "C-unwind" fn release_callback(
        info: *mut std::ffi::c_void,
        _data: std::ptr::NonNull<std::ffi::c_void>,
        _size: usize,
    ) {
        if !info.is_null() {
            // Safety: we created this with Arc::into_raw above
            drop(unsafe { Arc::from_raw(info as *const RgbaImage) });
        }
    }

    let provider = unsafe {CGDataProvider::with_data(info, data_ptr, data_len, Some(release_callback)).unwrap()};
    let color_space = CGColorSpace::new_device_rgb().unwrap();
    // image::Rgba is straight (not premultiplied) RGBA, top row first.
    let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0);

    let cg_image = unsafe {CGImage::new(
        width, height, 8, 32, bytes_per_row,
        Some(&color_space), bitmap_info, Some(&provider),
        std::ptr::null(), true, CGColorRenderingIntent::RenderingIntentDefault,
    ).unwrap() };

    UIImage::imageWithCGImage(&cg_image)
}

pub fn uiimage_to_image(image: &UIImage) -> Option<Image> {
    let cg_image = unsafe {image.CGImage()?};
    let width = CGImage::width(Some(&cg_image)) as u32;
    let height = CGImage::height(Some(&cg_image)) as u32;

    let color_space = CGColorSpace::new_device_rgb().expect("Failed to create RGB color space");
    let row = (width * 4) as usize;
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;

    let context = unsafe {CGBitmapContextCreate(
        std::ptr::null_mut(),
        width as usize,
        height as usize,
        8,
        row,
        Some(&color_space),
        info,
    ).expect("Failed to create image bitmap context")};

    CGContext::draw_image(
        Some(&context),
        CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64)),
        Some(&cg_image),
    );

    let ptr = std::ptr::NonNull::new(
        objc2_core_graphics::CGBitmapContextGetData(Some(&context)) as *mut u8,
    ).expect("Image bitmap context has no pixel data");
    let mut pixels = unsafe {
        std::slice::from_raw_parts(ptr.as_ptr(), row * height as usize)
    }.to_vec();
    unpremultiply(&mut pixels);
    Some(Arc::new(ImageBuffer::from_raw(width, height, pixels)
        .expect("Decoded image dimensions do not match pixel data")))
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

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {}
