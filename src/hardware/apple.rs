use block2::RcBlock;
use winit::event_loop::EventLoopProxy;

use dispatch2::{DispatchQueue, DispatchQoS, GlobalQueueIdentifier};

use objc2::Message;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyObject, AnyClass};
use objc2::msg_send;
use objc2_uniform_type_identifiers::{
    UTType, UTTypeMovie, UTTypeQuickTimeMovie, UTTypeMPEG4Movie, UTTypeAVI, UTTypeMPEG,
    UTTypeMPEG2TransportStream, UTTypeImage, UTTypeVideo, UTTypeAudio, UTTypeData,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSNumber,
    NSString, NSURL, NSURLContentTypeKey, NSURLIsExcludedFromBackupKey,
    NSProcessInfo,
};
use objc2_media_player::{
    MPRemoteCommandCenter, MPRemoteCommand, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
    MPNowPlayingInfoCenter, MPMediaItemPropertyTitle,
    MPNowPlayingInfoPropertyPlaybackRate,
};

use objc2_photos_ui::{PHPickerConfiguration, PHPickerFilter, PHPickerResult};
use objc2_core_foundation::{CFRetained, CGPoint, CGSize, CGRect};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo, CGDataProvider, CGImage,
    CGColorRenderingIntent, CGBitmapContextGetData, CGBitmapContextCreate, CGContext,
};

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::fs::OpenOptions;
use std::io::{Cursor, Read, Write};
use std::ptr::NonNull;

use crate::inputs::{Input, Media as MediaInput};
use crate::events::Output;
use super::{Clock, Data, DataPicker, Format, Media};
use super::picker::PickKind;
pub use super::picker::PickRequest;

mod capture;
pub use capture::{Camera, Microphone, Capture, CaptureHub};

static MEDIA_COMMANDS: AtomicBool = AtomicBool::new(false);

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

pub struct Notifications;
impl Notifications {
    pub fn authorize() {
        if objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_none() {return}
        let center = objc2_user_notifications::UNUserNotificationCenter::currentNotificationCenter();
        let options = objc2_user_notifications::UNAuthorizationOptions::Alert
            | objc2_user_notifications::UNAuthorizationOptions::Sound;
        center.requestAuthorizationWithOptions_completionHandler(options, &RcBlock::new(|_, _| {}));
    }
    pub fn send(header: &str, body: &str) {
        if objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_none() {return}
        use objc2_user_notifications::{
            UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
            UNUserNotificationCenter,
        };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(header));
        content.setBody(&NSString::from_str(body));
        let id = NSString::from_str(&format!("{:032x}", rand::random::<u128>()));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
        center.requestAuthorizationWithOptions_completionHandler(options, &RcBlock::new(move |granted: objc2::runtime::Bool, _| {
            if granted.as_bool() {
                UNUserNotificationCenter::currentNotificationCenter()
                    .addNotificationRequest_withCompletionHandler(&request, None);
            }
        }));
    }
}

pub struct MediaCommands(Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>);
impl MediaCommands {
    pub fn new(proxy: EventLoopProxy<Output>) -> Self {
        MEDIA_COMMANDS.store(true, Ordering::Release);
        let center = unsafe {MPRemoteCommandCenter::sharedCommandCenter()};
        let commands = unsafe {[
            center.playCommand(), center.pauseCommand(), center.togglePlayPauseCommand(),
        ]};
        Self(commands.into_iter().enumerate().map(|(kind, command)| {
            let proxy = proxy.clone();
            let block = RcBlock::new(move |_: NonNull<MPRemoteCommandEvent>| {
                let input = match kind {
                    0 => MediaInput::Play, 1 => MediaInput::Pause,
                    _ => if Clock::playing().unwrap_or(false) {MediaInput::Pause} else {MediaInput::Play},
                };
                let _ = proxy.send_event(Output::Input(Input::Media(input)));
                MPRemoteCommandHandlerStatus::Success
            });
            let target = unsafe {
                command.setEnabled(true);
                command.addTargetWithHandler(&block)
            };
            (command, target)
        }).collect())
    }
    pub fn update() {
        if !MEDIA_COMMANDS.load(Ordering::Acquire) {return}
        DispatchQueue::main().exec_async(|| unsafe {
            if !MEDIA_COMMANDS.load(Ordering::Acquire) {return}
            let center = MPNowPlayingInfoCenter::defaultCenter();
            if let Some(playing) = Clock::playing() {
                let title = NSProcessInfo::processInfo().processName();
                let rate = NSNumber::new_f64(if playing {1.0} else {0.0});
                let info = NSDictionary::from_slices(&[
                    MPMediaItemPropertyTitle, MPNowPlayingInfoPropertyPlaybackRate,
                ], &[title.as_ref() as &AnyObject, rate.as_ref()]);
                center.setNowPlayingInfo(Some(&info));
                super::AudioSession::playback_state(Some(playing));
            } else {
                center.setNowPlayingInfo(None);
                super::AudioSession::playback_state(None);
            }
        });
    }
}
impl Drop for MediaCommands {
    fn drop(&mut self) {
        MEDIA_COMMANDS.store(false, Ordering::Release);
        for (command, target) in &self.0 {unsafe {command.removeTarget(Some(target));}}
        unsafe {
            let center = MPNowPlayingInfoCenter::defaultCenter();
            center.setNowPlayingInfo(None);
            super::AudioSession::playback_state(None);
        }
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
    pub fn device_id(directory: &Path) -> String {
        let path = directory.join("DEVICE_ID");
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(&path).unwrap();
        file.lock().unwrap();
        let mut id = String::new();
        file.read_to_string(&mut id).unwrap();
        if id.is_empty() {
            id = hex::encode(rand::random::<[u8; 16]>());
            file.write_all(id.as_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        assert!(id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
        let excluded = NSNumber::new_bool(true);
        unsafe {
            url.setResourceValue_forKey_error(Some(&excluded), NSURLIsExcludedFromBackupKey).unwrap();
        }
        id
    }

    pub fn get(name: String) -> PathBuf {
        unsafe {
            let file_manager_class = AnyClass::get(c"NSFileManager").unwrap();
            let file_manager: *mut AnyObject = msg_send![file_manager_class, defaultManager];

            let mut error: *mut AnyObject = std::ptr::null_mut();
            let url: *mut NSURL = msg_send![
                file_manager,
                URLForDirectory: objc2_foundation::NSSearchPathDirectory::ApplicationSupportDirectory,
                inDomain: objc2_foundation::NSSearchPathDomainMask::UserDomainMask,
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

pub struct AudioType;
impl AudioType {
    pub fn matches(identifier: &NSString) -> bool {
        UTType::typeWithIdentifier(identifier)
            .is_some_and(|ty| ty.conformsToType(unsafe {UTTypeAudio}))
    }
}

pub struct ShareMedia {
    path: RefCell<Option<PathBuf>>,
}

impl ShareMedia {
    pub fn new() -> Self {Self{path: RefCell::new(None)}}

    pub fn native_image(image: &super::Image) -> CFRetained<CGImage> {
        let image = Arc::clone(image);
        let (width, height) = (image.width() as usize, image.height() as usize);
        let data = image.as_raw();
        let (pointer, length) = (data.as_ptr().cast(), data.len());
        let info = Arc::into_raw(image) as *mut std::ffi::c_void;
        let provider = unsafe {
            CGDataProvider::with_data(info, pointer, length, Some(Self::release_image)).unwrap()
        };
        let color = CGColorSpace::new_device_rgb().unwrap();
        // image::Rgba is straight RGBA, top row first.
        let bitmap = CGBitmapInfo(CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0);
        unsafe {
            CGImage::new(
                width, height, 8, 32, width * 4, Some(&color), bitmap, Some(&provider),
                std::ptr::null(), true, CGColorRenderingIntent::RenderingIntentDefault,
            ).unwrap()
        }
    }
    pub fn image(image: &CGImage) -> super::Image {
        let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
        let color = CGColorSpace::new_device_rgb().unwrap();
        let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        let context = unsafe {
            CGBitmapContextCreate(
                std::ptr::null_mut(), width, height, 8, width * 4, Some(&color), info,
            ).unwrap()
        };
        CGContext::draw_image(
            Some(&context),
            CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64)), Some(image),
        );
        let pointer = NonNull::new(CGBitmapContextGetData(Some(&context)).cast::<u8>()).unwrap();
        let mut pixels = unsafe {
            std::slice::from_raw_parts(pointer.as_ptr(), width * height * 4)
        }.to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            if alpha == 0 {pixel[..3].fill(0);} else if alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel = (u32::from(*channel) * 255 / alpha).min(255) as u8;
                }
            }
        }
        image::RgbaImage::from_raw(width as u32, height as u32, pixels).unwrap().into()
    }
    pub fn export(media: &Media) -> Result<(Vec<u8>, Retained<UTType>), String> {
        let format = if media.dimensions().is_some() {
            Format::Mkv_Ffv1_PcmF32le
        } else {Format::Wav_PcmF32le};
        let mut file = Cursor::new(Vec::new());
        media.write(&mut file, format)?;
        let parent = unsafe {if media.dimensions().is_some() {UTTypeMovie} else {UTTypeAudio}};
        let ty = UTType::typeWithFilenameExtension_conformingToType(
            &NSString::from_str(format.extension()), parent,
        ).unwrap();
        Ok((file.into_inner(), ty))
    }

    pub fn path(&self, data: &Data) -> Option<PathBuf> {
        let Data::Media(media) = data else {return None};
        let (bytes, ty) = Self::export(media)
            .map_err(|error| log::error!("Export failed: {error}")).ok()?;
        let mut path = std::env::temp_dir().join(format!(
            "maverick-{}-{}", std::process::id(), rand::random::<u64>(),
        ));
        if let Some(ext) = ty.preferredFilenameExtension() {
            path.set_extension(ext.to_string());
        }
        std::fs::write(&path, bytes.as_slice()).ok()?;
        if let Some(old) = self.path.borrow_mut().replace(path.clone()) {
            let _ = std::fs::remove_file(old);
        }
        Some(path)
    }
    unsafe extern "C-unwind" fn release_image(info: *mut std::ffi::c_void, _data: NonNull<std::ffi::c_void>, _size: usize) {
        drop(unsafe {Arc::from_raw(info as *const image::RgbaImage)});
    }
}

impl Drop for ShareMedia {
    fn drop(&mut self) {
        if let Some(path) = self.path.borrow_mut().take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl PickRequest {
    pub fn documents(&self) -> bool {
        matches!(self.kind(), PickKind::Audio | PickKind::Media | PickKind::File | PickKind::Data)
    }

    pub fn photos(&self) -> Retained<PHPickerConfiguration> {
        let config = unsafe {PHPickerConfiguration::new()};
        unsafe {
            let filter = match self.kind() {
                PickKind::Image => PHPickerFilter::imagesFilter(),
                PickKind::Video => PHPickerFilter::videosFilter(),
                PickKind::Visual => PHPickerFilter::anyFilterMatchingSubfilters(
                    &NSArray::from_retained_slice(&[
                        PHPickerFilter::imagesFilter(), PHPickerFilter::videosFilter(),
                    ]),
                ),
                _ => unreachable!("This selection uses the document picker"),
            };
            config.setFilter(Some(&filter));
            config.setSelectionLimit(if self.multiple() {0} else {1});
        }
        config
    }

    pub fn file_types(&self) -> Vec<Retained<UTType>> {
        unsafe {match self.kind() {
            PickKind::Image => vec![UTTypeImage.retain()],
            PickKind::Video => vec![UTTypeMovie.retain()],
            PickKind::Audio => vec![UTTypeAudio.retain()],
            PickKind::Visual => vec![UTTypeImage.retain(), UTTypeMovie.retain()],
            PickKind::Media => vec![UTTypeAudio.retain(), UTTypeMovie.retain()],
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
            if [unsafe {UTTypeAudio}, unsafe {UTTypeMovie}, unsafe {UTTypeVideo}]
                .iter().any(|parent| ty.conformsToType(parent)) {return Ok(PickFormat::Media)}
        }
        Ok(PickFormat::File)
    }

    fn load_file(kind: PickKind, url: &NSURL) -> Result<Data, String> {
        let path = PathBuf::from(url.path().expect("The selected file has no path").to_string());
        if matches!(kind, PickKind::File) {return Ok(Data::File(path))}
        let access = unsafe {url.startAccessingSecurityScopedResource()};
        let item = (|| {
            let format = Self::format(url)?;
            let decode = match format {
                PickFormat::Photo => kind.image(),
                PickFormat::Media => kind.media(),
                PickFormat::File => false,
            };
            if !decode {
                return if matches!(kind, PickKind::Data) {Ok(Data::File(path.clone()))} else {
                    Err("The file does not match the requested media type.".into())
                }
            }
            let bytes = std::fs::read(&path).map_err(|error| format!("Could not read file: {error}"))?;
            match format {
                PickFormat::Photo => DataPicker::image(&NSData::with_bytes(&bytes)).map(Data::Image),
                PickFormat::Media => Media::read(Cursor::new(bytes)).map(Data::Media),
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
        let kind = self.kind();
        let urls: Vec<_> = urls.iter().collect();
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
            DispatchQoS::UserInitiated,
        ));
        queue.exec_async(move || autoreleasepool(|_| {
            let items = urls.iter().map(|url| Self::load_file(kind, &url)).collect();
            DispatchQueue::main().exec_async(move || completion(items));
        }));
    }

    pub fn load_photos<F: FnOnce(Vec<Result<Data, String>>) + Send + 'static>(&self, results: &NSArray<PHPickerResult>, completion: F) {
        let count = results.count();
        let gather = PickLoad::new(completion, count);
        let image_id = unsafe {UTTypeImage}.identifier();
        let movie_id = unsafe {UTTypeMovie}.identifier();
        for i in 0..count {
            let provider = unsafe {results.objectAtIndex(i).itemProvider()};
            let name = provider.suggestedName().map(|name| name.to_string());
            if self.kind().video() && provider.hasItemConformingToTypeIdentifier(&movie_id) {
                let gather = gather.clone();
                let block = RcBlock::new(move |url: *mut NSURL, error: *mut NSError| {
                    let item = if let Some(error) = unsafe {error.as_ref()} {
                        Err(format!("Could not load the video: {}", error.localizedDescription()))
                    } else {
                        let url = unsafe {url.as_ref()}.expect("Apple returned a video without a file");
                        let path = url.path().expect("The selected video has no path").to_string();
                        std::fs::File::open(path).map_err(|error| error.to_string())
                            .and_then(Media::read).map(Data::Media)
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
            } else if provider.hasItemConformingToTypeIdentifier(&image_id) {
                let gather = gather.clone();
                let block = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
                    let item = if let Some(error) = unsafe {error.as_ref()} {
                        log::warn!("Image provider failed: {error:?}");
                        Err(format!("Could not load the image: {}", error.localizedDescription()))
                    } else {
                        let data = unsafe {data.as_ref()}.expect("Apple returned an image without data");
                        DataPicker::image(data).map(Data::Image)
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

enum PickFormat {Photo, Media, File}

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
