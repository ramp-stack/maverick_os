use crossfire::{spsc, AsyncTx};
use jni::{JNIEnv, JavaVM, NativeMethod};
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue, JFloatArray, JByteBuffer};
use tokio::io::AsyncReadExt;
use tokio::sync::{Notify, broadcast};
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::EventLoopProxy;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{
    Camera, Microphone, Capture, Data, Format, Hardware, Image, Media, Pickable, Picked, PickResult,
    Pixel, PushToken, Sample, Sound,
};
use super::capture::{Feed, Frame};
use super::picker::{PickKind, PickRequest};
use crate::events::{Events as AppEvents, Output};
use crate::inputs::{Input, DeviceInput, Key, KeyboardState, Modifiers};
use crate::runtime::RUNTIME;

pub struct SafeArea;
impl SafeArea {
    pub fn is_mobile() -> bool {true}
    pub fn physical(window: &winit::window::Window) -> crate::PhysicalWindow {
        let size = window.inner_size();
        crate::PhysicalWindow {
            width: size.width, height: size.height, scale_factor: window.scale_factor(),
        }
    }
    pub fn get(_window: &winit::window::Window) -> (f32, f32, f32, f32) {
        Bridge::with(|env, activity| {
            let array = JFloatArray::from(env.call_method(activity, "safeArea", "()[F", &[])?.l()?);
            let mut insets = [0.0; 4];
            env.get_float_array_region(array, 0, &mut insets)?;
            Ok((insets[0], insets[1], insets[2], insets[3]))
        })
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
    pub fn get(&self) -> (f32, bool) {
        Bridge::with(|env, activity| {
            let inset = env.call_method(&activity, "keyboardInset", "()F", &[])?.f()?;
            let visible = env.call_method(activity, "keyboardVisible", "()Z", &[])?.z()?;
            Ok((inset, visible))
        })
    }
}

pub struct Events {
    activity: GlobalRef,
    vm: JavaVM,
    touches: usize,
}
impl Events {
    pub fn new(_window: &crate::Window, proxy: EventLoopProxy<Output>) -> Self {
        Bridge::with(|env, activity| {
            let class = env.get_object_class(&activity)?;
            env.register_native_methods(class, &[
                NativeMethod {
                    name: "mediaCommand".into(), sig: "(I)V".into(),
                    fn_ptr: Self::media as *mut std::ffi::c_void,
                },
                NativeMethod {
                    name: "notificationReceived".into(),
                    sig: "(Ljava/lang/String;Ljava/lang/String;)V".into(),
                    fn_ptr: Self::notification as *mut std::ffi::c_void,
                },
                NativeMethod {
                    name: "windowChanged".into(), sig: "()V".into(),
                    fn_ptr: Self::window as *mut std::ffi::c_void,
                },
                NativeMethod {
                    name: "keyboardText".into(), sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Keyboards::text as *mut std::ffi::c_void,
                },
                NativeMethod {
                    name: "keyboardKey".into(), sig: "(IIIII)Z".into(),
                    fn_ptr: Keyboards::key as *mut std::ffi::c_void,
                },
            ])?;
            let activity = env.new_global_ref(&activity)?;
            let vm = env.get_java_vm()?;
            // The Activity monitor protects callbacks until Drop takes this association back.
            unsafe {env.set_rust_field(activity.as_obj(), "events", proxy)?;}
            let owner = Self {activity, vm, touches: 0};
            env.call_method(owner.activity.as_obj(), "observeEvents", "()V", &[])?;
            Ok(owner)
        })
    }
    pub fn process(&mut self, event: WindowEvent, _scale: f64) -> Option<WindowEvent> {
        // NativeActivity handles touches before Activity.dispatchTouchEvent can observe them.
        // NativeText's separate Dialog windows keep selection touches out of this queue.
        if let WindowEvent::Touch(touch) = &event {
            match touch.phase {
                TouchPhase::Started => self.touches += 1,
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    self.touches = self.touches.saturating_sub(1);
                }
                TouchPhase::Moved => {},
            }
            let action = match touch.phase {
                TouchPhase::Started => 0, TouchPhase::Moved => 2,
                TouchPhase::Ended => 1, TouchPhase::Cancelled => 3,
            };
            let mut env = self.vm.attach_current_thread().unwrap();
            env.call_method(self.activity.as_obj(), "keyboardTouch", "(IJFFI)V", &[
                JValue::Int(action), JValue::Long(touch.id as i64),
                JValue::Float(touch.location.x as f32), JValue::Float(touch.location.y as f32),
                JValue::Int(self.touches as i32),
            ]).unwrap();
        }
        Some(event)
    }
    pub fn suspend(&mut self) {self.touches = 0;}
    pub async fn next(&mut self) -> Input {std::future::pending().await}

    fn proxy(env: &mut JNIEnv, activity: JObject) -> EventLoopProxy<Output> {
        unsafe {env.get_rust_field::<_, _, EventLoopProxy<Output>>(activity, "events").unwrap().clone()}
    }

    extern "system" fn media(mut env: JNIEnv, activity: JObject, command: i32) {
        let event = match command {
            0 => crate::inputs::Media::Play, 1 => crate::inputs::Media::Pause,
            2 => crate::inputs::Media::Toggle, _ => unreachable!(),
        };
        let _ = Self::proxy(&mut env, activity).send_event(Output::Input(Input::Media(event)));
    }
    extern "system" fn window(mut env: JNIEnv, activity: JObject) {
        let _ = Self::proxy(&mut env, activity).send_event(Output::GeometryChanged);
    }
    extern "system" fn notification(mut env: JNIEnv, activity: JObject, title: JString, body: JString) {
        let proxy = Self::proxy(&mut env, activity);
        let title = String::from(env.get_string(&title).unwrap());
        let body = String::from(env.get_string(&body).unwrap());
        let _ = proxy.send_event(Output::Input(Input::Notification(title, body)));
    }
}
impl Drop for Events {
    fn drop(&mut self) {
        let mut env = self.vm.attach_current_thread().unwrap();
        let activity = self.activity.as_obj();
        let _monitor = env.lock_obj(activity).unwrap();
        env.call_method(activity, "stopEvents", "()V", &[]).unwrap();
        unsafe {env.take_rust_field::<_, _, EventLoopProxy<Output>>(activity, "events").unwrap();}
    }
}

#[derive(Clone)]
pub struct Keyboard(Rc<Keyboards>);
impl Keyboard {
    pub(crate) fn new(_proxy: EventLoopProxy<Output>, inset: Arc<KeyboardInset>) -> Self {
        Self(Rc::new(Keyboards {id: Cell::new(None), inset}))
    }
    pub fn show(&self, _padding: f32) {
        if let Some(id) = self.0.id.get() {
            Bridge::with(|env, activity| {
                env.call_method(activity, "reopenKeyboard", "(I)V", &[JValue::Int(id)]).map(|_| ())
            });
        } else {
            let id = Bridge::with(|env, activity| {
                env.call_method(activity, "openKeyboard", "()I", &[])?.i()
            });
            self.0.id.set(Some(id));
        }
    }
    pub fn hide(&self) {
        if let Some(id) = self.0.id.get() {
            Bridge::with(|env, activity| {
                env.call_method(activity, "hideKeyboard", "(I)V", &[JValue::Int(id)]).map(|_| ())
            });
        }
    }
    pub fn visible(&self) -> bool {self.0.inset.get().1}
}

pub struct Logger;
impl Logger {
    pub fn start(level: Option<log::Level>) {
        android_logger::init_once(android_logger::Config::default().with_tag("Maverick")
            .with_max_level(level.unwrap_or(log::Level::Info).to_level_filter()));
    }
}

pub struct ApplicationSupport;
impl ApplicationSupport {
    pub fn get(_name: String) -> PathBuf {
        PathBuf::from(Bridge::with(|env, activity| {
            let directory = env.call_method(activity, "getFilesDir", "()Ljava/io/File;", &[])?.l()?;
            let path = env.call_method(directory, "getAbsolutePath", "()Ljava/lang/String;", &[])?.l()?;
            Ok(String::from(env.get_string(&JString::from(path))?))
        }))
    }

    pub fn device_id(directory: &Path) -> String {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .mode(0o600).open(directory.join("DEVICE_ID")).unwrap();
        assert_eq!(unsafe {libc::flock(file.as_raw_fd(), libc::LOCK_EX)}, 0);
        let mut id = String::new();
        file.read_to_string(&mut id).unwrap();
        if id.is_empty() {
            id = hex::encode(rand::random::<[u8; 16]>());
            file.write_all(id.as_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        assert!(id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        id
    }
}

pub struct Clipboard;
impl Clipboard {
    pub fn new() -> Self {Self}
    pub fn set(&self, data: &Data) {Bridge::send("clipboardWrite", data);}
    pub fn get(&self) -> Option<Data> {
        let value = Bridge::with(|env, activity| {
            let value = env.call_method(activity, "clipboardRead", "()Ljava/lang/String;", &[])?.l()?;
            if value.is_null() {return Ok(None)}
            Ok(Some(String::from(env.get_string(&JString::from(value))?)))
        })?;
        let value: serde_json::Value = serde_json::from_str(&value).unwrap();
        if let Some(text) = value["text"].as_str() {return Some(Data::Text(text.to_owned()))}
        match Bridge::load(PickKind::Data, &value) {
            Ok(data) => Some(data),
            Err(error) => {log::warn!("Clipboard unavailable: {error}"); None}
        }
    }
}

pub struct Haptics;
impl Haptics {
    pub fn new() -> Self {Self}
    pub fn trigger(&self) {
        Bridge::with(|env, activity| env.call_method(activity, "haptic", "()V", &[]).map(|_| ()));
    }
}

pub struct Push {
    previous: String,
}
impl Push {
    pub fn new() -> Self {Self {previous: String::new()}}
    pub async fn next(&mut self) -> PushToken {
        let (operation, activity, vm) = Bridge::with(|env, activity| {
            let previous = env.new_string(&self.previous)?;
            let reference = env.new_global_ref(&activity)?;
            let operation = env.call_method(activity, "beginPush", "(Ljava/lang/String;)J",
                &[JValue::Object(&previous)])?.j()?;
            Ok((operation, reference, env.get_java_vm()?))
        });
        let _wait = PushWait {id: (operation >> 32) as i32, activity: Some(activity), vm};
        let fd = unsafe {OwnedFd::from_raw_fd(operation as i32)};
        let stream = std::os::unix::net::UnixStream::from(fd);
        stream.set_nonblocking(true).unwrap();
        let mut stream = tokio::net::UnixStream::from_std(stream).unwrap();
        let mut token = String::new();
        match stream.read_to_string(&mut token).await {
            Ok(_) if !token.is_empty() => {
                self.previous = token.clone();
                PushToken::Fcm(token)
            }
            _ => std::future::pending().await,
        }
    }
    pub fn notify(_proxy: EventLoopProxy<Output>, header: String, body: String) {
        Bridge::with(|env, activity| {
            let header = env.new_string(header)?;
            let body = env.new_string(body)?;
            env.call_method(activity, "notifyUser", "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&header), JValue::Object(&body)]).map(|_| ())
        });
    }
}

pub struct Share;
impl Share {
    pub fn new() -> Self {Self}
    pub fn share(&self, data: &Data) {Bridge::send("share", data);}
}

pub struct DataPicker {
    tx: AsyncTx<spsc::List<PickRequest>>,
    changed: Arc<Notify>,
    worker: tokio::task::JoinHandle<()>,
}
impl DataPicker {
    pub fn new() -> Self {
        let (tx, rx): (AsyncTx<spsc::List<PickRequest>>, crossfire::AsyncRx<_>) =
            spsc::build(spsc::List::new());
        let changed = Arc::new(Notify::new());
        let cancel = changed.clone();
        let worker = RUNTIME.get().unwrap().block_on(async move {
            tokio::spawn(async move {
                while let Ok(request) = rx.recv().await {Self::select(request, &cancel).await;}
            })
        });
        Self {tx, changed, worker}
    }
    pub fn pick<P: Pickable>(&self, hardware: &Hardware) -> Picked<P> {
        let (tx, rx): (AsyncTx<spsc::One<PickResult>>, _) = spsc::build(spsc::One::new());
        self.tx.try_send(P::request(tx)).unwrap();
        Picked::new(rx, hardware)
    }
    pub fn cancel(&self) {self.changed.notify_one();}

    async fn select(mut request: PickRequest, changed: &Notify) {
        if request.disconnected() {return}
        let mime = match request.kind() {
            PickKind::Image => "image/*",
            PickKind::Video => "video/*",
            PickKind::Audio => "audio/*",
            PickKind::Visual => "image/*,video/*",
            PickKind::Media => "audio/*,video/*",
            PickKind::File | PickKind::Data => "*/*",
        };
        let mut title = "Choose files".to_owned();
        loop {
            if request.disconnected() {return}
            let operation = Bridge::with(|env, activity| {
                let mime = env.new_string(mime)?;
                let title = env.new_string(&title)?;
                env.call_method(activity, "beginPick", "(Ljava/lang/String;ZZLjava/lang/String;)J",
                    &[JValue::Object(&mime), JValue::Bool(request.multiple().into()),
                      JValue::Bool(matches!(request.kind(), PickKind::File).into()), JValue::Object(&title)])?.j()
            });
            let selection = Selection((operation >> 32) as i32);
            let fd = unsafe {OwnedFd::from_raw_fd(operation as i32)};
            let stream = std::os::unix::net::UnixStream::from(fd);
            stream.set_nonblocking(true).unwrap();
            let mut stream = tokio::net::UnixStream::from_std(stream).unwrap();
            let mut bytes = Vec::new();
            let read = stream.read_to_end(&mut bytes);
            tokio::pin!(read);
            loop {
                tokio::select! {
                    biased;
                    _ = changed.notified() => if request.disconnected() {return},
                    result = &mut read => {
                        if let Err(error) = result {
                            log::warn!("Android picker closed: {error}");
                            request.cancel();
                            return
                        }
                        break
                    }
                }
            }
            drop(selection);
            if request.disconnected() {return}
            if bytes.is_empty() {request.cancel(); return}
            let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            if result["cancelled"].as_bool() == Some(true) {request.cancel(); return}
            let items = result["items"].as_array().unwrap().iter()
                .map(|item| Bridge::load(request.kind(), item)).collect();
            match request.accept(items) {
                None => {request.finish(); return}
                Some(error) => {log::warn!("{error}"); title = error;}
            }
        }
    }
}
impl Drop for DataPicker {
    fn drop(&mut self) {
        self.worker.abort();
        RUNTIME.get().unwrap().block_on(async {
            if let Err(error) = (&mut self.worker).await {
                assert!(error.is_cancelled(), "Picker failed: {error}");
            }
        });
    }
}

pub struct AudioSession {
    activity: GlobalRef,
    vm: JavaVM,
}
impl AudioSession {
    pub fn new() -> Result<Self, String> {
        Bridge::with(|env, activity| {
            let error = env.call_method(&activity, "acquireAudio", "()Ljava/lang/String;", &[])?.l()?;
            if !error.is_null() {
                return Ok(Err(String::from(env.get_string(&JString::from(error))?)))
            }
            Ok(Ok(Self {activity: env.new_global_ref(activity)?, vm: env.get_java_vm()?}))
        })
    }
}
impl Drop for AudioSession {
    fn drop(&mut self) {
        let mut env = self.vm.attach_current_thread().unwrap();
        env.call_method(self.activity.as_obj(), "releaseAudio", "()V", &[]).unwrap();
    }
}
pub struct MediaCommands;
impl MediaCommands {
    pub fn update(id: u128, playing: bool) {Self::publish(id, if playing {1} else {0});}
    pub fn remove(id: u128) {Self::publish(id, -1);}
    fn publish(id: u128, state: i32) {
        Bridge::with(|env, activity| {
            let id = env.new_string(format!("{id:032x}"))?;
            env.call_method(activity, "playback", "(Ljava/lang/String;I)V",
                &[JValue::Object(&id), JValue::Int(state)]).map(|_| ())
        });
    }
}
pub struct CaptureHub {
    feeds: Arc<Mutex<HashMap<u128, Feed>>>,
    session: CaptureSession,
}
impl CaptureHub {
    pub fn new(_hardware: &Hardware) -> Self {
        let feeds = Arc::new(Mutex::new(HashMap::new()));
        Self {session: CaptureSession::new(feeds.clone()), feeds}
    }
    pub fn camera(&self, hardware: &Hardware) -> Result<Camera, String> {
        let (id, frames) = self.subscribe(true, false);
        Ok(Camera::new(hardware, id, frames))
    }
    pub fn microphone(&self, hardware: &Hardware) -> Result<Microphone, String> {
        let (id, frames) = self.subscribe(false, true);
        Ok(Microphone::new(hardware, id, frames))
    }
    pub fn capture(&self, hardware: &Hardware) -> Result<Capture, String> {
        let (id, frames) = self.subscribe(true, true);
        Ok(Capture::new(hardware, id, frames))
    }
    pub fn error(&self, id: u128) -> Option<String> {
        self.feeds.lock().unwrap().get(&id).and_then(|feed| feed.error())
    }
    pub fn remove(&self, id: u128) {
        self.feeds.lock().unwrap().remove(&id).unwrap();
        self.configure();
    }
    fn subscribe(&self, camera: bool, microphone: bool) -> (u128, broadcast::Receiver<(Duration, Arc<Frame>)>) {
        let id = rand::random();
        let (feed, frames) = Feed::new(camera, microphone);
        self.feeds.lock().unwrap().insert(id, feed);
        self.configure();
        (id, frames)
    }
    fn configure(&self) {
        let (camera, microphone) = {
            let feeds = self.feeds.lock().unwrap();
            let active = feeds.values().filter(|feed| feed.is_open());
            (active.clone().any(|feed| feed.camera()), active.clone().any(|feed| feed.microphone()))
        };
        let mut env = self.session.vm.attach_current_thread().unwrap();
        env.call_method(self.session.object.as_obj(), "configure", "(ZZ)V", &[
            JValue::Bool(camera.into()), JValue::Bool(microphone.into()),
        ]).unwrap();
    }
}
impl Drop for CaptureHub {
    fn drop(&mut self) {self.feeds.lock().unwrap().clear();}
}

struct Keyboards {
    id: Cell<Option<i32>>,
    inset: Arc<KeyboardInset>,
}
impl Keyboards {
    extern "system" fn text(mut env: JNIEnv, activity: JObject, text: JString) {
        let proxy = Events::proxy(&mut env, activity);
        let text = String::from(env.get_string(&text).unwrap());
        AppEvents::text(&proxy, &text);
    }

    extern "system" fn key(mut env: JNIEnv, activity: JObject, code: i32, unicode: i32, action: i32, repeat: i32, meta: i32) -> jni::sys::jboolean {
        let key = match code {
            19 => Key::Up, 20 => Key::Down, 21 => Key::Left, 22 => Key::Right,
            61 => Key::Tab, 66 => Key::Enter, 67 => Key::Backspace, 112 => Key::Delete,
            111 => Key::Escape, 122 => Key::Home, 123 => Key::End,
            59 | 60 => Key::Shift, 113 | 114 => Key::Control,
            57 | 58 => Key::Alt, 117 | 118 => Key::SuperMeta,
            _ => match char::from_u32(unicode as u32).filter(|c| !c.is_control()) {
                Some(character) => Key::Character(character),
                None => return 0,
            },
        };
        let state = match action {
            0 if repeat > 0 => KeyboardState::Repeated,
            0 => KeyboardState::Pressed,
            1 => KeyboardState::Released,
            _ => return 0,
        };
        let proxy = Events::proxy(&mut env, activity);
        let _ = proxy.send_event(Output::Input(Input::Device(
            u64::MAX - 1, DeviceInput::Keyboard(key, state, Modifiers {
                shift: meta & 1 != 0, control: meta & 0x1000 != 0,
                alt: meta & 2 != 0, supermeta: meta & 0x10000 != 0,
            }),
        )));
        1
    }
}

impl Drop for Keyboards {
    fn drop(&mut self) {
        if let Some(id) = self.id.get() {
            Bridge::with(|env, activity| {
                env.call_method(activity, "closeKeyboard", "(I)V", &[JValue::Int(id)]).map(|_| ())
            });
        }
    }
}

struct CaptureSession {
    object: GlobalRef,
    vm: JavaVM,
}
impl CaptureSession {
    fn new(feeds: Arc<Mutex<HashMap<u128, Feed>>>) -> Self {
        Bridge::with(|env, activity| {
            let object = env.call_method(activity, "createCapture", "()Lme/ramp/maverick/MaverickCapture;", &[])?.l()?;
            let class = env.get_object_class(&object)?;
            env.register_native_methods(class, &[
                NativeMethod {
                    name: "video".into(),
                    sig: "(Ljava/nio/ByteBuffer;Ljava/nio/ByteBuffer;Ljava/nio/ByteBuffer;IIIIIJJI)V".into(),
                    fn_ptr: Self::video as *mut std::ffi::c_void,
                },
                NativeMethod {
                    name: "audio".into(), sig: "([FIIJ)V".into(),
                    fn_ptr: Self::audio as *mut std::ffi::c_void,
                },
                NativeMethod {
                    name: "failed".into(), sig: "(ZLjava/lang/String;)V".into(),
                    fn_ptr: Self::failed as *mut std::ffi::c_void,
                },
            ])?;
            unsafe {env.set_rust_field(&object, "nativeState", feeds)?;}
            Ok(Self {object: env.new_global_ref(object)?, vm: env.get_java_vm()?})
        })
    }
    extern "system" fn video(
        mut env: JNIEnv, object: JObject, y: JByteBuffer, u: JByteBuffer, v: JByteBuffer,
        y_stride: i32, uv_stride: i32, uv_pixel: i32, width: i32, height: i32,
        timestamp: i64, duration: i64, rotation: i32,
    ) {
        let y = unsafe {std::slice::from_raw_parts(
            env.get_direct_buffer_address(&y).unwrap(), env.get_direct_buffer_capacity(&y).unwrap(),
        )};
        let u = unsafe {std::slice::from_raw_parts(
            env.get_direct_buffer_address(&u).unwrap(), env.get_direct_buffer_capacity(&u).unwrap(),
        )};
        let v = unsafe {std::slice::from_raw_parts(
            env.get_direct_buffer_address(&v).unwrap(), env.get_direct_buffer_capacity(&v).unwrap(),
        )};
        let (width, height) = (width as usize, height as usize);
        let (out_width, out_height) = if rotation % 180 == 0 {(width, height)} else {(height, width)};
        let mut pixels = vec![Pixel::default(); width * height];
        for row in 0..height {
            for column in 0..width {
                let yi = row * y_stride as usize + column;
                let uvi = row / 2 * uv_stride as usize + column / 2 * uv_pixel as usize;
                let y = (y[yi] as i32 - 16).max(0);
                let u = u[uvi] as i32 - 128;
                let v = v[uvi] as i32 - 128;
                let (x, y_out) = match rotation {
                    0 => (column, row), 90 => (height - row - 1, column),
                    180 => (width - column - 1, height - row - 1),
                    270 => (row, width - column - 1), _ => unreachable!(),
                };
                pixels[y_out * out_width + x] = Pixel {
                    red: ((298 * y + 409 * v + 128) >> 8).clamp(0, 255) as u8,
                    green: ((298 * y - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8,
                    blue: ((298 * y + 516 * u + 128) >> 8).clamp(0, 255) as u8, alpha: 255,
                };
            }
        }
        let frame = Frame::Image(Image::new((out_width as u32, out_height as u32), pixels),
            Duration::from_nanos(duration as u64));
        Self::send(&mut env, object, timestamp, frame);
    }
    extern "system" fn audio(mut env: JNIEnv, object: JObject, samples: JFloatArray, length: i32, rate: i32, timestamp: i64) {
        let mut values = vec![0.0; length as usize];
        env.get_float_array_region(samples, 0, &mut values).unwrap();
        let sound = Sound {channels: 1, samples: values.into_iter().map(Sample).collect()};
        let duration = Duration::from_secs_f64(length as f64 / rate as f64);
        Self::send(&mut env, object, timestamp, Frame::Sound(sound, duration));
    }
    extern "system" fn failed(mut env: JNIEnv, object: JObject, camera: jni::sys::jboolean, error: JString) {
        let error = String::from(env.get_string(&error).unwrap());
        let feeds = unsafe {
            env.get_rust_field::<_, _, Arc<Mutex<HashMap<u128, Feed>>>>(object, "nativeState").unwrap()
        };
        for feed in feeds.lock().unwrap().values_mut() {
            if if camera != 0 {feed.camera()} else {feed.microphone()} {feed.fail(error.clone());}
        }
    }
    fn send(env: &mut JNIEnv, object: JObject, timestamp: i64, frame: Frame) {
        let feeds = unsafe {
            env.get_rust_field::<_, _, Arc<Mutex<HashMap<u128, Feed>>>>(object, "nativeState").unwrap()
        };
        let frame = Arc::new(frame);
        for feed in feeds.lock().unwrap().values_mut() {
            if !feed.started() {feed.start(timestamp as f64 / 1e9);}
            feed.send(timestamp as f64 / 1e9, &frame);
        }
    }
}
impl Drop for CaptureSession {
    fn drop(&mut self) {
        let mut env = self.vm.attach_current_thread().unwrap();
        let object = self.object.as_obj();
        {
            let _monitor = env.lock_obj(object).unwrap();
            unsafe {
                env.take_rust_field::<_, _, Arc<Mutex<HashMap<u128, Feed>>>>(object, "nativeState").unwrap();
            }
        }
        env.call_method(object, "close", "()V", &[]).unwrap();
    }
}

struct Selection(i32);
impl Drop for Selection {
    fn drop(&mut self) {
        Bridge::with(|env, activity| {
            env.call_method(activity, "cancelPick", "(I)V", &[JValue::Int(self.0)]).map(|_| ())
        });
    }
}

struct PushWait {
    id: i32,
    activity: Option<GlobalRef>,
    vm: JavaVM,
}
impl Drop for PushWait {
    fn drop(&mut self) {
        let mut env = self.vm.attach_current_thread().unwrap();
        let activity = self.activity.take().unwrap();
        env.call_method(activity.as_obj(), "cancelPush", "(I)V", &[JValue::Int(self.id)]).unwrap();
    }
}

struct Bridge;
impl Bridge {
    fn with<T>(action: impl FnOnce(&mut JNIEnv, JObject) -> jni::errors::Result<T>) -> T {
        let context = ndk_context::android_context();
        let vm = unsafe {JavaVM::from_raw(context.vm().cast()).unwrap()};
        let mut env = vm.attach_current_thread().unwrap();
        let result = env.with_local_frame(32, |env| {
            // android-activity gives ndk-context an Application, not its Activity.
            let application = unsafe {JObject::from_raw(context.context().cast())};
            let loader = env.call_method(application, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?.l()?;
            let name = env.new_string("me.ramp.maverick.MaverickActivity")?;
            let class = env.call_method(loader, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)])?.l()?;
            let activity = env.call_static_method(JClass::from(class), "current",
                "()Lme/ramp/maverick/MaverickActivity;", &[])?.l()?;
            action(env, activity)
        });
        if env.exception_check().unwrap() {
            env.exception_describe().unwrap();
            env.exception_clear().unwrap();
        }
        result.expect("Maverick Android activity integration failed")
    }

    fn send(method: &str, data: &Data) {
        let temporary = match data {
            Data::Image(image) => {
                let path = ApplicationSupport::get(String::new())
                    .join(format!("{:032x}.png", rand::random::<u128>()));
                let file = std::fs::File::create(&path).unwrap();
                image.write(file, super::ImageFormat::Png).unwrap();
                Some(path)
            }
            Data::Media(media) => {
                let format = if media.video.dimensions().is_some() {
                    Format::Mkv_Ffv1_PcmF32le
                } else {Format::Wav_PcmF32le};
                let path = ApplicationSupport::get(String::new())
                    .join(format!("{:032x}.{}", rand::random::<u128>(), format.extension()));
                let result = std::fs::File::create(&path).map_err(|error| error.to_string())
                    .and_then(|file| media.write(file, format));
                if let Err(error) = result {
                    let _ = std::fs::remove_file(path);
                    log::warn!("Media export failed: {error}");
                    return
                }
                Some(path)
            }
            _ => None,
        };
        let (kind, value) = match data {
            Data::Text(text) => ("text/plain", text.clone()),
            Data::Image(_) => ("image/png", temporary.as_ref().unwrap().to_string_lossy().into_owned()),
            Data::File(path) => ("application/octet-stream", path.to_string_lossy().into_owned()),
            Data::Media(media) => (
                if media.video.dimensions().is_some() {"video/x-matroska"} else {"audio/wav"},
                temporary.as_ref().unwrap().to_string_lossy().into_owned(),
            ),
        };
        Self::with(|env, activity| {
            let kind = env.new_string(kind)?;
            let value = env.new_string(value)?;
            env.call_method(activity, method, "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&kind), JValue::Object(&value)]).map(|_| ())
        });
        if let Some(path) = temporary {std::fs::remove_file(path).unwrap();}
    }

    fn load(kind: PickKind, value: &serde_json::Value) -> Result<Data, String> {
        if let Some(error) = value["error"].as_str() {return Err(error.to_owned())}
        let path = PathBuf::from(value["path"].as_str().unwrap());
        if matches!(kind, PickKind::File) {return Ok(Data::File(path))}
        if value["image"].as_bool() == Some(true) || matches!(kind, PickKind::Image) {
            let result = std::fs::File::open(&path).map_err(|error| error.to_string())
                .and_then(Image::read).map(Data::Image);
            let _ = std::fs::remove_file(path);
            result
        } else if value["media"].as_bool() == Some(true)
            || kind.media() && !matches!(kind, PickKind::Data)
        {
            let result = std::fs::File::open(&path).map_err(|error| error.to_string())
                .and_then(Media::read).map(Data::Media);
            let _ = std::fs::remove_file(path);
            result
        } else {Ok(Data::File(path))}
    }
}
