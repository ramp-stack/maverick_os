use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, MainThreadBound};
use ::media::{Image, Pixel, Sample, Sound};
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
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidthOfPlane,
    kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_601_4,
    kCVImageBufferYCbCrMatrix_ITU_R_709_2, kCVImageBufferYCbCrMatrix_ITU_R_2020,
    kCVImageBufferYCbCrMatrix_SMPTE_240M_1995,
    CVPixelBufferLockBaseAddress, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use objc2_core_audio_types::AudioBufferList;
use objc2_core_foundation::{CFRetained, CFString};
use tokio::sync::broadcast;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::{AudioSession, Camera, Microphone, Capture, Hardware, _Hardware};
use super::super::capture::{Feed, Frame};

type Feeds = Arc<Mutex<HashMap<u128, Feed>>>;
type FrameReceiver = broadcast::Receiver<(Duration, Arc<Frame>)>;
type MicrophoneSession = (
    Retained<AVCaptureDeviceInput>, Retained<AVCaptureAudioDataOutput>, AudioSession,
);

pub struct CaptureHub {
    owner: Weak<_Hardware>,
    session: Retained<AVCaptureSession>,
    feeds: Feeds,
    camera: RefCell<Option<(Retained<AVCaptureDeviceInput>, Retained<AVCaptureVideoDataOutput>)>>,
    microphone: RefCell<Option<MicrophoneSession>>,
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
    pub fn camera(&self, hardware: &Hardware) -> Result<Camera, String> {
        let (id, frames) = self.subscribe(true, false)?;
        Ok(Camera::new(hardware, id, frames))
    }
    pub fn microphone(&self, hardware: &Hardware) -> Result<Microphone, String> {
        let (id, frames) = self.subscribe(false, true)?;
        Ok(Microphone::new(hardware, id, frames))
    }
    pub fn capture(&self, hardware: &Hardware) -> Result<Capture, String> {
        let (id, frames) = self.subscribe(true, true)?;
        Ok(Capture::new(hardware, id, frames))
    }
    pub fn error(&self, id: u128) -> Option<String> {
        self.feeds.lock().unwrap().get(&id).and_then(|feed| feed.error())
    }
    pub fn remove(&self, id: u128) {
        let (camera, microphone) = {
            let mut feeds = self.feeds.lock().unwrap();
            feeds.remove(&id).unwrap();
            let active = feeds.values().filter(|feed| feed.started());
            (active.clone().any(|feed| feed.camera()), active.clone().any(|feed| feed.microphone()))
        };
        self.configure(camera, microphone).expect("Could not release capture inputs");
    }
    fn subscribe(&self, camera: bool, microphone: bool) -> Result<(u128, FrameReceiver), String> {
        let id = rand::random();
        let (feed, frames) = Feed::new(camera, microphone);
        self.feeds.lock().unwrap().insert(id, feed);
        if let Err(error) = self.start(id) {self.remove(id); return Err(error)}
        Ok((id, frames))
    }
    fn start(&self, id: u128) -> Result<(), String> {
        let Some(feed) = self.feeds.lock().unwrap().get(&id).cloned() else {return Ok(())};
        for (needed, media, name) in unsafe {[
            (feed.camera(), AVMediaTypeVideo.unwrap(), "Camera"),
            (feed.microphone(), AVMediaTypeAudio.unwrap(), "Microphone"),
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
                            && let Some(feed) = hub.feeds.lock().unwrap().get_mut(&id)
                        {
                            feed.fail(error);
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
            let active = feeds.values().filter(|feed| feed.started());
            (active.clone().any(|feed| feed.camera()), active.clone().any(|feed| feed.microphone()))
        };
        if let Err(error) = self.configure(camera || feed.camera(), microphone || feed.microphone()) {
            self.configure(camera, microphone).ok();
            return Err(error)
        }
        if !unsafe {self.session.isRunning()} {unsafe {self.session.startRunning();}}
        if !unsafe {self.session.isRunning()} {
            self.configure(camera, microphone).ok();
            return Err("Capture could not start".into())
        }
        let origin = unsafe {CMClock::host_time_clock().time().seconds()};
        self.feeds.lock().unwrap().get_mut(&id).unwrap().start(origin);
        Ok(())
    }
    fn configure(&self, camera: bool, microphone: bool) -> Result<(), String> {
        let mut _audio_session = None;
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
                _audio_session = Some(session);
            }
            Ok(())
        })();
        unsafe {self.session.commitConfiguration();}
        if !camera && !microphone && unsafe {self.session.isRunning()} {
            unsafe {self.session.stopRunning();}
        }
        result
    }
}
impl Drop for CaptureHub {
    fn drop(&mut self) {
        self.feeds.lock().unwrap().clear();
        self.configure(false, false).unwrap();
    }
}

#[repr(C)]
struct ImageBuffer {
    data: *mut c_void,
    height: usize,
    width: usize,
    row_bytes: usize,
}

#[repr(C)]
struct PixelRange {
    y_bias: i32,
    chroma_bias: i32,
    y_range: i32,
    chroma_range: i32,
    y_max: i32,
    y_min: i32,
    chroma_max: i32,
    chroma_min: i32,
}

#[repr(C)]
struct Matrix {
    y: f32,
    cr_red: f32,
    cr_green: f32,
    cb_green: f32,
    cb_blue: f32,
}

struct CaptureState {
    feeds: Feeds,
    session: Retained<AVCaptureSession>,
    interval: Mutex<Duration>,
    // objc2 ivar storage cannot hold vImage's 16-byte alignment directly.
    conversions: Box<[(&'static CFString, Conversion); 4]>,
}

#[repr(C, align(16))]
struct Conversion {
    opaque: [u8; 128],
}
impl Conversion {
    fn new(red: f32, blue: f32) -> Self {
        let green = 1.0 - red - blue;
        let matrix = Matrix {
            y: 1.0, cr_red: 2.0 * (1.0 - red), cr_green: -2.0 * red * (1.0 - red) / green,
            cb_green: -2.0 * blue * (1.0 - blue) / green, cb_blue: 2.0 * (1.0 - blue),
        };
        let range = PixelRange {
            y_bias: 16, chroma_bias: 128, y_range: 235, chroma_range: 240,
            y_max: 255, y_min: 0, chroma_max: 255, chroma_min: 0,
        };
        let mut conversion = Self {opaque: [0; 128]};
        // vImage's fixed input/output types are 420Yp8_CbCr8 (4) and ARGB8888 (0).
        let result = unsafe {
            vImageConvert_YpCbCrToARGB_GenerateConversion(&matrix, &range, &mut conversion, 4, 0, 0)
        };
        assert_eq!(result, 0, "Could not initialize camera color conversion");
        conversion
    }
    fn image(&self, buffer: &CVPixelBuffer) -> Result<Image, String> {
        let flags = CVPixelBufferLockFlags::ReadOnly;
        if unsafe {CVPixelBufferLockBaseAddress(buffer, flags)} != 0 {
            return Err("Invalid camera pixel buffer".into())
        }
        let result = (|| {
            let luma = ImageBuffer {
                data: CVPixelBufferGetBaseAddressOfPlane(buffer, 0),
                height: CVPixelBufferGetHeightOfPlane(buffer, 0),
                width: CVPixelBufferGetWidthOfPlane(buffer, 0),
                row_bytes: CVPixelBufferGetBytesPerRowOfPlane(buffer, 0),
            };
            let chroma = ImageBuffer {
                data: CVPixelBufferGetBaseAddressOfPlane(buffer, 1),
                height: CVPixelBufferGetHeightOfPlane(buffer, 1),
                width: CVPixelBufferGetWidthOfPlane(buffer, 1),
                row_bytes: CVPixelBufferGetBytesPerRowOfPlane(buffer, 1),
            };
            if luma.width == 0 || luma.height == 0 || luma.data.is_null() || chroma.data.is_null()
                || luma.row_bytes < luma.width || chroma.row_bytes < chroma.width * 2
                || chroma.width != luma.width.div_ceil(2) || chroma.height != luma.height.div_ceil(2)
            {
                return Err("Invalid camera pixel buffer".into())
            }
            let count = luma.width * luma.height;
            let mut pixels = Vec::<Pixel>::with_capacity(count);
            let output = ImageBuffer {
                data: pixels.as_mut_ptr().cast(), height: luma.height,
                width: luma.width, row_bytes: luma.width * std::mem::size_of::<Pixel>(),
            };
            let result = unsafe {
                vImageConvert_420Yp8_CbCr8ToARGB8888(
                    &luma, &chroma, &output, self, [1, 2, 3, 0].as_ptr(), 255, 0,
                )
            };
            if result != 0 {return Err(format!("Camera color conversion failed: {result}"))}
            // Successful vImage conversion initializes every RGBA pixel, including opaque alpha.
            unsafe {pixels.set_len(count);}
            Ok(Image::new((luma.width as u32, luma.height as u32), pixels))
        })();
        unsafe {CVPixelBufferUnlockBaseAddress(buffer, flags);}
        result
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
                self.image(sample).map(|image| Frame::Image(image, duration))
            } else {Self::sound(sample).map(|(sound, duration)| Frame::Sound(sound, duration))};
            let result = result.map(Arc::new);
            let mut feeds = state.feeds.lock().unwrap();
            for feed in feeds.values_mut() {
                match &result {
                    Ok(frame) => feed.send(timestamp, frame),
                    Err(error) => feed.fail(error.clone()),
                }
            }
        }
    }
);
impl CaptureDelegate {
    fn new(feeds: Feeds, session: Retained<AVCaptureSession>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(CaptureState {
            feeds, session, interval: Mutex::new(Duration::ZERO),
            conversions: Box::new(unsafe {[
                (kCVImageBufferYCbCrMatrix_ITU_R_601_4, 0.299, 0.114),
                (kCVImageBufferYCbCrMatrix_ITU_R_709_2, 0.2126, 0.0722),
                (kCVImageBufferYCbCrMatrix_ITU_R_2020, 0.2627, 0.0593),
                (kCVImageBufferYCbCrMatrix_SMPTE_240M_1995, 0.212, 0.087),
            ]}.map(|(key, red, blue)| (key, Conversion::new(red, blue)))),
        });
        unsafe {msg_send![super(this), init]}
    }
    fn sound(sample: &CMSampleBuffer) -> Result<(Sound, Duration), String> {
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
                samples.push(Sample(value));
            }
        }
        let duration = Duration::from_secs_f64(frames as f64 / format.mSampleRate);
        Ok((Sound {channels: channels as u16, samples}, duration))
    }
    fn image(&self, sample: &CMSampleBuffer) -> Result<Image, String> {
        let buffer = unsafe {sample.image_buffer()}.unwrap();
        let matrix = unsafe {buffer.attachment(kCVImageBufferYCbCrMatrixKey, std::ptr::null_mut())};
        let conversions = &self.ivars().conversions;
        let conversion = match matrix {
            Some(matrix) => {
                let matrix = matrix.downcast::<CFString>().unwrap();
                &conversions.iter().find(|(key, _)| **key == *matrix)
                    .ok_or("Unsupported camera color matrix")?.1
            },
            None => &conversions[0].1,
        };
        conversion.image(buffer.as_ref())
    }
}

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    fn vImageConvert_YpCbCrToARGB_GenerateConversion(
        matrix: &Matrix, range: &PixelRange, conversion: &mut Conversion,
        input: u32, output: u32, flags: u32,
    ) -> isize;
    fn vImageConvert_420Yp8_CbCr8ToARGB8888(
        luma: &ImageBuffer, chroma: &ImageBuffer, output: &ImageBuffer,
        conversion: &Conversion, permutation: *const u8, alpha: u8, flags: u32,
    ) -> isize;
}
