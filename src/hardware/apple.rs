use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::rc::Rc;
use std::ffi::CStr;
use std::path::PathBuf;

use arc_swap::ArcSwap;
use block2::RcBlock;
use crossfire::{spsc, AsyncRx, AsyncTx, TryRecvError};
use super::{Cancled, Data, DataPicker, Image, Media, PickResult, Video};
use image::{ImageBuffer, RgbaImage};

use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchQoS, GlobalQueueIdentifier};

use objc2::{AnyThread, Message};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{ProtocolObject, AnyObject, AnyClass};
use objc2::{define_class, msg_send, DefinedClass};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput,
    AVCaptureDevicePosition, AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVMediaTypeVideo, AVCaptureVideoDataOutputSampleBufferDelegate,
    AVCaptureSessionPresetHigh, AVCaptureDeviceRotationCoordinator,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelBufferPixelFormatTypeKey,
    CVPixelBuffer, CVPixelBufferLockFlags, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferUnlockBaseAddress,
};
use objc2_uniform_type_identifiers::{
    UTType, UTTypeMovie, UTTypeQuickTimeMovie, UTTypeMPEG4Movie, UTTypeAVI, UTTypeMPEG,
    UTTypeMPEG2TransportStream, UTTypeImage, UTTypeVideo, UTTypeData,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL,
    NSURLContentTypeKey,
};

use objc2_photos_ui::{PHPickerConfiguration, PHPickerFilter, PHPickerResult};

pub enum PickState {
    Waiting(PickRequest),
    Selecting(PickRequest),
    Dismissing(PickRequest),
    Loading(PickRequest),
    Rejected(PickRequest, String),
    Alerting(PickRequest),
}

impl PickState {
    pub fn request(&self) -> &PickRequest {
        match self {
            Self::Waiting(request) | Self::Selecting(request) | Self::Dismissing(request)
                | Self::Loading(request) | Self::Rejected(request, _) | Self::Alerting(request) => request,
        }
    }
}

const NS_APPLICATION_SUPPORT_DIRECTORY: u64 = 14;
const NS_USER_DOMAIN_MASK: u64 = 1;

pub trait Pickable: Clone {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest;
    fn from_data(items: Vec<Data>) -> Self;
}

impl Pickable for Data {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Data, tx)
    }
    fn from_data(mut items: Vec<Data>) -> Self {
        assert_eq!(items.len(), 1, "A single-item picker must return one item");
        items.pop().unwrap()
    }
}

impl Pickable for Image {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Image, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::Image(image) => image,
            _ => unreachable!("The photo picker returned a different data type"),
        }
    }
}

impl Pickable for Video {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Video, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::Video(video) => video,
            _ => unreachable!("The video picker returned a different data type"),
        }
    }
}

impl Pickable for Media {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Media, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::Image(image) => Media::Image(image),
            Data::Video(video) => Media::Video(video),
            _ => unreachable!("The media picker returned a different data type"),
        }
    }
}

impl Pickable for PathBuf {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::File, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::File(path) => path,
            _ => unreachable!("The file picker returned a different data type"),
        }
    }
}

impl<T: Pickable> Pickable for Vec<T> {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        let mut request = T::request(tx);
        request.multiple = true;
        request
    }
    fn from_data(items: Vec<Data>) -> Self {
        items.into_iter().map(|item| T::from_data(vec![item])).collect()
    }
}

pub struct Picked<T: Pickable> {
    rx: Option<AsyncRx<spsc::One<PickResult>>>,
    item: Option<Result<T, Cancled>>,
    picker: Rc<DataPicker>,
}

impl<T: Pickable> Picked<T> {
    pub fn new(rx: AsyncRx<spsc::One<PickResult>>, picker: Rc<DataPicker>) -> Self {
        Self{rx: Some(rx), item: None, picker}
    }

    pub async fn get(mut self) -> Result<T, Cancled> {
        if let Some(item) = self.item.take() {return item}
        self.rx.as_ref().unwrap().recv().await.unwrap_or(Err(Cancled)).map(T::from_data)
    }

    pub fn try_get(&mut self) -> Option<Result<T, Cancled>> {
        if let Some(item) = &self.item {return Some(item.clone())}
        let item = match self.rx.as_ref().unwrap().try_recv() {
            Ok(items) => items.map(T::from_data),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(Cancled),
        };
        self.item = Some(item.clone());
        Some(item)
    }
}

impl<T: Pickable> Drop for Picked<T> {
    fn drop(&mut self) {
        drop(self.rx.take());
        self.picker.cancel();
    }
}

pub struct Logger;
impl Logger {
    pub fn start (level: Option<log::Level>) {
        let level = level.unwrap_or(log::Level::Info);
        env_logger::builder().filter_level(level.to_level_filter()).init();
    }
}

pub struct ApplicationSupport;
impl ApplicationSupport {
    pub fn get(name: String) -> PathBuf {
        unsafe {
            let file_manager_class = AnyClass::get(c"NSFileManager").unwrap();
            let file_manager: *mut AnyObject = msg_send![file_manager_class, defaultManager];

            let mut error: *mut AnyObject = std::ptr::null_mut();
            let url: *mut NSURL = msg_send![
                file_manager,
                URLForDirectory: NS_APPLICATION_SUPPORT_DIRECTORY,
                inDomain: NS_USER_DOMAIN_MASK,
                appropriateForURL: std::ptr::null::<AnyObject>(),
                create: true,
                error: &mut error,
            ];

            if url.is_null() {panic!("Failed to get Application Support directory URL");}

            let path_nsstring: *mut NSString = msg_send![url, path];
            if path_nsstring.is_null() {panic!("Failed to get path from Application Support URL");}

            let c_str: *const std::os::raw::c_char = msg_send![path_nsstring, UTF8String];
            if c_str.is_null() {panic!("Failed to get UTF8 string from Application Support path");}

            let path = CStr::from_ptr(c_str).to_string_lossy().into_owned();
            PathBuf::from(path).join(name)
        }
    }
}

pub type Frames = Arc<ArcSwap<RgbaImage>>;
pub type Session = Rc<Retained<AVCaptureSession>>;

pub struct CameraHandle {
    session: Session,
    frames: Frames,
    _delegate: Retained<CameraDelegate>,
    _rotation_coordinator: Retained<AVCaptureDeviceRotationCoordinator>,
}

impl CameraHandle {
    pub fn new() -> Self {
        let frames: Frames = Arc::new(ArcSwap::from_pointee(RgbaImage::new(1, 1)));
        let session = unsafe { AVCaptureSession::new() };

        let device = unsafe {
            AVCaptureDevice::defaultDeviceWithDeviceType_mediaType_position(
                objc2_av_foundation::AVCaptureDeviceTypeBuiltInWideAngleCamera,
                AVMediaTypeVideo,
                AVCaptureDevicePosition::Unspecified,
            )
        }.unwrap();

        let rotation_coordinator = unsafe {
            AVCaptureDeviceRotationCoordinator::initWithDevice_previewLayer(
                AVCaptureDeviceRotationCoordinator::alloc(),
                &device, None,
            )
        };

        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device).unwrap() };
        let output = unsafe { AVCaptureVideoDataOutput::new() };

        // Native biplanar YUV (video range). Far cheaper than BGRA.
        let key: &NSString = unsafe {
            &*(kCVPixelBufferPixelFormatTypeKey as *const objc2_core_foundation::CFString as *const NSString)
        };
        let val = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        let settings = NSDictionary::from_slices::<NSString>(&[key], &[val.as_ref()]);
        unsafe { output.setVideoSettings(Some(&settings)) };

        let queue = DispatchQueue::new("camera.frame.queue", DispatchQueueAttr::SERIAL);
        let delegate = CameraDelegate::new(Arc::clone(&frames));
        let proto = ProtocolObject::from_ref(&*delegate);
        unsafe { output.setSampleBufferDelegate_queue(Some(proto), Some(&queue)) };

        unsafe {
            session.beginConfiguration();
            if session.canSetSessionPreset(AVCaptureSessionPresetHigh) {
                session.setSessionPreset(AVCaptureSessionPresetHigh);
            }
            if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                session.commitConfiguration();
                panic!("Can't add Input || Can't add Output");
            }
            session.addInput(&input);
            session.addOutput(&output);

            if let Some(media_type) = AVMediaTypeVideo
            && let Some(conn) = output.connectionWithMediaType(media_type) {
                let angle = rotation_coordinator.videoRotationAngleForHorizonLevelCapture();
                if conn.isVideoRotationAngleSupported(angle) {
                    conn.setVideoRotationAngle(angle);
                }
            }
            

            session.commitConfiguration();
        }

        CameraHandle {
            session: Rc::new(session),
            frames,
            _delegate: delegate,
            _rotation_coordinator: rotation_coordinator
        }
    }

    pub fn start(&self) -> Camera {
        if !unsafe {self.session.isRunning()} {
            unsafe { self.session.startRunning(); }
        }
        Camera{session: Rc::clone(&self.session), frames: Arc::clone(&self.frames)}
    }
}

impl Drop for CameraHandle {
    fn drop(&mut self) {
        unsafe {
            if self.session.isRunning() {self.session.stopRunning();}
        }
    }
}

#[derive(Clone)]
pub struct Camera {
    session: Session,
    frames: Frames
}

impl Camera {
    pub fn frame(&self) -> Arc<RgbaImage> {
        self.frames.load_full()
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            // CameraHandle and the capture delegate each keep a frames Arc.
            // strong_count still includes this Camera, so 3 means it is the last one.
            if self.session.isRunning() && Arc::strong_count(&self.frames) == 3 {
                self.session.stopRunning();
            }
        }
    }
}

#[link(name = "PhotosUI", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AVKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[link(name = "c++")]
unsafe extern "C" {}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

pub struct MovieType;

impl MovieType {
    pub fn get(video: &super::Video) -> Retained<UTType> {
        let extension = match video.format().split(',').next().unwrap() {
            "mov" => {
                let mut atoms = video.bytes().as_slice();
                let mut legacy = false;
                loop {
                    let Some(header) = atoms.get(..8) else {
                        break (atoms.is_empty() && legacy).then_some("mov");
                    };
                    let size = u32::from_be_bytes(header[..4].try_into().unwrap());
                    let (size, start) = match size {
                        0 => (atoms.len(), 8),
                        1 => {
                            let Some(size) = atoms.get(8..16) else {break None};
                            let size = u64::from_be_bytes(size.try_into().unwrap());
                            let Ok(size) = usize::try_from(size) else {break None};
                            (size, 16)
                        }
                        size => (size as usize, 8),
                    };
                    if size < start {break None}
                    let Some(atom) = atoms.get(..size) else {break None};
                    if &header[4..8] == b"ftyp" {
                        // The major brand identifies the container; unknown brands stay generic.
                        break match atom.get(start..start + 4) {
                            Some(b"qt  ") => Some("mov"),
                            Some(b"isom" | b"iso2" | b"iso3" | b"iso4" | b"iso5" | b"iso6"
                                | b"iso7" | b"iso8" | b"iso9" | b"mp41" | b"mp42" | b"avc1"
                                | b"M4V " | b"M4VH" | b"M4VP" | b"MSNV" | b"F4V " | b"dash") => Some("mp4"),
                            Some(brand) if brand.starts_with(b"3gp") => Some("3gp"),
                            Some(brand) if brand.starts_with(b"3g2") => Some("3g2"),
                            Some(b"mjp2" | b"mj2s") => Some("mj2"),
                            _ => None,
                        };
                    }
                    legacy |= &header[4..8] == b"moov";
                    atoms = &atoms[size..];
                }
            }
            "avi" => Some("avi"),
            "mpeg" => Some("mpg"),
            "mpegts" => Some("ts"),
            "flv" => Some("flv"),
            "ogg" => Some("ogv"),
            "asf" => Some("asf"),
            _ => None,
        };
        let known = unsafe {match extension {
            Some("mov") => Some(UTTypeQuickTimeMovie),
            Some("mp4") => Some(UTTypeMPEG4Movie),
            Some("avi") => Some(UTTypeAVI),
            Some("mpg") => Some(UTTypeMPEG),
            Some("ts") => Some(UTTypeMPEG2TransportStream),
            _ => None,
        }};
        if let Some(ty) = known {return ty.retain()}
        let movie = unsafe {UTTypeMovie};
        extension.and_then(|ext| UTType::typeWithFilenameExtension_conformingToType(
            &NSString::from_str(ext), movie,
        )).unwrap_or_else(|| movie.retain())
    }

    pub fn matches(identifier: &NSString) -> bool {
        let known = unsafe {[
            UTTypeMovie, UTTypeQuickTimeMovie, UTTypeMPEG4Movie, UTTypeAVI,
            UTTypeMPEG, UTTypeMPEG2TransportStream,
        ]};
        known.iter().any(|ty| &*ty.identifier() == identifier)
            || UTType::typeWithIdentifier(identifier)
                .is_some_and(|ty| ty.conformsToType(unsafe {UTTypeMovie}))
    }
}

pub struct ShareMovie {
    path: RefCell<Option<PathBuf>>,
}

impl ShareMovie {
    pub fn new() -> Self {ShareMovie{path: RefCell::new(None)}}

    pub fn path(&self, video: &super::Video) -> Option<PathBuf> {
        let mut path = std::env::temp_dir().join(format!(
            "maverick-{}-{}", std::process::id(), rand::random::<u64>(),
        ));
        if let Some(ext) = MovieType::get(video).preferredFilenameExtension() {
            path.set_extension(ext.to_string());
        }
        std::fs::write(&path, video.bytes().as_slice()).ok()?;
        if let Some(old) = self.path.borrow_mut().replace(path.clone()) {
            let _ = std::fs::remove_file(old);
        }
        Some(path)
    }
}

impl Drop for ShareMovie {
    fn drop(&mut self) {
        if let Some(path) = self.path.borrow_mut().take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub struct PickRequest {
    id: Arc<()>,
    tx: AsyncTx<spsc::One<PickResult>>,
    kind: PickKind,
    multiple: bool,
    items: Vec<Data>,
}

impl PickRequest {
    fn new(kind: PickKind, tx: AsyncTx<spsc::One<PickResult>>) -> Self {
        PickRequest{id: Arc::new(()), tx, kind, multiple: false, items: Vec::new()}
    }

    pub fn id(&self) -> Arc<()> {self.id.clone()}
    pub fn matches(&self, id: &Arc<()>) -> bool {Arc::ptr_eq(&self.id, id)}
    pub fn disconnected(&self) -> bool {self.tx.is_disconnected()}

    pub fn finish(self) {let _ = self.tx.try_send(Ok(self.items));}
    pub fn cancel(self) {let _ = self.tx.try_send(Err(Cancled));}
    pub fn has_items(&self) -> bool {!self.items.is_empty()}

    pub fn accept(&mut self, items: Vec<Result<Data, String>>) -> Option<String> {
        let mut errors = Vec::new();
        for item in items {
            match item {
                Ok(item) => self.items.push(item),
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {return None}
        let count = errors.len();
        let mut message = format!("{count} selected item{} could not be opened.\n\n{}",
            if count == 1 {""} else {"s"}, errors.join("\n"));
        if self.has_items() {
            let count = self.items.len();
            message.push_str(&format!(
                "\n\n{count} valid item{} kept. Choose replacements, use the valid items, or cancel.",
                if count == 1 {" is"} else {"s are"},
            ));
        } else {
            message.push_str("\n\nChoose another selection or cancel.");
        }
        Some(message)
    }

    pub fn multiple(&self) -> bool {self.multiple}
    pub fn documents(&self) -> bool {matches!(self.kind, PickKind::File | PickKind::Data)}

    pub fn photos(&self) -> Retained<PHPickerConfiguration> {
        let config = unsafe {PHPickerConfiguration::new()};
        unsafe {
            let filter = match self.kind {
                PickKind::Image => PHPickerFilter::imagesFilter(),
                PickKind::Video => PHPickerFilter::videosFilter(),
                PickKind::Media | PickKind::Data => {
                    let filters = NSArray::from_retained_slice(&[
                        PHPickerFilter::imagesFilter(), PHPickerFilter::videosFilter(),
                    ]);
                    PHPickerFilter::anyFilterMatchingSubfilters(&filters)
                }
                PickKind::File => unreachable!("File selections use the document picker"),
            };
            config.setFilter(Some(&filter));
            config.setSelectionLimit(if self.multiple {0} else {1});
        }
        config
    }

    pub fn file_types(&self) -> Vec<Retained<UTType>> {
        unsafe {match self.kind {
            PickKind::Image => vec![UTTypeImage.retain()],
            PickKind::Video => vec![UTTypeMovie.retain()],
            PickKind::Media => vec![UTTypeImage.retain(), UTTypeMovie.retain()],
            PickKind::File | PickKind::Data => vec![UTTypeData.retain()],
        }}
    }

    fn format(url: &NSURL) -> Result<PickFormat, String> {
        let mut value = None;
        unsafe {url.getResourceValue_forKey_error(&mut value, NSURLContentTypeKey)}
            .map_err(|error| format!("Could not read the file type: {}", error.localizedDescription()))?;
        let ty = value.map(|value| value.downcast::<UTType>()
            .expect("Apple returned a non-UTType content type")).or_else(|| {
            let path = PathBuf::from(url.path().expect("The selected file has no path").to_string());
            path.extension().and_then(|ext| ext.to_str())
                .and_then(|ext| UTType::typeWithFilenameExtension(&NSString::from_str(ext)))
        });
        if let Some(ty) = ty {
            if ty.conformsToType(unsafe {UTTypeImage}) {return Ok(PickFormat::Photo)}
            if ty.conformsToType(unsafe {UTTypeMovie}) || ty.conformsToType(unsafe {UTTypeVideo}) {
                return Ok(PickFormat::Video);
            }
        }
        Ok(PickFormat::File)
    }

    fn image(data: &NSData) -> Result<Image, String> {
        #[cfg(target_os = "macos")]
        let image = objc2_app_kit::NSImage::initWithData(objc2_app_kit::NSImage::alloc(), data)
            .and_then(|image| super::macos::nsimage_to_image(&image));
        #[cfg(target_os = "ios")]
        let image = objc2_ui_kit::UIImage::initWithData(objc2_ui_kit::UIImage::alloc(), data)
            .and_then(|image| super::ios::uiimage_to_image(&image));
        image.ok_or_else(|| "The image could not be decoded.".into())
    }

    fn load_file(kind: PickKind, url: &NSURL) -> Result<Data, String> {
        let path = PathBuf::from(url.path().expect("The selected file has no path").to_string());
        if matches!(kind, PickKind::File) {return Ok(Data::File(path))}
        let access = unsafe {url.startAccessingSecurityScopedResource()};
        let item = (|| {
            let format = Self::format(url)?;
            let decode = match format {
                PickFormat::Photo => kind.image(),
                PickFormat::Video => kind.video(),
                PickFormat::File => false,
            };
            if !decode {return Ok(Data::File(path.clone()))}
            let bytes = std::fs::read(&path).map_err(|error| format!("Could not read file: {error}"))?;
            match format {
                PickFormat::Photo => Self::image(&NSData::with_bytes(&bytes)).map(Data::Image),
                PickFormat::Video => Video::new(bytes).map(Data::Video)
                    .ok_or_else(|| "The video could not be decoded.".into()),
                PickFormat::File => unreachable!(),
            }
        })();
        if access {unsafe {url.stopAccessingSecurityScopedResource();}}
        item.map_err(|error| {
            let name = path.file_name().unwrap().to_string_lossy();
            format!("{name}: {error}")
        })
    }

    pub fn load_files<F: FnOnce(Vec<Result<Data, String>>) + Send + 'static>(&self, urls: &NSArray<NSURL>, completion: F) {
        let kind = self.kind;
        let urls: Vec<_> = urls.iter().collect();
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
            DispatchQoS::UserInitiated,
        ));
        queue.exec_async(move || autoreleasepool(|_| {
            let items = urls.iter().map(|url| Self::load_file(kind, &url)).collect();
            DispatchQueue::main().exec_async(move || completion(items));
        }));
    }

    pub fn load_media<F: FnOnce(Vec<Result<Data, String>>) + Send + 'static>(&self, results: &NSArray<PHPickerResult>, completion: F) {
        let count = results.count();
        let gather = PickLoad::new(completion, count);
        let movie_id = unsafe {UTTypeMovie}.identifier();
        let image_id = unsafe {UTTypeImage}.identifier();
        for i in 0..count {
            let provider = unsafe {results.objectAtIndex(i).itemProvider()};
            let name = provider.suggestedName().map(|name| name.to_string());
            if self.kind.video() && provider.hasItemConformingToTypeIdentifier(&movie_id) {
                let gather = gather.clone();
                let block = RcBlock::new(move |url: *mut NSURL, error: *mut NSError| {
                    let item = if let Some(error) = unsafe {error.as_ref()} {
                        log::warn!("Video provider failed: {error:?}");
                        Err(format!("Could not load the video: {}", error.localizedDescription()))
                    } else {
                        let url = unsafe {url.as_ref()}.expect("Apple returned a video without a file");
                        let path = url.path().expect("Apple returned a video file without a path");
                        std::fs::read(path.to_string())
                            .map_err(|error| format!("Could not read the video: {error}"))
                            .and_then(|bytes| Video::new(bytes).map(Data::Video)
                                .ok_or_else(|| "The video could not be decoded.".into()))
                    };
                    let item = item.map_err(|error| match &name {
                        Some(name) => format!("{name}: {error}"),
                        None => error,
                    });
                    gather.lock().unwrap().done(i, item);
                });
                let _progress = unsafe {
                    provider.loadFileRepresentationForTypeIdentifier_completionHandler(&movie_id, &block)
                };
            } else if self.kind.image() && provider.hasItemConformingToTypeIdentifier(&image_id) {
                let gather = gather.clone();
                let block = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
                    let item = if let Some(error) = unsafe {error.as_ref()} {
                        log::warn!("Image provider failed: {error:?}");
                        Err(format!("Could not load the image: {}", error.localizedDescription()))
                    } else {
                        let data = unsafe {data.as_ref()}.expect("Apple returned an image without data");
                        Self::image(data).map(Data::Image)
                    };
                    let item = item.map_err(|error| match &name {
                        Some(name) => format!("{name}: {error}"),
                        None => error,
                    });
                    gather.lock().unwrap().done(i, item);
                });
                let _progress = unsafe {
                    provider.loadDataRepresentationForTypeIdentifier_completionHandler(&image_id, &block)
                };
            } else {
                unreachable!("Apple returned media outside the configured picker filter");
            }
        }
    }
}

enum PickFormat {Photo, Video, File}

#[derive(Clone, Copy)]
enum PickKind {Image, Video, Media, File, Data}

impl PickKind {
    fn image(self) -> bool {matches!(self, Self::Image | Self::Media | Self::Data)}
    fn video(self) -> bool {matches!(self, Self::Video | Self::Media | Self::Data)}
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Frames]
    struct CameraDelegate;

    unsafe impl NSObjectProtocol for CameraDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // NV12 (420v) → RGBA. Tight, integer-only, stride-aware.
            // Video-range BT.601 coefficients (standard for camera 420v).
            let image_buffer = unsafe { sample_buffer.image_buffer() }.unwrap();
            let pb: &CVPixelBuffer = image_buffer.as_ref();

            let flags = CVPixelBufferLockFlags::ReadOnly;
            if unsafe { CVPixelBufferLockBaseAddress(pb, flags) } != 0 {
                return;
            }

            let width = CVPixelBufferGetWidth(pb) as u32;
            let y_rows = CVPixelBufferGetHeightOfPlane(pb, 0);
            let uv_rows = CVPixelBufferGetHeightOfPlane(pb, 1);
            if width == 0 || y_rows == 0 || uv_rows == 0 {
                unsafe { CVPixelBufferUnlockBaseAddress(pb, flags) };
                return;
            }

            let y_base = CVPixelBufferGetBaseAddressOfPlane(pb, 0);
            let uv_base = CVPixelBufferGetBaseAddressOfPlane(pb, 1);
            let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
            let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
            let y_len = y_stride.checked_mul(y_rows).unwrap_or(0);
            let uv_len = uv_stride.checked_mul(uv_rows).unwrap_or(0);

            if y_base.is_null() || uv_base.is_null() || y_len == 0 || uv_len == 0 || y_stride < width as usize {
                unsafe { CVPixelBufferUnlockBaseAddress(pb, flags) };
                return;
            }

            let y_plane = unsafe { std::slice::from_raw_parts(y_base as *const u8, y_len) };
            let uv_plane = unsafe { std::slice::from_raw_parts(uv_base as *const u8, uv_len) };

            let mut dst = vec![0u8; (width as usize) * y_rows * 4];
            let w = width as usize;
            let h = y_rows;

            // Integer BT.601 limited-range conversion
            for y in 0..h {
                let y_row = y * y_stride;
                let uv_row = (y / 2) * uv_stride;
                let dst_row = y * w * 4;

                for x in 0..w {
                    let yi = y_row + x;
                    let uvi = uv_row + (x & !1); // UV is subsampled 2×
                    if yi >= y_plane.len() || uvi + 1 >= uv_plane.len() {continue;}

                    let y_val = y_plane[yi] as i32;
                    let u = uv_plane[uvi] as i32 - 128;
                    let v = uv_plane[uvi + 1] as i32 - 128;

                    let y_adj = (y_val - 16).max(0);

                    let r = ((298 * y_adj + 409 * v + 128) >> 8).clamp(0, 255) as u8;
                    let g = ((298 * y_adj - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8;
                    let b = ((298 * y_adj + 516 * u + 128) >> 8).clamp(0, 255) as u8;

                    let di = dst_row + x * 4;
                    dst[di] = r;
                    dst[di + 1] = g;
                    dst[di + 2] = b;
                    dst[di + 3] = 255;
                }
            }

            unsafe { CVPixelBufferUnlockBaseAddress(pb, flags) };
            if let Some(image) = ImageBuffer::from_raw(width, y_rows as u32, dst) {
                self.ivars().store(Arc::new(image));
            }
        }
    }
);

impl CameraDelegate {
    fn new(frames: Frames) -> Retained<Self> {
        let this = Self::alloc().set_ivars(frames);
        unsafe { msg_send![super(this), init] }
    }
}

struct PickLoad<F: FnOnce(Vec<Result<Data, String>>) + Send + 'static> {
    completion: Option<F>,
    items: Vec<Option<Result<Data, String>>>,
    left: usize,
}

impl<F: FnOnce(Vec<Result<Data, String>>) + Send + 'static> PickLoad<F> {
    fn new(completion: F, count: usize) -> Arc<Mutex<Self>> {
        assert!(count > 0, "An empty selection must cancel before loading");
        Arc::new(Mutex::new(PickLoad{
            completion: Some(completion), items: vec![None; count], left: count,
        }))
    }

    fn done(&mut self, index: usize, item: Result<Data, String>) {
        self.items[index] = Some(item);
        self.left -= 1;
        if self.left == 0 {
            let items = std::mem::take(&mut self.items).into_iter()
                .map(|item| item.unwrap()).collect();
            let completion = self.completion.take().unwrap();
            DispatchQueue::main().exec_async(move || completion(items));
        }
    }
}
