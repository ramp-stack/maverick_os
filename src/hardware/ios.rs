use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, NamedKey};
use crossfire::{spsc, AsyncRx, AsyncTx};
use futures_timer::Delay;

use block2::RcBlock;
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::{
    AnyThread, MainThreadOnly, MainThreadMarker, extern_class, extern_methods,
    define_class, msg_send, sel, DefinedClass, Message
};
use objc2_ui_kit::UIApplicationDelegate;
use objc2_foundation::{
    NSBundle, NSError,
    NSString, NSArray, NSSet, NSData, NSNotification, NSNotificationCenter, NSObject,
    NSObjectProtocol, NSNumber, NSValue, NSURL, NSDictionary,
    NSObjectNSKeyValueObserverRegistration, NSKeyValueObservingOptions,
    NSKeyValueChangeOldKey, NSKeyValueChangeNewKey,
};
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryPlayback, AVAudioSessionCategoryPlayAndRecord, AVAudioSessionSetActiveOptions,
    AVAudioSessionRouteChangeNotification, AVAudioSessionRouteChangeReasonKey,
    AVAudioSessionRouteChangeReason, AVAudioSessionInterruptionNotification,
    AVAudioSessionInterruptionTypeKey, AVAudioSessionInterruptionType,
};
use objc2_core_foundation::CGRect;
use objc2_quartz_core::CATransaction;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::rc::{Retained, Allocated, Weak};
use objc2_ui_kit::{
    UIPasteboard, UIApplication, UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UIImage,
    UIViewController, UIViewAutoresizing, UIWindow, UIWindowScene, UISceneActivationState, UIScene,
    UIActivityViewController, UIResponder, UIDocumentPickerViewController,
    UIDocumentPickerDelegate, UIApplicationState, UIApplicationDidBecomeActiveNotification,
    UIWindowDidBecomeKeyNotification, UIWindowDidBecomeVisibleNotification,
    UIViewControllerTransitionCoordinator, UIViewControllerTransitionCoordinatorContext,
    UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
    UIAdaptivePresentationControllerDelegate, UIPresentationController,
    UITextView, UITextViewDelegate, UIScrollViewDelegate, UITextInput, NSTextContainer,
    UITextAutocorrectionType, UITextSpellCheckingType,
    NSObjectUIAccessibility, NSValueUIGeometryExtensions,
    UIKeyboardDidShowNotification, UIKeyboardDidHideNotification, UIKeyboardDidChangeFrameNotification,
    UIKeyboardWillChangeFrameNotification, UIKeyboardAnimationDurationUserInfoKey,
    UIKeyboardFrameEndUserInfoKey, UIKeyboardIsLocalUserInfoKey,
};
use objc2_photos_ui::{
    PHPickerConfiguration, PHPickerResult,
    PHPickerViewControllerDelegate,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use objc2_ui_kit::UIView;
// objc2 marks storyboard APIs deprecated for visionOS; they remain the iOS launch-screen API.
#[allow(deprecated)]
use objc2_ui_kit::UIStoryboard;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::ptr::NonNull;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::hardware::{
    Data, Hardware, Image, Media, Pickable, Picked, PickResult, PushDestination,
};
use super::apple::{AudioType, MovieType, PickRequest, PickState};
use super::apple::MediaCommands;
use crate::inputs::{Input, Media as MediaInput, DeviceInput, MouseState};
use crate::events::{Events as AppEvents, Output};

static AUDIO_SESSIONS: Mutex<(usize, usize)> = Mutex::new((0, 0));
thread_local! {static PUSH: _Push = _Push::new();}

#[derive(Clone)]
pub struct Keyboard(Rc<Retained<KeyboardView>>);
impl Drop for Keyboard {
    fn drop(&mut self) {
        if Rc::strong_count(&self.0) != 1 {return;}
        self.0.ivars().active.set(false);
        self.0.resignFirstResponder();
        unsafe {self.0.setDelegate(None);}
        self.0.removeFromSuperview();
    }
}

#[derive(Default)]
pub struct Keyboards(RefCell<std::rc::Weak<Retained<KeyboardView>>>);
impl Keyboards {
    pub fn open(&self, proxy: EventLoopProxy<Output>) -> Keyboard {
        let mut current = self.0.borrow_mut();
        if let Some(view) = current.upgrade() {return Keyboard(view);}
        let mtm = MainThreadMarker::new().unwrap();
        let view = KeyboardView::alloc(mtm).set_ivars(KeyboardIvars {
            active: Cell::new(true), flushing: Cell::new(false), proxy,
        });
        let view: Retained<KeyboardView> = unsafe {
            msg_send![super(view), initWithFrame: CGRect::ZERO, textContainer: None::<&NSTextContainer>]
        };
        unsafe {view.setDelegate(Some(ProtocolObject::from_ref(&*view)));}
        view.setAccessibilityElementsHidden(true, mtm);
        SafeArea::window().addSubview(&view);
        view.becomeFirstResponder();
        let view = Rc::new(view);
        *current = Rc::downgrade(&view);
        Keyboard(view)
    }
}

pub struct SafeArea;
impl SafeArea {
    pub fn touch_controls() -> bool {true}
    pub fn physical(window: &winit::window::Window) -> crate::PhysicalWindow {
        // Winit's iOS inner size excludes safe-area insets; rendering and touches do not.
        let size = window.outer_size();
        crate::PhysicalWindow {
            width: size.width, height: size.height, scale_factor: window.scale_factor(),
        }
    }
    pub fn get(window: &winit::window::Window) -> (f32, f32, f32, f32) {
        let RawWindowHandle::UiKit(handle) = window.window_handle().unwrap().as_raw() else {
            unreachable!()
        };
        let view = unsafe {handle.ui_view.cast::<UIView>().as_ref()};
        let insets = view.safeAreaInsets();
        (insets.top as f32, insets.left as f32, insets.bottom as f32, insets.right as f32)
    }
    fn window() -> Retained<UIWindow> {
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
}

pub struct LaunchScreen(Option<Retained<UIViewController>>);
impl LaunchScreen {
    #[allow(deprecated)]
    pub fn new(window: &crate::Window) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let bundle = NSBundle::mainBundle();
        let controller = bundle.objectForInfoDictionaryKey(&NSString::from_str("UILaunchStoryboardName"))
            .and_then(|name| name.downcast::<NSString>().ok())
            .and_then(|name| {
                UIStoryboard::storyboardWithName_bundle(&name, Some(&bundle), mtm)
                    .instantiateInitialViewController()
            });
        if let Some(controller) = &controller {
            let RawWindowHandle::UiKit(handle) = window.window_handle().unwrap().as_raw() else {
                unreachable!()
            };
            let parent = unsafe {handle.ui_view.cast::<UIView>().as_ref()};
            let root = parent.window().unwrap().rootViewController().unwrap();
            root.addChildViewController(controller);
            let view = controller.view().expect("Launch storyboard has no view");
            view.setFrame(parent.bounds());
            view.setAutoresizingMask(UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight);
            view.setUserInteractionEnabled(false);
            view.setAccessibilityElementsHidden(true, mtm);
            // Keep the launch view above the Metal layer added during renderer initialization.
            view.layer().setZPosition(1.0);
            parent.addSubview(&view);
            view.layoutIfNeeded();
            controller.didMoveToParentViewController(Some(&root));
            // Commit the splash before synchronous application and renderer initialization.
            CATransaction::flush();
        }
        Self(controller)
    }

    pub fn dismiss(&mut self) {
        if let Some(controller) = self.0.take() {
            controller.willMoveToParentViewController(None);
            if let Some(view) = controller.view() {view.removeFromSuperview();}
            controller.removeFromParentViewController();
        }
    }
}
impl Drop for LaunchScreen {
    fn drop(&mut self) {self.dismiss();}
}

pub struct KeyboardInset {
    view: MainThreadBound<Retained<UIView>>,
    visible: MainThreadBound<Rc<Cell<bool>>>,
    observers: MainThreadBound<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}
impl KeyboardInset {
    pub fn new(window: &winit::window::Window, proxy: EventLoopProxy<Output>) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let RawWindowHandle::UiKit(handle) = window.window_handle().unwrap().as_raw() else {
            unreachable!()
        };
        let parent = unsafe {handle.ui_view.cast::<UIView>().as_ref()};
        let guide = parent.keyboardLayoutGuide();
        guide.setUsesBottomSafeArea(false);
        let view = UIView::initWithFrame(UIView::alloc(mtm), CGRect::ZERO);
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        view.setUserInteractionEnabled(false);
        view.setAccessibilityElementsHidden(true, mtm);
        parent.addSubview(&view);
        view.topAnchor().constraintEqualToAnchor(&guide.topAnchor()).setActive(true);
        view.bottomAnchor().constraintEqualToAnchor(&parent.bottomAnchor()).setActive(true);
        view.leadingAnchor().constraintEqualToAnchor(&parent.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&parent.trailingAnchor()).setActive(true);
        parent.layoutIfNeeded();
        let visible = Rc::new(Cell::new(false));
        let center = NSNotificationCenter::defaultCenter();
        let mut observers = Vec::new();
        for (name, hidden, finished) in unsafe {[
            (UIKeyboardDidShowNotification, false, true),
            (UIKeyboardDidChangeFrameNotification, false, true),
            (UIKeyboardDidHideNotification, true, true),
            (UIKeyboardWillChangeFrameNotification, false, false),
        ]} {
            let visible = visible.clone();
            let parent = parent.retain();
            let proxy = proxy.clone();
            let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
                let notification = unsafe {notification.as_ref()};
                let Some(showing) = Self::visibility(notification, hidden, &parent) else {return};
                if finished || showing {visible.set(showing);}
                let _ = proxy.send_event(Output::GeometryChanged);
            });
            observers.push(unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            });
        }
        Self {
            view: MainThreadBound::new(view, mtm), visible: MainThreadBound::new(visible, mtm),
            observers: MainThreadBound::new(observers, mtm),
        }
    }

    pub fn get(&self) -> (f32, bool) {
        let mtm = MainThreadMarker::new().unwrap();
        let view = self.view.get(mtm);
        let parent = view.superview().unwrap();
        let layer = view.layer();
        // The layout frame is the animation's destination; the presentation layer is onscreen now.
        let frame = unsafe {layer.presentationLayer()}.unwrap_or(layer).frame();
        let overlap = frame.size.height.max(0.0).min(parent.bounds().size.height) as f32;
        (overlap, self.visible.get(mtm).get() || overlap > 0.0)
    }

    fn visibility(notification: &NSNotification, hidden: bool, view: &UIView) -> Option<bool> {
        let info = notification.userInfo()?;
        if info.objectForKey(unsafe {UIKeyboardIsLocalUserInfoKey})
            .and_then(|value| value.downcast::<NSNumber>().ok()).is_some_and(|value| !value.boolValue()) {
            return None
        }
        if hidden {return Some(false)}
        let frame = info.objectForKey(unsafe {UIKeyboardFrameEndUserInfoKey})?
            .downcast::<NSValue>().ok()?;
        let window = view.window()?;
        let frame = window.convertRect_fromWindow(unsafe {frame.CGRectValue()}, None);
        let area = window.bounds();
        let left = frame.origin.x.max(area.origin.x);
        let top = frame.origin.y.max(area.origin.y);
        let right = (frame.origin.x + frame.size.width).min(area.origin.x + area.size.width);
        let bottom = (frame.origin.y + frame.size.height).min(area.origin.y + area.size.height);
        Some(right > left && bottom > top)
    }
}
impl Drop for KeyboardInset {
    fn drop(&mut self) {
        dispatch2::run_on_main(|mtm| {
            let center = NSNotificationCenter::defaultCenter();
            for observer in self.observers.get(mtm) {unsafe {
                center.removeObserver(AsRef::<AnyObject>::as_ref(&**observer));
            }}
            self.view.get(mtm).removeFromSuperview();
        });
    }
}

pub struct AudioSession {
    recording: bool,
}
impl AudioSession {
    pub fn playback_state(_playing: Option<bool>) {}
    pub fn new() -> Result<Self, String> {Self::acquire(false)}
    pub fn recording() -> Result<Self, String> {Self::acquire(true)}
    pub fn configure_capture(session: &objc2_av_foundation::AVCaptureSession) {
        unsafe {session.setAutomaticallyConfiguresApplicationAudioSession(false);}
    }
    fn acquire(recording: bool) -> Result<Self, String> {
        let session = unsafe {AVAudioSession::sharedInstance()};
        let mut sessions = AUDIO_SESSIONS.lock().unwrap();
        unsafe {
            let category = if recording || sessions.1 > 0 {
                AVAudioSessionCategoryPlayAndRecord
            } else {AVAudioSessionCategoryPlayback}.unwrap();
            session.setCategory_error(category).map_err(|error| error.localizedDescription().to_string())?;
            session.setActive_error(true).map_err(|error| error.localizedDescription().to_string())?;
        }
        if recording {sessions.1 += 1;} else {sessions.0 += 1;}
        Ok(Self {recording})
    }
}
impl Drop for AudioSession {
    fn drop(&mut self) {
        let mut sessions = AUDIO_SESSIONS.lock().unwrap();
        if self.recording {sessions.1 -= 1;} else {sessions.0 -= 1;}
        let result = unsafe {
            let session = AVAudioSession::sharedInstance();
            if sessions.0 == 0 && sessions.1 == 0 {
                session.setActive_withOptions_error(
                    false, AVAudioSessionSetActiveOptions::NotifyOthersOnDeactivation,
                )
            } else {
                let category = if sessions.1 > 0 {
                    AVAudioSessionCategoryPlayAndRecord
                } else {AVAudioSessionCategoryPlayback}.unwrap();
                session.setCategory_error(category)
            }
        };
        if let Err(error) = result {log::warn!("Could not release audio session: {error}");}
    }
}

pub struct Events {
    window: crate::Window,
    touches: usize,
    samples: VecDeque<(Instant, (f32, f32))>,
    position: (f32, f32),
    dragged: bool,
    velocity: (f32, f32),
    updated: Option<Instant>,
    animation: Rc<Cell<Option<(Instant, Instant)>>>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    volume: Retained<VolumeObserver>,
    _commands: MediaCommands,
}
impl Events {
    pub fn new(window: &crate::Window, proxy: EventLoopProxy<Output>) -> Self {
        let RawWindowHandle::UiKit(handle) = window.window_handle().unwrap().as_raw() else {
            unreachable!()
        };
        let view = unsafe {handle.ui_view.cast::<UIView>().as_ref()}.retain();
        let session = unsafe {AVAudioSession::sharedInstance()};
        let center = NSNotificationCenter::defaultCenter();
        let notifications = unsafe {[
            (AVAudioSessionRouteChangeNotification, AVAudioSessionRouteChangeReasonKey,
                AVAudioSessionRouteChangeReason::OldDeviceUnavailable.0),
            (AVAudioSessionInterruptionNotification, AVAudioSessionInterruptionTypeKey,
                AVAudioSessionInterruptionType::Began.0),
        ]};
        let mut observers = Vec::new();
        for (name, key, pause) in notifications {
            let proxy = proxy.clone();
            let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
                let info = unsafe {notification.as_ref()}.userInfo();
                let value = info.as_ref().and_then(|info| info.objectForKey(key.unwrap()))
                    .and_then(|value| value.downcast::<NSNumber>().ok());
                if value.is_some_and(|value| value.unsignedIntegerValue() == pause) {
                    let _ = proxy.send_event(Output::Input(Input::Media(MediaInput::Pause)));
                }
            });
            observers.push(unsafe {
                center.addObserverForName_object_queue_usingBlock(name, Some(&session), None, &block)
            });
        }
        let animation = Rc::new(Cell::new(None));
        let timing = animation.clone();
        let geometry = window.clone();
        let resize = proxy.clone();
        let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
            let notification = unsafe {notification.as_ref()};
            if KeyboardInset::visibility(notification, false, &view).is_none() {return;}
            let duration = notification.userInfo().unwrap()
                .objectForKey(unsafe {UIKeyboardAnimationDurationUserInfoKey}).unwrap()
                .downcast::<NSNumber>().unwrap().doubleValue();
            let now = Instant::now();
            let end = now + Duration::from_secs_f64(duration);
            timing.set(Some(((now + Duration::from_millis(16)).min(end), end)));
            let _ = resize.send_event(Output::Input(Input::Resized(geometry.logical())));
        });
        observers.push(unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(UIKeyboardWillChangeFrameNotification), None, None, &block,
            )
        });
        let volume = VolumeObserver::alloc().set_ivars(proxy.clone());
        let volume: Retained<VolumeObserver> = unsafe {msg_send![super(volume), init]};
        unsafe {
            session.addObserver_forKeyPath_options_context(
                &volume, &NSString::from_str("outputVolume"),
                NSKeyValueObservingOptions::Old | NSKeyValueObservingOptions::New,
                std::ptr::null_mut(),
            );
        }
        Self {
            window: window.clone(), animation, observers, volume, _commands: MediaCommands::new(proxy),
            touches: 0, samples: VecDeque::new(), position: (0.0, 0.0), dragged: false,
            velocity: (0.0, 0.0), updated: None,
        }
    }

    pub fn process(&mut self, event: WindowEvent, scale: f64) -> Option<WindowEvent> {
        match &event {
            WindowEvent::KeyboardInput {event, ..} if matches!(
                event.logical_key, Key::Named(NamedKey::AudioVolumeUp | NamedKey::AudioVolumeDown)
            ) => return None,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged {..}
                | WindowEvent::Focused(false) => self.reset_gesture(),
            WindowEvent::Touch(touch) => {
                let position = (
                    (touch.location.x / scale) as f32, (touch.location.y / scale) as f32
                );
                let now = Instant::now();
                match touch.phase {
                    TouchPhase::Started => {
                        self.touches += 1;
                        self.samples.clear();
                        self.dragged = false;
                        self.velocity = (0.0, 0.0);
                        self.updated = None;
                        if self.touches == 1 {
                            self.position = position;
                            self.sample(position, now);
                        }
                    }
                    TouchPhase::Moved if self.touches == 1 && !self.samples.is_empty() => {
                        self.sample(position, now);
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        if touch.phase == TouchPhase::Ended && self.touches == 1 && !self.samples.is_empty() {
                            self.sample(position, now);
                            let (time, first) = self.samples.front().unwrap();
                            let elapsed = now.duration_since(*time).as_secs_f32();
                            if elapsed >= 0.008 && self.dragged {
                                self.velocity = (
                                    (first.0 - position.0) / elapsed, (first.1 - position.1) / elapsed
                                );
                                if self.velocity.0.hypot(self.velocity.1) >= 30.0 {
                                    self.updated = Some(now);
                                }
                            }
                        }
                        if touch.phase == TouchPhase::Cancelled {
                            self.velocity = (0.0, 0.0);
                            self.updated = None;
                        }
                        self.touches = self.touches.saturating_sub(1);
                        self.samples.clear();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Some(event)
    }
    pub fn suspend(&mut self) {self.reset_gesture();}
    pub async fn next(&mut self) -> Input {
        loop {
            let momentum = self.updated.map(|updated| (updated + Duration::from_millis(16), false));
            let animation = self.animation.get().map(|(next, _)| (next, true));
            let Some((next, animation)) = [momentum, animation].into_iter().flatten().min() else {
                return std::future::pending().await
            };
            Delay::new(next.saturating_duration_since(Instant::now())).await;
            let now = Instant::now();
            if animation {
                let Some((next, end)) = self.animation.get() else {continue};
                if now < next {continue;}
                self.animation.set((now < end).then_some((
                    (now + Duration::from_millis(16)).min(end), end,
                )));
                return Input::Resized(self.window.logical())
            }
            let elapsed = now.duration_since(self.updated.unwrap()).as_secs_f32();
            let damping = 2.0;
            let remaining = (self.velocity.0.hypot(self.velocity.1) / 5.0).ln() / damping;
            let decay = (-damping * elapsed.min(remaining)).exp();
            let distance = (1.0 - decay) / damping;
            let delta = (self.velocity.0 * distance, self.velocity.1 * distance);
            self.velocity = (self.velocity.0 * decay, self.velocity.1 * decay);
            self.updated = (elapsed < remaining).then_some(now);
            return Input::Device(u64::MAX, DeviceInput::Mouse(
                self.position, MouseState::Scroll(delta.0, delta.1)
            ))
        }
    }
    fn reset_gesture(&mut self) {
        self.touches = 0;
        self.samples.clear();
        self.velocity = (0.0, 0.0);
        self.updated = None;
    }
    fn sample(&mut self, position: (f32, f32), now: Instant) {
        self.dragged |= (position.0 - self.position.0).hypot(position.1 - self.position.1) >= 4.0;
        self.samples.push_back((now, position));
        while self.samples.len() > 1
            && now.duration_since(self.samples.front().unwrap().0).as_secs_f32() > 0.1 {
            self.samples.pop_front();
        }
    }
}
impl Drop for Events {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in &self.observers {unsafe {
            center.removeObserver(AsRef::<objc2::runtime::AnyObject>::as_ref(&**observer));
        }}
        unsafe {
            AVAudioSession::sharedInstance().removeObserver_forKeyPath(
                &self.volume, &NSString::from_str("outputVolume"),
            );
        }
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
                if (AudioType::matches(&ty) || MovieType::matches(&ty))
                    && let Some(data) = self.0.dataForPasteboardType(&ty)
                    && let Ok(media) = Media::read(Cursor::new(data.to_vec()))
                {
                    return Some(Data::Media(media));
                }
            }
            if let Some(i) = self.0.image() && let Some(image) = Clipboard::image(&i) {
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
                Data::Image(image) => self.0.setImage(Some(&Clipboard::native_image(image))),
                Data::File(path) => {
                    let path = NSString::from_str(&path.to_string_lossy());
                    self.0.setURL(Some(&NSURL::fileURLWithPath(&path)));
                }
                Data::Media(media) => {
                    let (bytes, ty) = match super::apple::ShareMedia::export(media) {
                        Ok(export) => export,
                        Err(error) => {log::error!("Clipboard export failed: {error}"); return}
                    };
                    let bytes = NSData::with_bytes(&bytes);
                    self.0.setData_forPasteboardType(&bytes, &ty.identifier());
                }
            }
        }
    }
    fn native_image(image: &Image) -> Retained<UIImage> {
        UIImage::imageWithCGImage(&super::apple::ShareMedia::native_image(image))
    }
    fn image(image: &UIImage) -> Option<Image> {
        let image = unsafe {image.CGImage()?};
        Some(super::apple::ShareMedia::image(&image))
    }
}

pub struct Haptics(std::cell::RefCell<Option<Retained<UIImpactFeedbackGenerator>>>);
impl Haptics {
    pub fn new() -> Self {Self(std::cell::RefCell::new(None))}

    pub fn trigger(&self) {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            let root = SafeArea::window().rootViewController().expect("no root view controller");
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

pub struct Push;

impl Push {
    pub async fn current(previous: Option<&PushDestination>) -> PushDestination {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        DispatchQueue::main().exec_async(move || {
            let destination = PUSH.with(|push| push.0.as_ref().map(|delegate| {
                delegate.ivars().destination.subscribe()
            }));
            let _ = sender.send(destination);
        });
        let Some(mut destination) = receiver.await.unwrap() else {
            return futures::future::pending().await
        };
        loop {
            let current = destination.borrow_and_update().clone();
            if let Some(current) = current {
                if previous != Some(&current) {return current}
                destination.changed().await.unwrap();
                continue;
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            DispatchQueue::main().after(Duration::from_secs(30).try_into().unwrap(), move || {
                PUSH.with(|push| {
                    let delegate = push.0.as_ref().unwrap();
                    if delegate.ivars().destination.borrow().is_none()
                        && delegate.ivars().requested.get().elapsed() >= Duration::from_secs(30) {
                        delegate.ivars().requested.set(Instant::now());
                        UIApplication::sharedApplication(MainThreadMarker::new().unwrap()).registerForRemoteNotifications();
                    }
                });
                let _ = sender.send(());
            }).unwrap();
            tokio::select! {
                biased;
                changed = destination.changed() => changed.unwrap(),
                elapsed = receiver => elapsed.unwrap(),
            }
        }
    }
    pub fn notify(proxy: EventLoopProxy<Output>, header: String, body: String) {
        DispatchQueue::main().exec_async(move || {
            let app = UIApplication::sharedApplication(MainThreadMarker::new().unwrap());
            if app.applicationState() == UIApplicationState::Active {
                let _ = proxy.send_event(Output::Input(Input::Notification(header, body)));
            } else {super::apple::Notifications::send(&header, &body);}
        });
    }
}

pub struct Share {
    media: super::apple::ShareMedia,
}
impl Share {
    pub fn new() -> Self {Share{media: super::apple::ShareMedia::new()}}

    /// Opens the system share sheet for a single item.
    /// Must be called on the main thread.
    pub fn share(&self, data: &Data) {
        let item: Retained<_> = match data {
            Data::Text(text) => NSString::from_str(text).into_super(),
            Data::Image(image) => Clipboard::native_image(image).into_super(),
            Data::File(path) => {
                let path = NSString::from_str(&path.to_string_lossy());
                NSURL::fileURLWithPath(&path).into_super()
            }
            Data::Media(_) => {
                let Some(path) = self.media.path(data) else {return};
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

        let mut vc = SafeArea::window().rootViewController().unwrap();
        while let Some(presented) = vc.presentedViewController() {vc = presented;}
        if let Some(popover) = activity_vc.popoverPresentationController() {
            let view = vc.view().unwrap();
            popover.setSourceView(Some(&view));
            popover.setSourceRect(view.bounds());
        }
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
        pub unsafe fn init_with_configuration(this: Allocated<Self>, configuration: &PHPickerConfiguration) -> Retained<Self>;

        #[unsafe(method(setDelegate:))]
        #[unsafe(method_family = none)]
        pub unsafe fn set_delegate(&self, delegate: Option<&ProtocolObject<dyn PHPickerViewControllerDelegate>>);
    );
}

pub struct DataPicker {
    tx: AsyncTx<spsc::List<PickRequest>>,
    delegate: Retained<LibraryPickerDelegate>,
}

impl DataPicker {
    pub fn image(data: &NSData) -> Result<Image, String> {
        if let Ok(image) = Image::read(data.to_vec().as_slice()) {return Ok(image)}
        UIImage::initWithData(UIImage::alloc(), data)
            .and_then(|image| Clipboard::image(&image))
            .ok_or_else(|| "The image could not be decoded.".into())
    }
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

    pub fn pick<P: Pickable>(&self, hardware: &Hardware) -> Picked<P> {
        let (tx, rx): (AsyncTx<spsc::One<PickResult>>, _) = spsc::build(spsc::One::new());
        self.tx.try_send(P::request(tx)).expect("Picker queue disconnected");
        self.delegate.show();
        Picked::new(rx, hardware)
    }

    pub fn cancel(&self) {self.delegate.cancel();}
}

impl Drop for DataPicker {
    fn drop(&mut self) {
        unsafe {NSNotificationCenter::defaultCenter().removeObserver(&self.delegate);}
        if let Some(state) = self.delegate.ivars().current.borrow_mut().take() {
            match state {
                PickState::Waiting(request) | PickState::Selecting(request) |
                    PickState::Dismissing(request) | PickState::Loading(request) |
                    PickState::Rejected(request, _) | PickState::Alerting(request) => request.cancel(),
            }
        }
        while let Ok(request) = self.delegate.ivars().rx.try_recv() {request.cancel();}
        let controller = self.delegate.ivars().controller.borrow_mut().take();
        if let Some(controller) = controller {
            if let Some(presentation) = controller.presentationController() {
                unsafe {presentation.setDelegate(None);}
            }
            if let Some(picker) = controller.downcast_ref::<UIDocumentPickerViewController>() {
                picker.setDelegate(None);
            }
            if let Some(picker) = controller.downcast_ref::<PHPickerViewController>() {
                unsafe {picker.set_delegate(None);}
            }
            LibraryPickerDelegate::dismiss(&controller, RcBlock::new(|| {}));
        }
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
    }

    unsafe impl PHPickerViewControllerDelegate for LibraryPickerDelegate {
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
                request.load_photos(&results, move |items| {
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

struct KeyboardIvars {
    active: Cell<bool>,
    flushing: Cell<bool>,
    proxy: EventLoopProxy<Output>,
}

define_class!(
    #[unsafe(super(UITextView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = KeyboardIvars]
    struct KeyboardView;

    unsafe impl NSObjectProtocol for KeyboardView {}
    unsafe impl UIScrollViewDelegate for KeyboardView {}
    unsafe impl UITextViewDelegate for KeyboardView {
        #[unsafe(method(textViewDidChange:))]
        fn changed(&self, _view: &UITextView) {self.commit();}
    }
    impl KeyboardView {
        #[unsafe(method(autocorrectionType))]
        fn autocorrection_type(&self) -> UITextAutocorrectionType {UITextAutocorrectionType::No}

        #[unsafe(method(spellCheckingType))]
        fn spell_checking_type(&self) -> UITextSpellCheckingType {UITextSpellCheckingType::No}

        #[unsafe(method(hasText))]
        fn has_text(&self) -> bool {self.ivars().active.get()}

        #[unsafe(method(deleteBackward))]
        fn delete_backward(&self) {
            if self.ivars().active.get() && self.text().is_empty() {
                AppEvents::key(&self.ivars().proxy, crate::inputs::Key::Backspace);
            } else {
                let _: () = unsafe {msg_send![super(self), deleteBackward]};
            }
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            let _: () = unsafe {msg_send![super(self), unmarkText]};
            self.commit();
        }
    }
);

impl KeyboardView {
    fn commit(&self) {
        if !self.ivars().active.get() || self.ivars().flushing.get()
            || self.markedTextRange().is_some() {return}
        let text = self.text().to_string();
        if text.is_empty() {return}
        self.ivars().flushing.set(true);
        AppEvents::text(&self.ivars().proxy, &text);
        self.setText(None);
        self.ivars().flushing.set(false);
    }
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = EventLoopProxy<Output>]
    struct VolumeObserver;
    unsafe impl NSObjectProtocol for VolumeObserver {}
    impl VolumeObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn changed(
            &self, _key: Option<&NSString>, _object: Option<&objc2::runtime::AnyObject>,
            change: Option<&NSDictionary<NSString, objc2::runtime::AnyObject>>, _context: *mut std::ffi::c_void,
        ) {
            let Some(change) = change else {return};
            let old = unsafe {change.objectForKey(NSKeyValueChangeOldKey)}
                .and_then(|value| value.downcast::<NSNumber>().ok());
            let new = unsafe {change.objectForKey(NSKeyValueChangeNewKey)}
                .and_then(|value| value.downcast::<NSNumber>().ok());
            if let (Some(old), Some(new)) = (old, new) {
                let (old, new) = (old.floatValue(), new.floatValue());
                let input = if new > old {MediaInput::VolumeUp}
                    else if new < old {MediaInput::VolumeDown} else {return};
                let _ = self.ivars().send_event(Output::Input(Input::Media(input)));
            }
        }
    }
);


struct _Push(Option<Retained<PushDelegate>>);

impl _Push {
    fn new() -> Self {
        let delegate = if NSBundle::mainBundle().bundleIdentifier().is_none() {None} else {
            let mtm = MainThreadMarker::new().unwrap();
            let app = UIApplication::sharedApplication(mtm);
            let previous = unsafe {msg_send![&app, delegate]};
            let delegate = PushDelegate::alloc(mtm).set_ivars(PushIvars {
                previous, destination: tokio::sync::watch::channel(None).0,
                requested: Cell::new(Instant::now()),
            });
            let delegate: Retained<PushDelegate> = unsafe {msg_send![super(delegate), init]};
            let object = ProtocolObject::<dyn UIApplicationDelegate>::from_ref(&*delegate);
            unsafe {let _: () = msg_send![&app, setDelegate: object];}
            super::apple::Notifications::authorize();
            app.registerForRemoteNotifications();
            Some(delegate)
        };
        Self(delegate)
    }
}

impl Drop for _Push {
    fn drop(&mut self) {
        let Some(delegate) = self.0.as_ref() else {return};
        let app = UIApplication::sharedApplication(MainThreadMarker::new().unwrap());
        let current: Option<Retained<AnyObject>> = unsafe {msg_send![&app, delegate]};
        if current.as_ref().is_some_and(|current| {
            std::ptr::addr_eq(Retained::as_ptr(current), Retained::as_ptr(delegate))
        }) {
            let previous = delegate.ivars().previous.as_deref();
            unsafe {let _: () = msg_send![&app, setDelegate: previous];}
        }
    }
}

struct PushIvars {
    previous: Option<Retained<AnyObject>>,
    destination: tokio::sync::watch::Sender<Option<PushDestination>>,
    requested: Cell<Instant>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MaverickPushDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = PushIvars]
    struct PushDelegate;

    unsafe impl NSObjectProtocol for PushDelegate {
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            let responds: bool = unsafe {msg_send![super(self), respondsToSelector: selector]};
            responds || self.ivars().previous.as_ref().is_some_and(|previous| {
                unsafe {msg_send![previous, respondsToSelector: selector]}
            })
        }
    }

    impl PushDelegate {
        #[unsafe(method_id(forwardingTargetForSelector:))]
        fn forwarding_target(&self, _selector: Sel) -> Option<Retained<AnyObject>> {
            self.ivars().previous.clone()
        }
    }

    unsafe impl UIApplicationDelegate for PushDelegate {
        #[unsafe(method(application:didRegisterForRemoteNotificationsWithDeviceToken:))]
        fn registered(&self, application: &UIApplication, token: &NSData) {
            let destination = PushDestination::Apns(hex::encode(token.to_vec()));
            self.ivars().destination.send_replace(Some(destination));
            if let Some(previous) = &self.ivars().previous {
                let selector =
                    objc2::sel!(application:didRegisterForRemoteNotificationsWithDeviceToken:);
                let responds: bool = unsafe {msg_send![previous, respondsToSelector: selector]};
                if responds {
                    unsafe {
                        let _: () = msg_send![
                            previous, application: application,
                            didRegisterForRemoteNotificationsWithDeviceToken: token,
                        ];
                    }
                }
            }
        }

        #[unsafe(method(application:didFailToRegisterForRemoteNotificationsWithError:))]
        fn failed(&self, application: &UIApplication, error: &NSError) {
            self.ivars().destination.send_replace(None);
            if let Some(previous) = &self.ivars().previous {
                let selector =
                    objc2::sel!(application:didFailToRegisterForRemoteNotificationsWithError:);
                let responds: bool = unsafe {msg_send![previous, respondsToSelector: selector]};
                if responds {
                    unsafe {
                        let _: () = msg_send![
                            previous, application: application,
                            didFailToRegisterForRemoteNotificationsWithError: error,
                        ];
                    }
                }
            }
        }
    }
);
