use std::sync::Arc;

use proscenium::{Data, Image};
use image::{ImageBuffer, RgbaImage};
use crossfire::{spsc, AsyncRx, AsyncTx};

use block2::RcBlock;
use objc2::{
    MainThreadOnly, MainThreadMarker, AnyThread, extern_class, extern_methods,
    define_class, msg_send, ClassType, DefinedClass
};
use objc2_foundation::{NSString, NSArray, NSSet, NSData, NSError, NSItemProvider, NSObject, NSObjectProtocol};
use objc2_core_foundation::{CGPoint, CGSize, CGRect};
use objc2::runtime::ProtocolObject;
use objc2::rc::{Retained, Allocated};
use objc2_ui_kit::{
    UIPasteboard, UIApplication, UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UIImage,
    UIViewController, UIWindow, UIWindowScene, UISceneActivationState, UIScene,
    UIActivityViewController, UIResponder
};
use objc2_core_graphics::{
    CGImage, CGColorRenderingIntent, CGBitmapInfo, CGColorSpace, CGDataProvider,
    CGImageAlphaInfo, CGContext, CGBitmapContextCreate, CGImageByteOrderInfo
};
use objc2_photos_ui::{
    PHPickerConfiguration, PHPickerFilter, PHPickerResult, PHPickerViewControllerDelegate,
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
            self.0.string().map(|s| Data::Text(s.to_string())).or_else(||
                self.0.image().map(|i| Data::Image(uiimage_to_image(&i)))
            )
        }
    }

    pub fn set(&self, data: Data) {
        unsafe {
            match data {
                Data::Text(text) => {
                    let ns_string = NSString::from_str(&text);
                    self.0.setString(Some(&ns_string));
                },
                Data::Image(image) => {
                    self.0.setImage(Some(&image_to_uiimage(image)));
                }
            }
        }
    }
}

pub struct Haptics(Retained<UIImpactFeedbackGenerator>);
impl Haptics {
    pub fn new() -> Self {
        let key_window = get_key_window();
        let root_vc: Retained<UIViewController> = key_window.rootViewController().unwrap();
            
        let generator = UIImpactFeedbackGenerator::feedbackGeneratorWithStyle_forView(
            UIImpactFeedbackStyle::Medium,
            &root_vc.view().unwrap(),
        );
        generator.prepare();
        
        Self(generator)
    }

    pub fn trigger(&self) { self.0.impactOccurred()}
}

pub struct Share();
impl Share {
    pub fn new() -> Self {Self()}

    /// Opens the system share sheet for a single item.
    /// Must be called on the main thread.
    pub fn share(&self, data: Data) {
        let item: Retained<_> = match data {
            Data::Text(text) => NSString::from_str(&text).into_super(),
            Data::Image(image) => image_to_uiimage(image).into_super(),
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

type Callback = AsyncTx<spsc::One<Result<Image, ()>>>;

pub struct PhotoPicker {
    delegate: Retained<PhotoPickerDelegate>,
    tx: AsyncTx<spsc::List<Callback>>,
}

impl PhotoPicker {
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("main thread");
        let config = unsafe { PHPickerConfiguration::new() };
        unsafe {
            config.setFilter(Some(&PHPickerFilter::imagesFilter()));
            config.setSelectionLimit(1);
        }

        let (tx, rx) = spsc::build(spsc::List::new());
        let delegate = PhotoPickerDelegate::alloc(mtm)
            .set_ivars(PhotoPickerDelegateIvars(rx, config));
        let delegate: Retained<PhotoPickerDelegate> =
            unsafe { msg_send![super(delegate), init] };

        Self { delegate, tx }
    }

    pub fn pick_photo(&self) -> AsyncRx<spsc::One<Result<Image, ()>>> {
        let (tx, rx) = spsc::build(spsc::One::new());
        self.tx.try_send(tx).unwrap();

        let window = get_key_window();
        let mut picking = false;
        if let Some(mut vc) = window.rootViewController() {
            loop {
                if vc.isKindOfClass(PHPickerViewController::class()) {
                    picking = true;
                    break;
                }
                match vc.presentedViewController() {
                    Some(next) => vc = next,
                    None => break,
                }
            }
        }

        if !picking {
            let picker = unsafe {
                PHPickerViewController::init_with_configuration(
                    PHPickerViewController::alloc(self.delegate.mtm()),
                    &self.delegate.ivars().1,
                )
            };
            unsafe {
                picker.set_delegate(Some(ProtocolObject::from_ref(&*self.delegate)));
                get_view_controller()
                    .presentViewController_animated_completion(&picker, true, None);
            }
        }
        rx
    }
}

pub struct PhotoPickerDelegateIvars(
    AsyncRx<spsc::List<Callback>>,
    Retained<PHPickerConfiguration>,
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PhotoPickerDelegateIvars]
    struct PhotoPickerDelegate;

    unsafe impl NSObjectProtocol for PhotoPickerDelegate {}

    // Trait has no methods on iOS in objc2-photos-ui 0.3.2. Empty impl still conforms.
    unsafe impl PHPickerViewControllerDelegate for PhotoPickerDelegate {}

    impl PhotoPickerDelegate {
        #[unsafe(method(picker:didFinishPicking:))]
        fn picker_did_finish_picking(
            &self,
            picker: &PHPickerViewController,
            results: &NSArray<PHPickerResult>,
        ) {
            let tx = self.ivars().0.try_recv().unwrap();
            picker.dismissViewControllerAnimated_completion(true, None);

            if results.count() == 0 {
                tx.try_send(Err(())).unwrap();
            } else {
                let result = results.objectAtIndex(0);
                let provider: Retained<NSItemProvider> = unsafe { result.itemProvider() };
                let uti = NSString::from_str("public.image");
                let block = RcBlock::new(move |data: *mut NSData, _err: *mut NSError| {
                    if data.is_null() {
                        tx.try_send(Err(())).unwrap();
                        return;
                    }
                    let data = unsafe { &*data };
                    let ui = UIImage::initWithData(UIImage::alloc(), data);
                    match ui {
                        Some(ui) => tx.try_send(Ok(uiimage_to_image(&ui))).unwrap(),
                        None => tx.try_send(Err(())).unwrap(),
                    }
                });
                
                let _progress = unsafe {
                    provider
                        .loadDataRepresentationForTypeIdentifier_completionHandler(&uti, &block)
                };
            }

            if !self.ivars().0.is_empty() {
                let next = unsafe {
                    PHPickerViewController::init_with_configuration(
                        PHPickerViewController::alloc(self.mtm()),
                        &self.ivars().1,
                    )
                };
                unsafe {
                    next.set_delegate(Some(ProtocolObject::from_ref(self)));
                    get_view_controller()
                        .presentViewController_animated_completion(&next, true, None);
                }
            }
        }
    }
);

fn get_key_window() -> Retained<UIWindow> {
    let app = UIApplication::sharedApplication(MainThreadMarker::new().unwrap());
    let scenes: Retained<NSSet<UIScene>> = app.connectedScenes();

    let window_scene = scenes.into_iter().find_map(|scene| {
        if scene.activationState() != UISceneActivationState::ForegroundActive {
            return None;
        }
        scene.downcast::<UIWindowScene>().ok()
    }).unwrap();

    window_scene.keyWindow().unwrap()
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
    let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0);

    let cg_image = unsafe {CGImage::new(
        width, height, 8, 32, bytes_per_row,
        Some(&color_space), bitmap_info, Some(&provider),
        std::ptr::null(), true, CGColorRenderingIntent::RenderingIntentDefault,
    ).unwrap() };

    UIImage::imageWithCGImage(&cg_image)
}

fn uiimage_to_image(image: &UIImage) -> Image {
    let cg_image = unsafe {image.CGImage().unwrap()};
    let width = CGImage::width(Some(&cg_image)) as u32;
    let height = CGImage::height(Some(&cg_image)) as u32;

    let color_space = CGColorSpace::new_device_rgb().unwrap();

    let mut pixels = vec![0u8; (width * height * 4) as usize];

    let context = unsafe {CGBitmapContextCreate(
        pixels.as_mut_ptr() as *mut _,
        width as usize,
        height as usize,
        8,
        (width * 4) as usize,
        Some(&color_space),
        CGImageAlphaInfo::PremultipliedLast.0
    ).unwrap()};

    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64),
    );

    CGContext::draw_image(Some(&context), rect, Some(&cg_image));
    Arc::new(ImageBuffer::from_raw(width, height, pixels).unwrap())
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {}
