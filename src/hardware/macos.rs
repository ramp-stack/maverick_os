use std::sync::Arc;

use arboard::ImageData;
use image::{RgbaImage, Rgba, ImageBuffer};
use crossfire::{spsc, AsyncRx, AsyncTx};

use predator::{Data, Image};

use block2::RcBlock;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, AnyThread};
use objc2::runtime::{ProtocolObject, AnyObject};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSImage, NSSharingServicePicker,
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSViewController
};
use objc2_core_foundation::{CGSize, CGPoint, CGRect};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorSpace, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGDataProvider, CGImage, CGColorRenderingIntent,
    CGBitmapContextGetData, CGBitmapContextCreate, CGContext
};
use objc2_foundation::{NSArray, NSData, NSError, NSItemProvider, NSObject, NSObjectProtocol, NSString, NSRectEdge};
use objc2_photos_ui::{
    PHPickerConfiguration, PHPickerFilter, PHPickerResult, PHPickerViewController,
    PHPickerViewControllerDelegate,
};



pub struct SafeArea;
impl SafeArea {
    pub fn get() -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }
}

pub struct Clipboard(arboard::Clipboard);
impl Clipboard {
    pub fn new() -> Self {Self(arboard::Clipboard::new().unwrap())}

    pub fn set(&mut self, data: Data) {match data {
        Data::Text(text) => self.0.set_text(text).unwrap(),
        Data::Image(img) => self.0.set_image(ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: std::borrow::Cow::Borrowed(img.as_raw()),
        }).unwrap()
    }}

    pub fn get(&mut self) -> Option<Data> {
        self.0.get_text().ok().map(Data::Text).or_else(||
            self.0.get_image().ok().and_then(|i| Some(Data::Image(Arc::new(RgbaImage::from_raw(
                i.width as u32, i.height as u32, i.bytes.into_owned(),
            )?))))
        )
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

pub struct Share;

impl Share {
    pub fn new() -> Self {
        Self
    }

    /// Shows the macOS share picker for a single item.
    /// Automatically anchors to the key window's content view.
    /// Call on the main thread.
    pub fn share(&self, data: Data) {
        let app = NSApplication::sharedApplication(objc2::MainThreadMarker::new().unwrap());

        let window = app.keyWindow().or_else(|| app.mainWindow()).unwrap();
        let view = window.contentView().unwrap();

        let item: Retained<AnyObject> = match data {
            Data::Text(text) => NSString::from_str(&text).into(),
            Data::Image(image) => image_to_nsimage(image).into(),
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

        let vc = content_view_controller();
        let picking = vc.presentedViewControllers().is_some_and(|presented| {
            (0..presented.count()).any(|i| {
                presented
                    .objectAtIndex(i)
                    .isKindOfClass(PHPickerViewController::class())
            })
        });

        if !picking {
            present_picker(&vc, &self.delegate, &self.delegate.ivars().1);
        }
        rx
    }
}

fn content_view_controller() -> Retained<NSViewController> {
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    let window = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .expect("no key window");
    window
        .contentViewController()
        .expect("key window has no contentViewController")
}

fn present_picker(
    vc: &NSViewController,
    delegate: &PhotoPickerDelegate,
    config: &PHPickerConfiguration,
) {
    let picker = unsafe {
        PHPickerViewController::initWithConfiguration(
            PHPickerViewController::alloc(delegate.mtm()),
            config,
        )
    };
    unsafe {
        picker.setDelegate(Some(ProtocolObject::from_ref(delegate)));
    }
    vc.presentViewControllerAsModalWindow(&picker);
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

    unsafe impl PHPickerViewControllerDelegate for PhotoPickerDelegate {
        #[unsafe(method(picker:didFinishPicking:))]
        fn picker_did_finish_picking(
            &self,
            picker: &PHPickerViewController,
            results: &NSArray<PHPickerResult>,
        ) {
            if let Some(parent) = unsafe { picker.presentingViewController() } {
                parent.dismissViewController(picker);
            }

            let tx = self.ivars().0.try_recv().unwrap();
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
                    match NSImage::initWithData(NSImage::alloc(), data) {
                        Some(image) => tx.try_send(Ok(nsimage_to_image(&image))).unwrap(),
                        None => tx.try_send(Err(())).unwrap(),
                    }
                });
                let _progress = unsafe {
                    provider.loadDataRepresentationForTypeIdentifier_completionHandler(&uti, &block)
                };
            }

            if !self.ivars().0.is_empty() {
                present_picker(&content_view_controller(), self, &self.ivars().1);
            }
        }
    }
);























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
    let bitmap_info = CGBitmapInfo(
        CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
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

pub fn nsimage_to_image(image: &NSImage) -> Image {
    let cg = unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }
        .expect("NSImage has no CGImage");

    let width = CGImage::width(Some(&*cg)) as u32;
    let height = CGImage::height(Some(&*cg)) as u32;
    let row = width as usize * 4;

    let color_space = CGColorSpace::new_device_rgb().unwrap();
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
    }
    .unwrap();

    CGContext::translate_ctm(Some(&context), 0.0, height as f64);
    CGContext::scale_ctm(Some(&context), 1.0, -1.0);
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        ),
        Some(&*cg),
    );

    let ptr = CGBitmapContextGetData(Some(&context)) as *const u8;
    let bytes = unsafe { std::slice::from_raw_parts(ptr, row * height as usize) }.to_vec();
    Arc::new(ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bytes).unwrap())
}

#[link(name = "Cocoa", kind = "framework")]
unsafe extern "C" {}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {}
