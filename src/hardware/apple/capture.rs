use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, MainThreadBound};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2_av_foundation::{
    AVCaptureAudioDataOutput, AVCaptureAudioDataOutputSampleBufferDelegate, AVCaptureConnection,
    AVCaptureDevice, AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureDeviceRotationCoordinator,
    AVCaptureOutput, AVCaptureSession, AVCaptureSessionPresetHigh, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeAudio, AVMediaTypeVideo,
    AVCaptureDeviceTypeBuiltInWideAngleCamera, AVAuthorizationStatus,
};
use objc2_core_media::{
    CMAudioFormatDescriptionGetStreamBasicDescription, CMClock, CMSampleBuffer, CMSyncConvertTime,
};
use objc2_core_video::{
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelBufferPixelFormatTypeKey,
    CVPixelBuffer, CVPixelBufferLockFlags, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use objc2_core_audio_types::AudioBufferList;
use objc2_core_foundation::CFRetained;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::{AudioSession, Hardware, Image, Media, Sound, _Hardware};
use super::super::media::{AudioFrame, VideoFrame, Source};

type Feeds = Arc<Mutex<HashMap<u128, Feed>>>;

#[derive(Clone)]
pub struct Camera {
    media: Media,
    subscription: Rc<Subscription>,
}
impl Camera {
    pub fn error(&self) -> Option<String> {self.subscription.error()}
}
impl AsRef<Media> for Camera {
    fn as_ref(&self) -> &Media {&self.media}
}

#[derive(Clone)]
pub struct Microphone {
    media: Media,
    subscription: Rc<Subscription>,
}
impl Microphone {
    pub fn error(&self) -> Option<String> {self.subscription.error()}
}
impl AsRef<Media> for Microphone {
    fn as_ref(&self) -> &Media {&self.media}
}

#[derive(Clone)]
pub struct Capture {
    media: Media,
    subscription: Rc<Subscription>,
}
impl Capture {
    pub fn error(&self) -> Option<String> {self.subscription.error()}
}
impl AsRef<Media> for Capture {
    fn as_ref(&self) -> &Media {&self.media}
}

pub struct CaptureHub {
    owner: Weak<_Hardware>,
    session: Retained<AVCaptureSession>,
    feeds: Feeds,
    camera: RefCell<Option<(Retained<AVCaptureDeviceInput>, Retained<AVCaptureVideoDataOutput>)>>,
    microphone: RefCell<Option<(
        Retained<AVCaptureDeviceInput>, Retained<AVCaptureAudioDataOutput>, AudioSession,
    )>>,
    rotation: RefCell<Option<Retained<AVCaptureDeviceRotationCoordinator>>>,
    delegate: Retained<CaptureDelegate>,
}
impl CaptureHub {
    pub fn new(hardware: &Hardware) -> Self {
        let session = unsafe {AVCaptureSession::new()};
        let feeds = Arc::new(Mutex::new(HashMap::new()));
        let delegate = CaptureDelegate::new(feeds.clone(), session.clone());
        Self {
            owner: Rc::downgrade(&hardware.0), session, feeds,
            camera: RefCell::new(None), microphone: RefCell::new(None),
            rotation: RefCell::new(None), delegate,
        }
    }
    pub fn camera(&self) -> Result<Camera, String> {
        let media = Media::new();
        let subscription = self.subscribe(&media, true, false)?;
        Ok(Camera {media, subscription})
    }
    pub fn microphone(&self) -> Result<Microphone, String> {
        let media = Media::new();
        let subscription = self.subscribe(&media, false, true)?;
        Ok(Microphone {media, subscription})
    }
    pub fn capture(&self) -> Result<Capture, String> {
        let media = Media::new();
        let subscription = self.subscribe(&media, true, true)?;
        Ok(Capture {media, subscription})
    }
    fn subscribe(&self, media: &Media, camera: bool, microphone: bool) -> Result<Rc<Subscription>, String> {
        let source = <&Source>::from(media);
        let offset = if camera {source.capture_video(microphone)?} else {source.capture_audio()?};
        let id = rand::random();
        self.feeds.lock().unwrap().insert(id, Feed {
            media: media.clone(), camera, microphone, origin: None, offset,
        });
        if let Err(error) = self.start(id) {self.remove(id); return Err(error)}
        Ok(Rc::new(Subscription {owner: self.owner.clone(), id}))
    }
    fn start(&self, id: u128) -> Result<(), String> {
        let Some(feed) = self.feeds.lock().unwrap().get(&id).cloned() else {return Ok(())};
        for (needed, media, name) in unsafe {[
            (feed.camera, AVMediaTypeVideo.unwrap(), "Camera"),
            (feed.microphone, AVMediaTypeAudio.unwrap(), "Microphone"),
        ]} {
            if !needed {continue}
            match unsafe {AVCaptureDevice::authorizationStatusForMediaType(media)} {
                AVAuthorizationStatus::Authorized => {},
                AVAuthorizationStatus::NotDetermined => {
                    let owner = MainThreadBound::new(
                        self.owner.clone(), MainThreadMarker::new().unwrap(),
                    );
                    let completion = RcBlock::new(move |_granted: Bool| owner.get_on_main(|owner| {
                        let Some(owner) = owner.upgrade() else {return};
                        let hub = owner.capture.get().unwrap();
                        if let Err(error) = hub.start(id)
                            && let Some(feed) = hub.feeds.lock().unwrap().get(&id)
                        {
                            <&Source>::from(&feed.media).fail(error);
                        }
                    }));
                    unsafe {AVCaptureDevice::requestAccessForMediaType_completionHandler(
                        media, &completion,
                    );}
                    return Ok(())
                }
                _ => return Err(format!("{name} access is denied; allow it in system settings")),
            }
        }
        let (camera, microphone) = {
            let feeds = self.feeds.lock().unwrap();
            let active = feeds.values().filter(|feed| feed.origin.is_some());
            (active.clone().any(|feed| feed.camera), active.clone().any(|feed| feed.microphone))
        };
        if let Err(error) = self.configure(camera || feed.camera, microphone || feed.microphone) {
            self.configure(camera, microphone).ok();
            return Err(error)
        }
        if !unsafe {self.session.isRunning()} {unsafe {self.session.startRunning();}}
        if !unsafe {self.session.isRunning()} {
            self.configure(camera, microphone).ok();
            return Err("Capture could not start".into())
        }
        let origin = unsafe {CMClock::host_time_clock().time().seconds()};
        self.feeds.lock().unwrap().get_mut(&id).unwrap().origin = Some(origin);
        Ok(())
    }
    fn configure(&self, camera: bool, microphone: bool) -> Result<(), String> {
        let mut audio_session = None;
        unsafe {self.session.beginConfiguration();}
        let result = (|| {
            if unsafe {self.session.canSetSessionPreset(AVCaptureSessionPresetHigh)} {
                unsafe {self.session.setSessionPreset(AVCaptureSessionPresetHigh);}
            }
            if camera && self.camera.borrow().is_none() {
                let device = unsafe {AVCaptureDevice::defaultDeviceWithDeviceType_mediaType_position(
                    AVCaptureDeviceTypeBuiltInWideAngleCamera, AVMediaTypeVideo,
                    AVCaptureDevicePosition::Unspecified,
                )}.ok_or("No camera is available")?;
                let input = unsafe {AVCaptureDeviceInput::deviceInputWithDevice_error(&device)}
                    .map_err(|error| error.localizedDescription().to_string())?;
                let output = unsafe {AVCaptureVideoDataOutput::new()};
                let key = unsafe {
                    &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString)
                };
                let value = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
                let settings = NSDictionary::from_slices::<NSString>(&[key], &[value.as_ref()]);
                unsafe {output.setVideoSettings(Some(&settings));}
                if !unsafe {self.session.canAddInput(&input) && self.session.canAddOutput(&output)} {
                    return Err("Camera cannot be added to capture".into())
                }
                let interval = unsafe {device.activeVideoMinFrameDuration().seconds()};
                let interval = Duration::try_from_secs_f64(interval).map_err(|error| error.to_string())?;
                let queue = DispatchQueue::new("capture.camera", DispatchQueueAttr::SERIAL);
                unsafe {
                    output.setSampleBufferDelegate_queue(
                        Some(ProtocolObject::from_ref(&*self.delegate)), Some(&queue),
                    );
                    self.session.addInput(&input);
                    self.session.addOutput(&output);
                }
                let rotation = unsafe {AVCaptureDeviceRotationCoordinator::initWithDevice_previewLayer(
                    AVCaptureDeviceRotationCoordinator::alloc(), &device, None,
                )};
                if let Some(media) = unsafe {AVMediaTypeVideo}
                    && let Some(connection) = unsafe {output.connectionWithMediaType(media)}
                {
                    let angle = unsafe {rotation.videoRotationAngleForHorizonLevelCapture()};
                    if unsafe {connection.isVideoRotationAngleSupported(angle)} {
                        unsafe {connection.setVideoRotationAngle(angle);}
                    }
                }
                *self.delegate.ivars().interval.lock().unwrap() = interval;
                *self.rotation.borrow_mut() = Some(rotation);
                *self.camera.borrow_mut() = Some((input, output));
            }
            if microphone && self.microphone.borrow().is_none() {
                let audio_session = AudioSession::recording()?;
                AudioSession::configure_capture(&self.session);
                let device = unsafe {AVCaptureDevice::defaultDeviceWithMediaType(AVMediaTypeAudio.unwrap())}
                    .ok_or("No microphone is available")?;
                let input = unsafe {AVCaptureDeviceInput::deviceInputWithDevice_error(&device)}
                    .map_err(|error| error.localizedDescription().to_string())?;
                let output = unsafe {AVCaptureAudioDataOutput::new()};
                if !unsafe {self.session.canAddInput(&input) && self.session.canAddOutput(&output)} {
                    return Err("Microphone cannot be added to capture".into())
                }
                let queue = DispatchQueue::new("capture.microphone", DispatchQueueAttr::SERIAL);
                unsafe {
                    output.setSampleBufferDelegate_queue(
                        Some(ProtocolObject::from_ref(&*self.delegate)), Some(&queue),
                    );
                    self.session.addInput(&input);
                    self.session.addOutput(&output);
                }
                *self.microphone.borrow_mut() = Some((input, output, audio_session));
            }
            if !camera && let Some((input, output)) = self.camera.borrow_mut().take() {
                unsafe {
                    output.setSampleBufferDelegate_queue(None, None);
                    self.session.removeOutput(&output);
                    self.session.removeInput(&input);
                }
                self.rotation.borrow_mut().take();
            }
            if !microphone && let Some((input, output, session)) = self.microphone.borrow_mut().take() {
                unsafe {
                    output.setSampleBufferDelegate_queue(None, None);
                    self.session.removeOutput(&output);
                    self.session.removeInput(&input);
                }
                audio_session = Some(session);
            }
            Ok(())
        })();
        unsafe {self.session.commitConfiguration();}
        if !camera && !microphone && unsafe {self.session.isRunning()} {
            unsafe {self.session.stopRunning();}
        }
        drop(audio_session);
        result
    }
    fn remove(&self, id: u128) {
        let (feed, camera, microphone) = {
            let mut feeds = self.feeds.lock().unwrap();
            let feed = feeds.remove(&id).unwrap();
            let active = feeds.values().filter(|feed| feed.origin.is_some());
            (feed, active.clone().any(|feed| feed.camera),
                active.clone().any(|feed| feed.microphone))
        };
        <&Source>::from(&feed.media).end_capture();
        self.configure(camera, microphone).expect("Could not release capture inputs");
    }
}
impl Drop for CaptureHub {
    fn drop(&mut self) {
        let feeds = std::mem::take(&mut *self.feeds.lock().unwrap());
        for feed in feeds.into_values() {<&Source>::from(&feed.media).end_capture();}
        self.configure(false, false).unwrap();
    }
}

enum Frame {
    Image(Image, Duration),
    Sound(Sound),
}

#[derive(Clone)]
struct Feed {
    media: Media,
    camera: bool,
    microphone: bool,
    origin: Option<f64>,
    offset: Duration,
}

struct CaptureState {
    feeds: Feeds,
    session: Retained<AVCaptureSession>,
    interval: Mutex<Duration>,
}

struct Subscription {
    owner: Weak<_Hardware>,
    id: u128,
}
impl Subscription {
    fn error(&self) -> Option<String> {
        let owner = self.owner.upgrade()?;
        let hub = owner.capture.get().unwrap();
        let feeds = hub.feeds.lock().unwrap();
        feeds.get(&self.id).and_then(|feed| <&Source>::from(&feed.media).error())
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {owner.capture.get().unwrap().remove(self.id);}
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = CaptureState]
    struct CaptureDelegate;

    unsafe impl NSObjectProtocol for CaptureDelegate {}
    unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for CaptureDelegate {}
    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CaptureDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn captured(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _connection: &AVCaptureConnection) {
            let state = self.ivars();
            let Some(clock) = (unsafe {state.session.synchronizationClock()}) else {return};
            let host = unsafe {CMClock::host_time_clock()};
            let timestamp = unsafe {
                CMSyncConvertTime(sample.presentation_time_stamp(), &clock, &host).seconds()
            };
            if state.feeds.lock().unwrap().is_empty() {return}
            let result = if unsafe {sample.image_buffer()}.is_some() {
                let duration = Duration::try_from_secs_f64(unsafe {sample.duration().seconds()})
                    .ok().filter(|duration| !duration.is_zero())
                    .unwrap_or(*state.interval.lock().unwrap());
                Self::image(sample).map(|image| Frame::Image(image, duration))
            } else {Self::sound(sample).map(Frame::Sound)};
            let feeds = state.feeds.lock().unwrap();
            for feed in feeds.values() {
                let Some(origin) = feed.origin else {continue};
                if <&Source>::from(&feed.media).error().is_some() {continue}
                let Ok(position) = Duration::try_from_secs_f64(timestamp - origin) else {continue};
                let position = feed.offset + position;
                let result = match (&result, feed.camera, feed.microphone) {
                    (Ok(Frame::Image(image, duration)), true, _) => {
                        <&Source>::from(&feed.media).splice_video(VideoFrame {
                            position, image: image.clone(), duration: *duration,
                        })
                    }
                    (Ok(Frame::Sound(sound)), _, true) => {
                        <&Source>::from(&feed.media).splice_audio(AudioFrame {position, sound: sound.clone()})
                    }
                    (Err(error), _, _) => Err(error.clone()),
                    _ => Ok(()),
                };
                if let Err(error) = result {<&Source>::from(&feed.media).fail(error);}
            }
        }
    }
);
impl CaptureDelegate {
    fn new(feeds: Feeds, session: Retained<AVCaptureSession>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(CaptureState {
            feeds, session, interval: Mutex::new(Duration::ZERO),
        });
        unsafe {msg_send![super(this), init]}
    }
    fn sound(sample: &CMSampleBuffer) -> Result<Sound, String> {
        let description = unsafe {sample.format_description()}.ok_or("Missing microphone format")?;
        let format = unsafe {CMAudioFormatDescriptionGetStreamBasicDescription(&description).as_ref()}
            .ok_or("Missing microphone audio format")?;
        if format.mFormatID != u32::from_be_bytes(*b"lpcm") {
            return Err("Microphone did not produce PCM samples".into())
        }
        let mut size = 0;
        unsafe {sample.audio_buffer_list_with_retained_block_buffer(
            &mut size, std::ptr::null_mut(), 0, None, None, 0, std::ptr::null_mut(),
        );}
        let unit = std::mem::size_of::<AudioBufferList>();
        let mut storage = vec![std::mem::MaybeUninit::<AudioBufferList>::uninit(); size.div_ceil(unit)];
        let list = storage.as_mut_ptr().cast::<AudioBufferList>();
        let mut block = std::ptr::null_mut();
        let result = unsafe {sample.audio_buffer_list_with_retained_block_buffer(
            std::ptr::null_mut(), list, size, None, None, 0, &mut block,
        )};
        let _block = NonNull::new(block).map(|block| unsafe {CFRetained::from_raw(block)});
        if result != 0 {return Err(format!("Could not read microphone samples: {result}"))}
        let buffers = unsafe {
            std::slice::from_raw_parts((*list).mBuffers.as_ptr(), (*list).mNumberBuffers as usize)
        };
        let channels = format.mChannelsPerFrame as usize;
        let frames = unsafe {sample.num_samples()} as usize;
        let bits = format.mBitsPerChannel;
        let flags = format.mFormatFlags;
        let planar = flags & 32 != 0;
        let width = format.mBytesPerFrame as usize / if planar {1} else {channels};
        if width == 0 || width > 8 || bits == 0 || bits as usize > width * 8 {
            return Err("Unsupported microphone sample format".into())
        }
        let mut samples = Vec::with_capacity(frames * channels);
        for frame in 0..frames {
            for channel in 0..channels {
                let buffer = &buffers[if planar {channel} else {0}];
                let offset = frame * format.mBytesPerFrame as usize + if planar {0} else {channel * width};
                if offset + width > buffer.mDataByteSize as usize || buffer.mData.is_null() {
                    return Err("Incomplete microphone samples".into())
                }
                let bytes = unsafe {std::slice::from_raw_parts(buffer.mData.cast::<u8>().add(offset), width)};
                let mut value = 0u64;
                for (index, byte) in bytes.iter().enumerate() {
                    let shift = if flags & 2 != 0 {width - index - 1} else {index};
                    value |= u64::from(*byte) << (shift * 8);
                }
                let value = if flags & 1 != 0 {
                    match bits {
                        32 => f32::from_bits(value as u32), 64 => f64::from_bits(value) as f32,
                        _ => return Err("Unsupported floating microphone sample".into()),
                    }
                } else {
                    if flags & 16 != 0 {value >>= width * 8 - bits as usize;}
                    let scale = 2.0f64.powi(bits as i32 - 1);
                    if flags & 4 != 0 {
                        let signed = ((value << (64 - bits)) as i64) >> (64 - bits);
                        (signed as f64 / scale) as f32
                    } else {(value as f64 / scale - 1.0) as f32}
                };
                samples.push(value);
            }
        }
        Ok(Sound {
            sample_rate: format.mSampleRate as u32, channels: channels as u16, samples: Arc::new(samples),
        })
    }
    fn image(sample: &CMSampleBuffer) -> Result<Image, String> {
        let buffer = unsafe {sample.image_buffer()}.unwrap();
        let buffer: &CVPixelBuffer = buffer.as_ref();
        let flags = CVPixelBufferLockFlags::ReadOnly;
        if unsafe {CVPixelBufferLockBaseAddress(buffer, flags)} != 0 {
            return Err("Invalid camera pixel buffer".into())
        }
        let result = (|| {
            let width = CVPixelBufferGetWidth(buffer) as u32;
            let height = CVPixelBufferGetHeightOfPlane(buffer, 0);
            let uv_height = CVPixelBufferGetHeightOfPlane(buffer, 1);
            let y_base = CVPixelBufferGetBaseAddressOfPlane(buffer, 0);
            let uv_base = CVPixelBufferGetBaseAddressOfPlane(buffer, 1);
            let y_stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, 0);
            let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, 1);
            let y_len = y_stride.checked_mul(height).unwrap_or(0);
            let uv_len = uv_stride.checked_mul(uv_height).unwrap_or(0);
            if width == 0 || y_len == 0 || uv_len == 0 || y_base.is_null() || uv_base.is_null()
                || y_stride < width as usize
            {
                return Err("Invalid camera pixel buffer".into())
            }
            let y_plane = unsafe {std::slice::from_raw_parts(y_base.cast::<u8>(), y_len)};
            let uv_plane = unsafe {std::slice::from_raw_parts(uv_base.cast::<u8>(), uv_len)};
            let mut image = image::RgbaImage::new(width, height as u32);
            for (x, y, pixel) in image.enumerate_pixels_mut() {
                let yi = y as usize * y_stride + x as usize;
                let uvi = y as usize / 2 * uv_stride + (x as usize & !1);
                if yi >= y_plane.len() || uvi + 1 >= uv_plane.len() {continue}
                let y = (y_plane[yi] as i32 - 16).max(0);
                let u = uv_plane[uvi] as i32 - 128;
                let v = uv_plane[uvi + 1] as i32 - 128;
                // BT.601 limited-range YUV to RGBA.
                *pixel = image::Rgba([
                    ((298 * y + 409 * v + 128) >> 8).clamp(0, 255) as u8,
                    ((298 * y - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8,
                    ((298 * y + 516 * u + 128) >> 8).clamp(0, 255) as u8, 255,
                ]);
            }
            Ok(image.into())
        })();
        unsafe {CVPixelBufferUnlockBaseAddress(buffer, flags);}
        result
    }
}
