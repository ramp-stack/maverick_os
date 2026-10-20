use std::ffi::c_void;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use block2::RcBlock;
use objc2::{class, msg_send, sel};
use objc2::declare::ClassBuilder;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2_foundation::{NSArray, NSObject, NSString};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_core_media::CMTime;
use image::RgbaImage;

use super::{Media, MAX_VIDEO_BYTES};

/// A video's still is at most this many pixels on its longest side.
const POSTER_SIDE: f64 = 640.0;
/// A photo picked with `open_media` is at most this many pixels on its
/// longest side (it's made smaller as it loads).
const MAX_PHOTO_SIDE: u32 = 1600;
/// `PHPickerConfigurationAssetRepresentationModeCurrent`.
const ASSET_REPRESENTATION_CURRENT: isize = 1;
/// `AVAssetExportSessionStatusCompleted`.
const EXPORT_COMPLETED: isize = 3;
/// `kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big`: bytes in the
/// order R, G, B, A.
const RGBA_BITMAP: u32 = 1 | (4 << 12);

#[cfg_attr(target_vendor = "apple", link(name = "ImageIO", kind = "framework"))]
unsafe extern "C" {
    fn CGImageSourceCreateWithURL(url: *const c_void, options: *const c_void) -> *mut c_void;
    fn CGImageSourceCreateThumbnailAtIndex(source: *mut c_void, index: usize, options: *const c_void) -> *mut CGImage;
    static kCGImageSourceCreateThumbnailFromImageAlways: *const c_void;
    static kCGImageSourceCreateThumbnailWithTransform: *const c_void;
    static kCGImageSourceThumbnailMaxPixelSize: *const c_void;
    static kCGImageSourceShouldCacheImmediately: *const c_void;
}

#[cfg_attr(target_vendor = "apple", link(name = "CoreGraphics", kind = "framework"))]
unsafe extern "C" {
    fn CGImageGetWidth(image: *const CGImage) -> usize;
    fn CGImageGetHeight(image: *const CGImage) -> usize;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGBitmapContextCreate(data: *mut c_void, width: usize, height: usize, bits_per_component: usize, bytes_per_row: usize, space: *mut c_void, bitmap_info: u32) -> *mut c_void;
    fn CGContextSetRGBFillColor(context: *mut c_void, red: f64, green: f64, blue: f64, alpha: f64);
    fn CGContextFillRect(context: *mut c_void, rect: CGRect);
    fn CGContextDrawImage(context: *mut c_void, rect: CGRect, image: *const CGImage);
    fn CGContextRelease(context: *mut c_void);
    static kCGColorSpaceSRGB: *const c_void;
}

#[cfg_attr(target_vendor = "apple", link(name = "CoreFoundation", kind = "framework"))]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

#[derive(Debug, Clone)]
pub enum ImageOrientation {
    Up,
    Down,
    Left,
    Right,
    UpMirrored,
    DownMirrored,
    LeftMirrored,
    RightMirrored,
}

impl ImageOrientation {
    pub fn from_ios_value(orientation: i64) -> Self {
        match orientation {
            0 => ImageOrientation::Up,
            1 => ImageOrientation::Down,
            2 => ImageOrientation::Left,
            3 => ImageOrientation::Right,
            4 => ImageOrientation::UpMirrored,
            5 => ImageOrientation::DownMirrored,
            6 => ImageOrientation::LeftMirrored,
            7 => ImageOrientation::RightMirrored,
            _ => ImageOrientation::Up,
        }
    }

    pub fn apply_to(&self, image: image::DynamicImage) -> image::DynamicImage {
        match self {
            ImageOrientation::Up => image,
            ImageOrientation::Down => image.rotate180(),
            ImageOrientation::Left => image.rotate270(),
            ImageOrientation::Right => image.rotate90(),
            ImageOrientation::UpMirrored => image.fliph(),
            ImageOrientation::DownMirrored => image.fliph().rotate180(),
            ImageOrientation::LeftMirrored => image.fliph().rotate90(),
            ImageOrientation::RightMirrored => image.fliph().rotate270(),
        }
    }
}

/// What the picker offers.
#[derive(Clone, Copy)]
enum Offer { Photos, PhotosAndVideos }

/// Called (on the main thread) with the picked item's `NSItemProvider`, or
/// null if nothing was picked.
type OnPick = Box<dyn FnOnce(*mut AnyObject) + Send + 'static>;

#[derive(Clone)]
pub struct OsPhotoPicker;

impl OsPhotoPicker {
    pub fn open(callback: impl FnOnce(Option<RgbaImage>) + Send + 'static) {
        present(Offer::Photos, Box::new(move |provider| unsafe {
            if provider.is_null() { return callback(None); }
            load_image(provider, Box::new(callback));
        }));
    }

    /// A photo or a video, made smaller to send as it loads: photos at most
    /// [`MAX_PHOTO_SIDE`], videos at most 960x540 and [`MAX_VIDEO_BYTES`]
    /// (longer ones are cut off there).
    pub fn open_media(callback: impl FnOnce(Option<Media>) + Send + 'static) {
        present(Offer::PhotosAndVideos, Box::new(move |provider| unsafe {
            if provider.is_null() { return callback(None); }
            let done: Box<dyn FnOnce(Option<Media>) + Send> = Box::new(callback);
            let movie = NSString::from_str("public.movie");
            let is_video: bool = msg_send![provider, hasItemConformingToTypeIdentifier: &*movie];
            match is_video {
                true => load_video(provider, done),
                false => load_small_image(provider, MAX_PHOTO_SIDE, Box::new(move |image| done(image.map(Media::Photo)))),
            }
        }));
    }
}

/// Shows the photo library picker.
fn present(offer: Offer, on_pick: OnPick) {
    let callback_box: Box<OnPick> = Box::new(on_pick);
    let callback_ptr = Box::into_raw(callback_box) as usize;

    dispatch2::DispatchQueue::main().exec_async(move || {
        let callback_ptr = callback_ptr as *mut c_void;
        autoreleasepool(|_| unsafe {
            let config_cls = class!(PHPickerConfiguration);
            let config: *mut AnyObject = msg_send![config_cls, new];

            let filter_cls = class!(PHPickerFilter);
            let images_filter: *mut AnyObject = msg_send![filter_cls, imagesFilter];
            let filter: *mut AnyObject = match offer {
                Offer::Photos => images_filter,
                Offer::PhotosAndVideos => {
                    let videos_filter: *mut AnyObject = msg_send![filter_cls, videosFilter];
                    let filters = NSArray::<AnyObject>::from_slice(&[&*images_filter, &*videos_filter]);
                    msg_send![filter_cls, anyFilterMatchingSubfilters: &*filters]
                }
            };
            let _: () = msg_send![config, setFilter: filter];
            if let Offer::PhotosAndVideos = offer {
                // The files as they are, without the system converting them
                // first (slow for videos): they're made smaller to send here.
                let _: () = msg_send![config, setPreferredAssetRepresentationMode: ASSET_REPRESENTATION_CURRENT];
            }

            let picker_cls = class!(PHPickerViewController);
            let picker: *mut AnyObject = msg_send![picker_cls, alloc];
            let picker: *mut AnyObject = msg_send![picker, initWithConfiguration: config];

            let delegate = create_photo_picker_delegate(callback_ptr);
            objc2::ffi::objc_retain(delegate as *mut _);
            let _: () = msg_send![picker, setDelegate: delegate];

            let ui_app = class!(UIApplication);
            let shared_app: *mut AnyObject = msg_send![ui_app, sharedApplication];
            let windows: *mut AnyObject = msg_send![shared_app, windows];
            let window: *mut AnyObject = msg_send![windows, firstObject];
            let root_vc: *mut AnyObject = msg_send![window, rootViewController];

            let null_block: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![
                root_vc,
                presentViewController: picker,
                animated: true,
                completion: null_block,
            ];
        });
    });
}

/// Loads a picked photo, the right way up.
unsafe fn load_image(item_provider: *mut AnyObject, done: Box<dyn FnOnce(Option<RgbaImage>) + Send + 'static>) {
    let uiimage_class = class!(UIImage);
    let can_load: bool = unsafe { msg_send![item_provider, canLoadObjectOfClass: uiimage_class] };
    if !can_load {
        return done(None);
    }

    // Wrap in Option so the Fn closure can take it once
    let done = Mutex::new(Some(done));

    let rc_block = RcBlock::new(move |image_obj: *mut AnyObject, _error: *mut AnyObject| {
        // Take the callback out — subsequent calls (if any) will be no-ops
        let Some(done) = done.lock().unwrap().take() else {
            return;
        };

        let rgba = if !image_obj.is_null() {
            let orientation: i64 = unsafe { msg_send![image_obj, imageOrientation] };
            unsafe { png_representation(image_obj) }
                .and_then(|png| image::load_from_memory(&png).ok())
                .map(|img| ImageOrientation::from_ios_value(orientation).apply_to(img).into_rgba8())
        } else {
            None
        };

        done(rgba);
    });

    let _: *mut AnyObject = unsafe { msg_send![
        item_provider,
        loadObjectOfClass: uiimage_class,
        completionHandler: &*rc_block
    ] };
}

/// Loads a picked photo at most `side` pixels on its longest side, the right
/// way up, over white. The system shrinks it as it reads it, so a big photo
/// loads about as fast as a small one.
unsafe fn load_small_image(item_provider: *mut AnyObject, side: u32, done: Box<dyn FnOnce(Option<RgbaImage>) + Send + 'static>) {
    let image_type = NSString::from_str("public.image");
    // Kept in case it has to be loaded the slow way.
    unsafe { objc2::ffi::objc_retain(item_provider as *mut _) };
    let provider = item_provider as usize;
    let done = Mutex::new(Some(done));

    let rc_block = RcBlock::new(move |url: *mut AnyObject, _error: *mut AnyObject| {
        let Some(done) = done.lock().unwrap().take() else {
            return;
        };
        let provider = provider as *mut AnyObject;
        // The file is only there until this returns.
        match autoreleasepool(|_| unsafe { small_image_at(url, side) }) {
            Some(image) => done(Some(image)),
            // Not something ImageIO reads: the slow way (full size).
            None => unsafe { load_image(provider, done) },
        }
        unsafe { objc2::ffi::objc_release(provider as *mut _) };
    });

    let _: *mut AnyObject = unsafe { msg_send![
        item_provider,
        loadFileRepresentationForTypeIdentifier: &*image_type,
        completionHandler: &*rc_block
    ] };
}

/// The image file at `url`, at most `side` pixels on its longest side, the
/// right way up, over white.
unsafe fn small_image_at(url: *mut AnyObject, side: u32) -> Option<RgbaImage> {
    if url.is_null() { return None; }
    let source = unsafe { CGImageSourceCreateWithURL(url as *const c_void, std::ptr::null()) };
    if source.is_null() { return None; }

    let options: *mut AnyObject = unsafe { msg_send![class!(NSMutableDictionary), dictionary] };
    let yes: *mut AnyObject = unsafe { msg_send![class!(NSNumber), numberWithBool: true] };
    let max_side: *mut AnyObject = unsafe { msg_send![class!(NSNumber), numberWithInteger: side as isize] };
    unsafe {
        // From the photo itself (not the small preview some files carry),
        // turned the way it was taken.
        for (key, value) in [
            (kCGImageSourceCreateThumbnailFromImageAlways, yes),
            (kCGImageSourceCreateThumbnailWithTransform, yes),
            (kCGImageSourceShouldCacheImmediately, yes),
            (kCGImageSourceThumbnailMaxPixelSize, max_side),
        ] {
            let _: () = msg_send![options, setObject: value, forKey: key as *mut AnyObject];
        }
    }

    let image = unsafe { CGImageSourceCreateThumbnailAtIndex(source, 0, options as *const c_void) };
    unsafe { CFRelease(source) };
    let image = NonNull::new(image)?;
    let rgba = unsafe { rgba_over_white(image.as_ptr()) };
    unsafe { CFRelease(image.as_ptr() as *const c_void) };
    rgba
}

/// `image`'s pixels (in sRGB), with any see-through parts over white.
unsafe fn rgba_over_white(image: *const CGImage) -> Option<RgbaImage> {
    let (width, height) = unsafe { (CGImageGetWidth(image), CGImageGetHeight(image)) };
    if width == 0 || height == 0 { return None; }
    let mut pixels = vec![0u8; width * height * 4];

    let space = unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) };
    if space.is_null() { return None; }
    let context = unsafe { CGBitmapContextCreate(pixels.as_mut_ptr() as *mut c_void, width, height, 8, width * 4, space, RGBA_BITMAP) };
    unsafe { CGColorSpaceRelease(space) };
    if context.is_null() { return None; }

    let all = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));
    unsafe {
        CGContextSetRGBFillColor(context, 1.0, 1.0, 1.0, 1.0);
        CGContextFillRect(context, all);
        CGContextDrawImage(context, all, image);
        CGContextRelease(context);
    }
    RgbaImage::from_raw(width as u32, height as u32, pixels)
}

/// Loads a picked video and makes it small enough to send.
unsafe fn load_video(item_provider: *mut AnyObject, done: Box<dyn FnOnce(Option<Media>) + Send + 'static>) {
    let movie = NSString::from_str("public.movie");
    let done = Mutex::new(Some(done));

    let rc_block = RcBlock::new(move |url: *mut AnyObject, _error: *mut AnyObject| {
        let Some(done) = done.lock().unwrap().take() else {
            return;
        };
        // The picker deletes its copy once this returns, so make our own first.
        match unsafe { copy_picked_file(url) } {
            Some(copy) => autoreleasepool(|_| unsafe { export_video(copy, done) }),
            None => done(None),
        }
    });

    let _: *mut AnyObject = unsafe { msg_send![
        item_provider,
        loadFileRepresentationForTypeIdentifier: &*movie,
        completionHandler: &*rc_block
    ] };
}

unsafe fn copy_picked_file(url: *mut AnyObject) -> Option<PathBuf> {
    if url.is_null() { return None; }
    let path: *mut NSString = unsafe { msg_send![url, path] };
    if path.is_null() { return None; }
    let source = PathBuf::from(unsafe { (*path).to_string() });
    let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("mov").to_string();
    let copy = temp_file("picked", &extension);
    std::fs::copy(&source, &copy).ok()?;
    Some(copy)
}

/// Exports `source` as an MP4 of at most 960x540 and [`MAX_VIDEO_BYTES`],
/// with a still from its start, then deletes it.
unsafe fn export_video(source: PathBuf, done: Box<dyn FnOnce(Option<Media>) + Send + 'static>) {
    let fail = |source: &Path, done: Box<dyn FnOnce(Option<Media>) + Send + 'static>| {
        let _ = std::fs::remove_file(source);
        done(None);
    };

    let Some(source_url) = (unsafe { file_url(&source) }) else { return fail(&source, done) };
    let asset: *mut AnyObject = unsafe { msg_send![class!(AVURLAsset), URLAssetWithURL: source_url, options: std::ptr::null_mut::<AnyObject>()] };
    if asset.is_null() { return fail(&source, done); }

    let Some(poster) = (unsafe { poster(asset) }) else { return fail(&source, done) };
    let (Some(preset), Some(file_type)) = (unsafe { av_constant("AVAssetExportPreset960x540") }, unsafe { av_constant("AVFileTypeMPEG4") }) else {
        return fail(&source, done);
    };

    let session: *mut AnyObject = unsafe { msg_send![class!(AVAssetExportSession), exportSessionWithAsset: asset, presetName: preset] };
    if session.is_null() { return fail(&source, done); }

    let output = temp_file("send", "mp4");
    let Some(output_url) = (unsafe { file_url(&output) }) else { return fail(&source, done) };
    unsafe {
        let _: () = msg_send![session, setOutputURL: output_url];
        let _: () = msg_send![session, setOutputFileType: file_type];
        let _: () = msg_send![session, setShouldOptimizeForNetworkUse: true];
        let _: () = msg_send![session, setFileLengthLimit: MAX_VIDEO_BYTES as i64];
        // Kept until the export finishes.
        objc2::ffi::objc_retain(session as *mut _);
    }

    let session_ptr = session as usize;
    let finish = Mutex::new(Some((done, poster)));
    let rc_block = RcBlock::new(move || {
        let Some((done, poster)) = finish.lock().unwrap().take() else {
            return;
        };
        let session = session_ptr as *mut AnyObject;
        let status: isize = unsafe { msg_send![session, status] };
        unsafe { objc2::ffi::objc_release(session as *mut _) };
        let _ = std::fs::remove_file(&source);

        let video = autoreleasepool(|_| {
            let mp4 = (status == EXPORT_COMPLETED).then(|| std::fs::read(&output).ok()).flatten().filter(|mp4| !mp4.is_empty())?;
            // How long it is after the export (which cuts off long videos).
            let duration = unsafe { duration_of(&output) }.unwrap_or_default();
            Some(Media::Video { mp4, poster, duration })
        });
        let _ = std::fs::remove_file(&output);
        done(video);
    });

    let _: () = unsafe { msg_send![session, exportAsynchronouslyWithCompletionHandler: &*rc_block] };
}

/// A still from the start of `asset`, the right way up.
unsafe fn poster(asset: *mut AnyObject) -> Option<RgbaImage> {
    let generator: *mut AnyObject = unsafe { msg_send![class!(AVAssetImageGenerator), assetImageGeneratorWithAsset: asset] };
    if generator.is_null() { return None; }
    unsafe {
        let _: () = msg_send![generator, setAppliesPreferredTrackTransform: true];
        let _: () = msg_send![generator, setMaximumSize: CGSize { width: POSTER_SIDE, height: POSTER_SIDE }];
    }

    let start = unsafe { CMTime::new(0, 600) };
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let image: *mut CGImage = unsafe { msg_send![
        generator,
        copyCGImageAtTime: start,
        actualTime: std::ptr::null_mut::<CMTime>(),
        error: &mut error
    ] };
    // `copy…` hands it over: released when this goes.
    let image = unsafe { CFRetained::from_raw(NonNull::new(image)?) };
    unsafe { rgba_over_white(&*image as *const CGImage) }
}

unsafe fn duration_of(path: &Path) -> Option<Duration> {
    let url = unsafe { file_url(path) }?;
    let asset: *mut AnyObject = unsafe { msg_send![class!(AVURLAsset), URLAssetWithURL: url, options: std::ptr::null_mut::<AnyObject>()] };
    if asset.is_null() { return None; }
    let duration: CMTime = unsafe { msg_send![asset, duration] };
    let seconds = unsafe { duration.seconds() };
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// A `UIImage` as PNG bytes.
unsafe fn png_representation(uiimage: *mut AnyObject) -> Option<Vec<u8>> {
    let symbol_name = CString::new("UIImagePNGRepresentation").unwrap();
    let func_ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol_name.as_ptr()) };
    if func_ptr.is_null() { return None; }
    let uiimage_png_rep_fn: extern "C" fn(*mut AnyObject) -> *mut AnyObject = unsafe { std::mem::transmute(func_ptr) };
    let nsdata: *mut AnyObject = uiimage_png_rep_fn(uiimage);
    if nsdata.is_null() { return None; }
    let bytes_ptr: *const c_void = unsafe { msg_send![nsdata, bytes] };
    let length: usize = unsafe { msg_send![nsdata, length] };
    Some(unsafe { std::slice::from_raw_parts(bytes_ptr as *const u8, length) }.to_vec())
}

/// One of AVFoundation's string constants (`AVFileTypeMPEG4`, say).
unsafe fn av_constant(name: &str) -> Option<*mut AnyObject> {
    let symbol = CString::new(name).ok()?;
    let ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr()) } as *const *mut AnyObject;
    if ptr.is_null() { return None; }
    let value = unsafe { *ptr };
    (!value.is_null()).then_some(value)
}

/// A file URL for `path`.
unsafe fn file_url(path: &Path) -> Option<*mut AnyObject> {
    let path = NSString::from_str(path.to_str()?);
    let url: *mut AnyObject = unsafe { msg_send![class!(NSURL), fileURLWithPath: &*path] };
    (!url.is_null()).then_some(url)
}

/// A new file name in the app's temporary folder.
fn temp_file(stem: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    std::env::temp_dir().join(format!("{stem}-{nanos}.{extension}"))
}

fn create_photo_picker_delegate(callback_ptr: *mut c_void) -> *mut AnyObject {
    static DELEGATE_CLASS: OnceLock<usize> = OnceLock::new();

    unsafe {
        let cls = *DELEGATE_CLASS.get_or_init(|| {
            let superclass = class!(NSObject);
            let name = c"RustPHPickerDelegate";
            let mut decl = ClassBuilder::new(name, superclass).unwrap();

            decl.add_ivar::<*mut c_void>(c"rustCallbackPtr");

            extern "C" fn picker_did_finish_picking(
                this: &AnyObject,
                _cmd: Sel,
                picker: *mut AnyObject,
                results: *mut AnyObject,
            ) {
                unsafe {
                    let null_block: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: null_block];

                    objc2::ffi::objc_release(this as *const _ as *mut _);

                    let ivar_name = c"rustCallbackPtr";
                    let ivar = this.class().instance_variable(ivar_name).unwrap();
                    let callback_ptr = *ivar.load::<*mut c_void>(this);

                    if callback_ptr.is_null() {
                        return;
                    }

                    let callback_box: Box<OnPick> = Box::from_raw(callback_ptr as *mut _);

                    let results_array: &NSArray<NSObject> = &*(results as *const NSArray<NSObject>);
                    if results_array.count() == 0 {
                        callback_box(std::ptr::null_mut());
                        return;
                    }

                    let result: *mut NSObject = msg_send![results_array, objectAtIndex: 0usize];
                    let item_provider: *mut AnyObject = msg_send![result, itemProvider];
                    callback_box(item_provider);
                }
            }

            decl.add_method(
                sel!(picker:didFinishPicking:),
                picker_did_finish_picking as extern "C" fn(&'static AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );

            // Fix: double-cast through raw pointer to satisfy the compiler
            decl.register() as *const AnyClass as usize
        }) as *const AnyClass;

        let delegate: *mut AnyObject = msg_send![cls, new];
        let ivar_name = c"rustCallbackPtr";
        let ivar = (*cls).instance_variable(ivar_name).unwrap();
        let ivar_ref: &mut *mut c_void = ivar.load_mut(&mut *delegate);
        *ivar_ref = callback_ptr;

        delegate
    }
}
