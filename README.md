# Maverick OS

Maverick OS provides common hardware interfaces for applications.

## Hardware initialization

`Hardware::new` sets up logging and the application support directory. Camera, photo/document
picker, keyboard, clipboard, haptics, sharing, and cache services initialize only when their corresponding
`Hardware` method is first called. Cloned hardware handles share those initialized services.
Creating hardware does not open the camera or create a picker delegate; camera setup begins at
`start_camera`, and picker setup begins at `pick`.

Hardware owns its capture and picker services. Dropping the last `Hardware` clone stops active
capture and cancels pending selections, even when operation handles remain. Capture and picker
handles do not extend the hardware's lifetime. Recorded media and completed selections remain usable.

Window and input event handling starts with the application. `Player` handles audio-only,
video-only, and combined media through the same playback interface.

The event loop passes cloned proxies to native callback owners. Callbacks deliver application
inputs through their own proxy; each executor wake sends an internal `Output::Wake` message directly
through Winit without a deduplication flag.
`MaverickOS` owns the driver state and resumes the application task on the UI thread. That task
awaits its inbox, scheduled native inputs, or `Application::changed()`. It drops competing waits
before handling the winning input; completed application changes update instructions directly.
It drains ready work without a batch limit or forced yield, waiting when no source is ready.

Native `GeometryChanged` messages refresh keyboard overlap, insets, and layout without resizing
the render surface. Winit window-size and scale changes resize the surface, and
`RedrawRequested` presents a frame even when the instructions have not changed. Suspension
releases the renderer and pauses application/native change waits until resume.

`hardware.notifier()` returns a clonable `Notify` handle. Pass it into services during setup and
call `notify.notify(header, body)` to request a user notification. Clones target the same
application and can cross threads without retaining its UI-only `Hardware` handle.

## Images

`Image::read(reader)` retains encoded raster content and reads its intrinsic dimensions without
constructing the RGBA pixel buffer. `dimensions()` does not trigger decoding.
`Image` dereferences to `Arc<RgbaImage>` and decodes on first access; clones share the same
cached pixels. If the payload cannot be decoded, it caches the existing corrupted-media image
instead. Reading invalid headers or failing to read the input still returns an error.
`Image::from(rgba)` accepts owned or shared RGBA buffers for generated images and camera frames.
Equality compares shared content identity without decoding or comparing pixels.

Changing an image's displayed size does not change its content or pixel allocation. WGPU Prism
retains the uploaded texture and scales it on the GPU. Actual vector content belongs to Prism's
separate `Svg` drawable; Maverick's `Image` does not contain SVGs.

## Capture and playback

`start_camera()` returns a silent `Camera`, `start_microphone()` returns a `Microphone`, and
`start_capture()` returns a `Capture` with video and audio. These handles keep capture active
until their last clone is dropped or the owning hardware shuts down. Each implements `AsRef<Media>`.

`Media` is one audio/video timeline. `Hardware::pick::<Media>()` selects audio or video.
`pick::<Video>()` selects a video with any accompanying audio, and `pick::<Audio>()` selects
audio-only content. `Video(pub Media)` and `Audio(pub Media)` expose the same timeline through
`.0` and implement `AsRef<Media>` for playback. They do not copy or strip tracks.
`pick::<Visual>()` selects either `Visual::Image(Image)` or `Visual::Video(Video)`;
`pick::<Image>()` selects only images. Each selection type also supports `Vec<T>` for multiple items.
Requests from the same hardware are queued. Dropping an unfinished `Picked` cancels only that
request; dropping the last hardware clone cancels every pending request. Completed requests retain
their result for repeated `get().await` calls.

```rust,ignore
let video = hardware.pick::<Video>().get().await?;
let player = Player::new(&video)?;
let audio = hardware.pick::<Audio>().get().await?;
let visuals = hardware.pick::<Vec<Visual>>().get().await?;
```

`Player::new` accepts `impl AsRef<Media>`, clones the media handle, and returns a concrete paused
`Player`. Keep both hardware and the capture handle while recording; the player does not own capture.
Dropping the last capture or hardware clone stops new input while recorded media remains available.

```rust,ignore
let camera = hardware.start_camera()?;
let player = Player::new(&camera)?;
player.set_mode(PlaybackMode::Live(std::time::Duration::ZERO));
player.start();
```

`set_mode(PlaybackMode)` selects playback behavior. `Default` plays to the available end and
waits for appended media. `Live(delay)` follows new content with the requested target buffer;
`Loop` repeats the current timeline. Seeking leaves live mode and preserves loop mode.

`Player::frame()` returns the current `Frame` immediately. Its `image` is `None` for audio-only
media. `Player::next().await` returns the exact next queued frame state. Reading the current frame
establishes the baseline for that wait and discards older unread updates. Each subscription keeps
a bounded queue of eight updates; consuming a lagged subscription with `next()` is an invariant
violation and panics. Call `frame()` to resume from current state when old updates are unwanted.

## Media editing and export

`Media::new()` creates an empty timeline. `Media::try_from(sound)` and
`Media::try_from((image, duration))` create generated media. `duration()` returns the current
known length, including zero for an empty timeline. `dimensions()` and `thumbnail()` return
`None` when no video exists. Thumbnails are generated only when requested and cached.

Media clones share one timeline. `replace(range, &replacement, tracks)` edits selected tracks;
`copy(range, tracks)` creates an independent timeline sharing immutable payloads. `Tracks::Audio`,
`Tracks::Video`, and `Tracks::Both` select the affected content. An empty replacement range inserts
content; an empty replacement media value deletes it. `append(&media)` snapshots and adds all
tracks at the destination's current end in one atomic edit, without running codecs.

Edits preserve each player's absolute timestamp. If an edit shortens the timeline past that
position, playback waits for more content. An editor preserving the same content after insertion
must seek to its new position explicitly.

`Media::read(reader)` accepts `Read + Seek` and imports a complete audio/video file. It does not
parse arbitrary network byte fragments. `write(writer, format)` accepts `Write + Seek` and uses
the explicit `Format`; the file extension does not select the encoder.

The format selects output tracks as well as codecs. Audio-only formats extract the audio track;
when the source has no audio, they write an empty audio track with zero samples and duration.
Video-only formats omit audio.
Combined formats preserve an absent audio track instead of adding one.

| Format | Container | Video codec | Audio codec |
|---|---|---|---|
| `Mp4_Mpeg4p2_Aac` | MP4 | MPEG-4 Part 2 | AAC |
| `Mp4_Mpeg4p2` | MP4 | MPEG-4 Part 2 | — |
| `Mkv_Ffv1_Flac` | Matroska | FFV1 | FLAC |
| `Mkv_Ffv1_PcmF32le` | Matroska | FFV1 | Float PCM, 32-bit |
| `Mkv_Ffv1` | Matroska | FFV1 | — |
| `Mov_ProRes422_PcmS16le` | MOV | ProRes 422 | Signed PCM, 16-bit |
| `Mov_ProRes422` | MOV | ProRes 422 | — |
| `M4a_Aac` | M4A | — | AAC |
| `M4a_Alac` | M4A | — | ALAC |
| `Wav_PcmS16le` | WAV | — | Signed PCM, 16-bit |
| `Wav_PcmF32le` | WAV | — | Float PCM, 32-bit |
| `Flac_Flac` | FLAC | — | FLAC |

```rust,ignore
let excerpt = media.copy(start..end, Tracks::Both)?;
let mut file = std::io::Cursor::new(Vec::new());
excerpt.write(&mut file, Format::Mkv_Ffv1_PcmF32le)?;
```

`chunks(duration)` returns `Chunks`, which retains an immutable timeline snapshot and lazily
yields independent media values sharing source payloads. Later edits and captured frames do
not affect it. Creating or iterating chunks does not encode the recording; call `write` when
bytes are needed. `Chunks` implements `Clone`, `DoubleEndedIterator`, `ExactSizeIterator`, and
`FusedIterator`; skipped chunks are not constructed.

`Media` implements `Extend` and `FromIterator` for owned or borrowed media. Each input snapshot
is appended in order without running codecs:

```rust,ignore
let joined: Media = recordings.iter().collect();
let mut recording = Media::new();
recording.extend(joined.chunks(std::time::Duration::from_secs(2))?);
```

The following uploader is application code; only the current chunk is encoded for each upload:

```rust,ignore
for chunk in media.chunks(std::time::Duration::from_secs(2))? {
    let mut file = std::io::Cursor::new(Vec::new());
    chunk.write(&mut file, Format::Mkv_Ffv1_PcmF32le)?;
    uploader.upload(file.into_inner()).await?;
}
```

Maverick owns media timelines and file formats. The integration layer owns remote storage,
chunk identifiers, network transfers, and downloading complete segments before adding them to
a timeline. Live playback does not discard retained recording history.

## Keyboard

`Hardware::open_keyboard()` returns a clonable `Keyboard` handle. On iOS and Android it opens
the system keyboard; dropping the last handle dismisses it. Repeated calls on the same hardware
instance share the session, including calls through cloned `Hardware` handles. Keep the handle
in the focused input's owner and drop it when that input loses focus. On macOS and Linux the
handle has no native UI to open; physical keyboard input continues normally.

```rust,ignore
let keyboard = hardware.open_keyboard();
let keep_open = keyboard.clone();
drop(keyboard); // Still open while keep_open exists.
drop(keep_open); // Dismisses the native keyboard.
```

Native text is delivered automatically to `Application::on_input` using the existing
`Input::Device(_, DeviceInput::Keyboard(key, state, modifiers))` events. Each committed Unicode
scalar produces `Key::Character` pressed/released events, including every scalar in pasted
text and emoji sequences. Return, tab and deletion use `Key::Enter`, `Key::Tab` and
`Key::Backspace`/`Key::Delete`. Native text input uses device ID `u64::MAX - 1` and default
modifiers. IME composition stays in the native editor until committed, avoiding duplicate
preedit text. Dropping the session discards unfinished composition.

This is an event stream: the native editor does not own the application's text or selection,
so surrounding-text autocorrection is disabled. Android applications must package Maverick's
`MaverickActivity.java` and `KeyboardView.java`; the provided Android build script includes both.
The hardware demo's **keyboard** button toggles the handle and shows received text.

`LogicalWindow::size` reports the available area above a docked native keyboard.
`Input::Resized` carries the changing size throughout opening and closing, including dismissal
after the last `Keyboard` is dropped. Apply only `window.insets` for the remaining safe area;
Maverick removes the covered bottom safe area along with the keyboard. Space already removed
by OS resizing is not subtracted again. Floating keyboards do not shorten the window.

`Window::physical()` continues to describe the rendering surface, preserving pixel scale and
touch coordinates. Applications lay out and clip their content to the reported logical size.

iOS tracks the keyboard layout guide's presentation geometry (iOS 17+). Android 11+ supplies
native IME animation frames.

`LogicalWindow::keyboard_visible` reports native keyboard visibility through the same
`Input::Resized` event. It remains true through dismissal and also covers floating keyboards,
which can appear without reducing the window size. Visibility comes from native notifications;
requesting a handle alone does not mark the keyboard visible. There is no separate keyboard
visibility event or public keyboard bounds. Retaining a handle does not prevent user dismissal.
Drop and reacquire it to request a new session after dismissal.

## Installation identity and push registration

`Hardware::device_id()` returns a random 128-bit hexadecimal identifier persisted in `DEVICE_ID`
in the application's support directory. It is shared by cloned handles and survives restarts.
The identifier file is excluded from Apple backups. Removing the application's stored data
creates a new identity on the next launch. On macOS,
removing only the app bundle leaves application support data, so the identifier may survive a
reinstall. It is not a hardware identifier and must not be used as a push address.

`Hardware::push_destination().await` waits for the platform's current `PushDestination`, even when it
matches a result from an earlier call or app launch. Compare it with the destination registered
with your server during the previous launch. Calling `current().await` on that destination waits
until the platform supplies a different token and returns the replacement. Repeated native callbacks
with the same token do not complete that wait.

```rust,ignore
let token = hardware.push_destination().await;
let replacement = token.current().await;
```

`PushDestination::Apns(token)` contains the Apple token as hexadecimal text;
`PushDestination::Fcm(token)` contains the opaque Android FCM token. Both support serde
serialization and equality comparisons. It remains a plain value, without native handles or
subscription state. A saved or cloned destination can also wait for its replacement. Maverick
obtains the current token from the platform SDK each launch rather than trusting a saved destination.

The Apple platform adapter shares one native registration for the lifetime of the application. Native
callbacks wake waiting callers. Maverick retries unsuccessful registration internally at most
every 30 seconds while callers wait; concurrent callers share attempts. Waiting for a change to an
available token does not trigger registration retries. Dropping a future cancels only that wait.
Changes received between waits are retained; a later wait returns the latest destination when it
differs from the token on which `current()` was called. The application's event loop must keep
running to deliver native callbacks and retries. Unchanged or unavailable destinations remain
pending. Existing application delegates continue receiving callbacks.

APNs registration requires a bundled, correctly signed app with the Push Notifications capability.
The notification server must supply its app's APNs topic, environment, and credentials. This API
does not request permission to display notifications or expose permission status; notification
presentation and RAMP's server registration will be added separately. Neither identifier is
exposed through Prism.

## Building

Video validation and playback use FFmpeg libraries built automatically by Cargo through
`ramp-ffmpeg`. FFmpeg 9.0.2 source ships in the transitive binding crate and is compiled for the
target. The libraries are linked statically into the application, so building and running
Maverick does not require an installed FFmpeg package or runtime. Building FFmpeg requires
neither Git nor a separate source download.

macOS and iOS builds use pregenerated FFmpeg bindings and require Rust, Xcode's native compiler
and target SDK, and Make. NASM is built automatically from bundled source only for Intel
(`x86`/`x86_64`) targets, preserving FFmpeg's optimized assembly. No separate NASM installation
or build-time source download is needed. These builds do not require libclang or pkg-config.
Subsequent builds reuse Cargo's build cache.

Run the hardware demo with:

```sh
cargo run --features demo
```

The demo controls all active players with play/pause, speed, volume, restart, and five-second
seek buttons. Its mode button cycles through `Default`, `Loop`, and `Live(Duration::ZERO)`;
each media panel shows its actual mode, position, speed, volume, and playback errors.
Seeking exits live mode while preserving loop mode. New camera previews start live, and
newly selected audio/video inherits the selected mode, speed, and volume. Native play/pause and
volume events use the same players.

## Rendering

`Window::physical()` reports the full drawable surface in pixels. `Window::logical()` reports
that same surface in logical points, with safe-area insets supplied separately. On iOS this
includes the areas behind the status bar and home indicator; layout applies those insets once.
Touch positions use the same full-surface origin. On macOS the surface excludes window decorations.

`Application::draw` produces the current drawing instructions. `Application::changed().await`
waits for background state that needs another draw. Maverick awaits this alongside native
events and `Events::next()`, which schedules future input such as active scrolling momentum
and keyboard animation. An idle application has no recurring 16 ms wake-up. Suspension stops
these waits until the application resumes. The media services retain their separate timing.

Native callbacks wake one owned application future. The small private event-loop adapter is
the only place that constructs a task context or waker and polls that future; application and
source interfaces use ordinary async functions.

Renderers implement `update(instructions) -> bool` to retain the latest scene and report whether
it needs presentation. This method must not draw or present. Maverick requests a redraw when it
returns true, then calls `draw()` in response to `RedrawRequested`. OS redraw requests always call
`draw()`, including when the scene is unchanged. Native input rebuilds the instructions and
requests a redraw only when `update` reports a change. Resize and resume force a redraw.

The Ramp/wgpu renderer compares drawing instructions exactly and retains one instruction list.
Unchanged scenes reuse prepared geometry; changed scenes are prepared on the next redraw.

## Linux (initial support)

The Linux adapter provides window/input integration, XDG application data storage,
persistent installation identity, the existing SQLite cache, clipboard text/images/files,
typed file, image, video, and audio picking through XDG Desktop Portal, and local portal notifications.
`share` uses the portal's application chooser for files, text, and images. It does not
provide an Apple-style share sheet. Haptic requests do nothing on Linux.

Install a session D-Bus, an appropriate XDG Desktop Portal backend for your desktop,
and the native build dependencies. On Ubuntu/Debian:

```sh
sudo apt install build-essential pkg-config libasound2-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-0
```

The published FFmpeg bundle includes `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu` bindings. The default bundled build needs no installed
FFmpeg, libclang, or NASM.

Clipboard ownership lasts while the hardware clipboard service is alive; persistence
when the app exits depends on the desktop's clipboard manager. Native Wayland clipboard
access requires a supported data-control protocol; arboard may use XWayland where available.
File pickers are queued, and dropping one handle cancels only that request. Invalid media
selections reopen the picker with an error and preserve valid items for a replacement
selection. The initial picker uses an unparented, nonmodal portal dialog.

Typed pickers filter the portal dialog and validate decoded images, video, or audio before returning.
`Visual` accepts images and videos; `Media` accepts audio and video. Generic data picking retains
non-image files as `Data::File`. Audio/video clipboard export, sharing, capture, and system media
controls are not implemented by the Linux adapter yet. Shared media processing and CPAL playback
remain available. Capture requests return an explicit error.
There is no Linux remote-push provider, so `push_destination()` remains pending; local
`notify` is independent. Portal availability, permissions, and application identity are
managed by the desktop; unavailable best-effort actions log their errors.

## Android (initial support)

The Android adapter provides the native window/touch lifecycle, application-private storage,
persistent installation identity, SQLite cache, text/image/file clipboard, queued system
typed file, image, video, and audio selection, the Android share sheet, view haptics, and local
notifications. Image selection uses Android's decoder to normalize orientation before returning RGBA pixels.
File selections are imported into app-private storage and stay usable after the picker closes.
Typed media selections are decoded before their temporary imports are removed. `Visual` filters
for images and videos; `Media` filters for audio and video. Generic data picking retains non-image
files as `Data::File`.
Dropping a picker handle cancels only its request; results use per-request sockets rather than
native callback pointers. Native services and Java objects are owned by the activity.

The standalone Android demo has been archived. The library demo remains in `src/main.rs`
and runs on desktop with `cargo run --features demo`.

Cargo Piston's Android development command packages applications that provide a `cdylib`
with an `android_main` entry point:

```sh
cargo piston dev android
```

It uses Android SDK platform 36, NDK 29, JDK 17 or newer, and `cargo-ndk`, packages Maverick's
Gradle bridge, and installs a development APK on the selected device or emulator. The app
requires Android 11 (API 30) or later. Published FFmpeg bindings cover `aarch64-linux-android`
and `x86_64-linux-android`. The library's desktop demo is a binary and is not an Android
`cdylib` target.

Applications embedding Maverick should include the `android/bridge` Gradle library module and
use `me.ramp.maverick.MaverickActivity` as their NativeActivity subclass. The library packages
the Java bridge, Firebase SDK, nonexported messaging service, notification defaults and
`SharedFileProvider`. Gradle merges its manifest and transitive Firebase dependencies.
Cargo Piston supplies the application manifest when packaging an Android development app. Clipboard and share files are copied to
an isolated cache directory; recipients receive read-only content URIs. Do not export the provider.

Android push uses Firebase Cloud Messaging. Register an Android app in Firebase, enable the
FCM HTTP v1 API, and include the public client configuration in the embedding application's
Android resources. Its package name must match the registered Firebase client. The Google
service-account private key belongs only to `air_notifications`'s `FcmApplication` contract;
never put it in the APK. RAMP additionally needs the Android application contract ID in
`RAMP_NOTIFICATION_APPLICATION` at native compile time.

`push_destination()` returns the current SDK token, and `.current()` waits for a different token.
Concurrent waits share registration and retries; dropping one future cancels only that wait.
Token rotation while no activity is running is retained by Firebase and read on the next launch.
Unconfigured builds keep the push wait pending and log the missing configuration once per attempt.

Push registration requests Android's notification permission when the activity is resumed.
Foreground local and remote notifications emit `Input::Notification`; background notifications
use the native notification tray. Tapping a notification opens the app. Firebase can deliver
server notification payloads after ordinary process death, but not while the user has force-stopped
the app. Google Play services and notification permission are required. The same AIR notification
server handles Android and Apple; FCM supplies the Android transport.
Audio/video clipboard export, sharing, capture, and OS media controls remain deferred,
matching initial Linux support. Capture returns an explicit
unsupported error. Shared media processing remains available; audio routing/focus and capture
need a separate media integration pass.

The application cache owns its SQLite connection at `ApplicationSupport/CACHE`; startup does not
change the process working directory. Opening this required cache is a startup invariant and panics
if it fails. Optional cache reads and writes return errors, including media export failures, so their
owners can present or recover from those failures. Ramp also treats loading and saving its persistent
Air identity as mandatory; it does not silently replace an unreadable identity.
