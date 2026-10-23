use crossfire::{spsc, AsyncTx};
use jni::{JNIEnv, JavaVM, NativeMethod, objects::{GlobalRef, JClass, JObject, JString, JValue, JFloatArray}};
use tokio::io::AsyncReadExt;
use tokio::sync::Notify;
use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use super::{Data, Hardware, Image, Media, Pickable, Picked, PickResult, PushDestination};
use super::picker::{PickKind, PickRequest};
use crate::events::{Events as AppEvents, Output};
use crate::inputs::{Input, DeviceInput, Key, KeyboardState, Modifiers};
use crate::runtime::RUNTIME;

pub struct SafeArea;
impl SafeArea {
    pub fn touch_controls() -> bool {true}
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
}
impl Events {
    pub fn new(_window: &crate::Window, proxy: EventLoopProxy<Output>) -> Self {
        Bridge::with(|env, activity| {
            let class = env.get_object_class(&activity)?;
            env.register_native_methods(class, &[
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
            let owner = Self {activity, vm};
            env.call_method(owner.activity.as_obj(), "observeEvents", "()V", &[])?;
            Ok(owner)
        })
    }
    pub fn process(&mut self, event: WindowEvent, _scale: f64) -> Option<WindowEvent> {Some(event)}
    pub fn suspend(&mut self) {}
    pub async fn next(&mut self) -> Input {std::future::pending().await}

    fn proxy(env: &mut JNIEnv, activity: JObject) -> EventLoopProxy<Output> {
        unsafe {env.get_rust_field::<_, _, EventLoopProxy<Output>>(activity, "events").unwrap().clone()}
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
pub struct Keyboard(Rc<i32>);
impl Drop for Keyboard {
    fn drop(&mut self) {
        if Rc::strong_count(&self.0) != 1 {return;}
        Bridge::with(|env, activity| {
            env.call_method(activity, "closeKeyboard", "(I)V", &[JValue::Int(*self.0)]).map(|_| ())
        });
    }
}

#[derive(Default)]
pub struct Keyboards(RefCell<Weak<i32>>);
impl Keyboards {
    pub fn open(&self, _proxy: EventLoopProxy<Output>) -> Keyboard {
        let mut current = self.0.borrow_mut();
        if let Some(id) = current.upgrade() {return Keyboard(id);}
        let id = Rc::new(Bridge::with(|env, activity| {
            env.call_method(activity, "openKeyboard", "()I", &[])?.i()
        }));
        *current = Rc::downgrade(&id);
        Keyboard(id)
    }

    extern "system" fn text(mut env: JNIEnv, activity: JObject, text: JString) {
        let proxy = Events::proxy(&mut env, activity);
        let text = String::from(env.get_string(&text).unwrap());
        AppEvents::text(&proxy, &text);
    }

    extern "system" fn key(
        mut env: JNIEnv, activity: JObject, code: i32, unicode: i32, action: i32, repeat: i32, meta: i32,
    ) -> jni::sys::jboolean {
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

pub struct Push;
impl Push {
    pub async fn current(previous: Option<&PushDestination>) -> PushDestination {
        let (operation, activity, vm) = Bridge::with(|env, activity| {
            let previous = match previous {Some(PushDestination::Fcm(token)) => token.as_str(), _ => ""};
            let previous = env.new_string(previous)?;
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
            Ok(_) if !token.is_empty() => PushDestination::Fcm(token),
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

pub struct AudioSession;
impl AudioSession {
    pub fn new() -> Result<Self, String> {Ok(Self)}
}
pub struct MediaCommands;
impl MediaCommands {
    pub fn update() {}
}
#[derive(Clone)]
pub struct Camera(std::convert::Infallible);
impl Camera {
    pub fn error(&self) -> Option<String> {match self.0 {}}
}
impl AsRef<Media> for Camera {
    fn as_ref(&self) -> &Media {match self.0 {}}
}

#[derive(Clone)]
pub struct Microphone(std::convert::Infallible);
impl Microphone {
    pub fn error(&self) -> Option<String> {match self.0 {}}
}
impl AsRef<Media> for Microphone {
    fn as_ref(&self) -> &Media {match self.0 {}}
}

#[derive(Clone)]
pub struct Capture(std::convert::Infallible);
impl Capture {
    pub fn error(&self) -> Option<String> {match self.0 {}}
}
impl AsRef<Media> for Capture {
    fn as_ref(&self) -> &Media {match self.0 {}}
}

pub struct CaptureHub;
impl CaptureHub {
    pub fn new(_hardware: &Hardware) -> Self {Self}
    pub fn camera(&self) -> Result<Camera, String> {
        Err("Android video capture is not implemented".into())
    }
    pub fn microphone(&self) -> Result<Microphone, String> {
        Err("Android audio capture is not implemented".into())
    }
    pub fn capture(&self) -> Result<Capture, String> {
        Err("Android video capture is not implemented".into())
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
                image.save_with_format(&path, image::ImageFormat::Png).unwrap();
                Some(path)
            }
            _ => None,
        };
        let (kind, value) = match data {
            Data::Text(text) => ("text/plain", text.clone()),
            Data::Image(_) => ("image/png", temporary.as_ref().unwrap().to_string_lossy().into_owned()),
            Data::File(path) => ("application/octet-stream", path.to_string_lossy().into_owned()),
            Data::Media(_) => {
                log::warn!("Android audio/video sharing and clipboard are not implemented");
                return
            }
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
        } else if kind.media() && !matches!(kind, PickKind::Data) {
            let result = std::fs::File::open(&path).map_err(|error| error.to_string())
                .and_then(Media::read).map(Data::Media);
            let _ = std::fs::remove_file(path);
            result
        } else {Ok(Data::File(path))}
    }
}
