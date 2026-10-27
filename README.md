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
the render surface and present changed content immediately. Winit window-size and scale changes
resize the surface; duplicate notifications with unchanged physical and logical geometry skip layout
and rendering. Logical-only keyboard changes preserve ongoing touch and scroll tracking.
`RedrawRequested` presents the prepared frame, refreshing layout only if logical geometry changed.
A skipped draw stays pending for the next input, geometry change, or redraw request. Suspension
releases the renderer and pauses application/native change waits until resume.

`hardware.notifier()` returns a clonable `Notify` handle. Pass it into services during setup and
call `notify.notify(header, body)` to request a user notification. Clones target the same
application and can cross threads without retaining its UI-only `Hardware` handle.

## Images

Media primitives belong to the `media2` package, imported as `media` and re-exported by Maverick.
`Image` contains raw pixels and a public `dimensions: (u32, u32)` field. It dereferences to its
`Vec<Pixel>`, so indexing, iteration, and pixel edits use ordinary collection operations.
Each `Pixel` has public red, green, blue, and alpha bytes. Pixels use straight-alpha RGBA8 in sRGB,
ordered left to right, then top to bottom. `image.as_ref()` exposes the same allocation as `&[u8]`
for rendering and native adapters. Cloning an `Image` shares its pixel allocation; mutating a clone
copies the buffer only when it is shared. Equality compares image data.

`Image::read(reader)` decodes encoded raster content into those pixels and reports invalid input.
`Image::new(dimensions, pixels)` takes an owned pixel buffer without copying it.
The buffer must contain `width * height` pixels;
operations consuming an image validate its dimensions. No public API exposes `RgbaImage`.

`image.write(writer, ImageFormat::Png)` encodes to any `Write`, including `Vec<u8>` and files.
`ImageFormat` supports PNG, JPEG, lossless WebP, GIF, BMP, ICO, TIFF, and QOI and provides
`extension()`. JPEG uses quality 75 and omits alpha; GIF uses palette color and binary transparency.
ICO requires dimensions no larger than 256 pixels. TIFF buffers its output because its encoder
needs seeking; GIF buffers its output to propagate errors when writing the final trailer.
The caller does not need to provide `Seek`.

Changing an image's displayed size does not change its pixels. Prism retains image content for
rendering and WGPU scales its uploaded texture. Vector content belongs to Prism's separate `Svg`
drawable; `media::Image` does not contain SVGs.

## Capture and playback

`start_camera()` returns a silent `Camera`, `start_microphone()` returns a `Microphone`, and
`start_capture()` returns a `Capture` with video and audio. Their asynchronous `next()` methods
return `Option<VideoFrame>`, `Option<AudioFrame>`, and `Option<MediaFrame>`, respectively.
Raw frames contain source-relative `timestamp`, `duration`, and `corrupted` fields plus an `Image`
or `Sound`. A `MediaFrame` has optional audio and video fields; each keeps its own timestamp.
All platforms deliver audio and video independently, so a combined capture's next result contains
whichever track arrived. Captures do not retain a recording; the application chooses where to insert frames.

These handles keep capture active until their last clone is dropped or the owning hardware shuts
down. Each clone has an independent bounded subscription. A slow consumer skips unread old frames
and retains source timestamp gaps. `None` means the subscription has closed; `error()` reports a
terminal capture failure when present. All four native platforms support camera, microphone, and
combined capture through the same handles. Independent requests share the native inputs; dropping
one request or its clones leaves other requests active.

Android uses Camera2 and AudioRecord, requests camera and microphone permission when needed, and
normalizes camera orientation and native timestamps before delivering RGBA images and float audio.
Capture requires a foreground app; background camera/microphone access remains subject to Android's
lifecycle restrictions. Devices without a shared camera/audio clock use the first camera frame's
arrival time to align video, so exact audiovisual synchronization depends on the device.

Linux uses GStreamer with the default video and audio sources and a shared system clock. Install
GStreamer's development packages when building and the device plugins when running. On Debian or
Ubuntu: `apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
gstreamer1.0-plugins-good gstreamer1.0-pulseaudio`. The audio server's default source selects the
microphone; the native video source selects the camera. Device access errors close the affected
subscriptions and are available through `error()`.

Linux and Android can pick, copy, paste, and share the new `Media` values. Media export uses lossless
Matroska for video and float WAV for audio, matching Apple's export path. Native recipients must
support those formats. Linux exposes playback through MPRIS; Android uses MediaSession and audio
focus, with a media notification for playback controls. Play and pause requests enter the application
as `Input::Media`, including Android focus loss and headphone disconnection. The application decides which players respond.

`Media` aggregates public `audio: Audio` and `video: Video` tracks. A `Video` is silent;
`Hardware::pick::<Video>()` extracts only the video track. `pick::<Audio>()` extracts audio,
and `pick::<Media>()` retains both tracks available in the selected content.
`pick::<Visual>()` selects either `Visual::Image(Image)` or `Visual::Video(Media)`, retaining sound;
`pick::<Image>()` selects only images. Each selection type also supports `Vec<T>` for multiple items.
Requests from the same hardware are queued. Dropping an unfinished `Picked` cancels only that
request; dropping the last hardware clone cancels every pending request. Completed requests retain
their result for repeated `get().await` calls.

```rust,ignore
let video = hardware.pick::<Video>().get().await?;
let mut player = video.player();
let audio = hardware.pick::<Audio>().get().await?;
let visuals = hardware.pick::<Vec<Visual>>().get().await?;
```

`Audio::player()`, `Video::player()`, and `Media::player()` create a core `media::Player` with its
own clock and cursor attached to the original timeline. Maverick re-exports this owned core player;
it does not add a shared wrapper. Fresh players begin paused. Keep hardware and the capture handle
while recording; playback does not own capture.

```rust,ignore
use std::time::Duration;
use media::Video;
use maverick_os::PlaybackMode;

let mut camera = hardware.start_camera()?;
let mut recording = Video::new();
let mut player = recording.player();
player.set_mode(PlaybackMode::Live(Duration::ZERO));
player.start();

while let Some(frame) = camera.next().await {
    recording.insert(frame);
}
if let Some(error) = camera.error() {return Err(error)}
```

`set_mode(PlaybackMode)` selects playback behavior. `Default` plays to the available end.
`Live(delay)` follows new content, starting the requested delay behind the current end;
`Loop` repeats the current timeline. Seeking leaves live mode and preserves loop mode.

`Player::frame()` returns the current playback `Frame` immediately. Its `video` field is an
`Option<Arc<VideoFrame>>`; render its `image` when present. Audio-only playback has no video.
`has_video()` reports track presence even during a gap without a current image.
`duration` is `None` in live mode. `deadline()` reports the next presentation deadline; the clock
advances independently of how often the host calls either method. Core playback has no async
subscription. Ramp owns shared playback control and its `frame()`/`next().await` subscription.

`Speaker::new()` opens the default native audio output. `format()` gives its sample rate and
channel count. `request()` reports the needed audio duration and whether output underrun occurred;
the host supplies samples with `write(sound)` and controls output through `set_playing(playing)`.
The queue target adapts to the device callback size. `clear()` discards queued samples;
`set_playing(false)` lets remaining samples drain before pausing. Device errors are returned by
the speaker, without changing the core player.

The host checks `player.audio_revision()` after processing pending edits with `frame()`. A changed
revision invalidates queued output. On a playing underrun or invalidation, it clears the speaker,
calls `player.synchronize_audio()` without changing playback mode, requests a fresh duration, and
renders samples with `player.audio(duration, rate, channels)`. Explicit stops clear the queue;
natural completion drains it. Decoding runs outside the native queue lock. The standalone demo
shows this coordination using the owned player and speaker directly.

`Playback::update(id, playing)` and `Playback::remove(id)` register host playback with native OS
media controls. Hardware events report play, pause, toggle, and volume actions to the application.

## Media editing and export

`Audio::new()`, `Video::new()`, and `Media::new()` create empty timelines. Insert raw frames into
the corresponding track at their timestamps. `Media::insert(frame, placement)` offsets incoming
source timestamps by the supplied placement. Insertion adds time before existing content;
`remove(start..end)` closes the removed interval. `left + right` concatenates recordings.

Cloning a track or media value creates an independent timeline sharing immutable clip payloads
and decoded cache entries. An edit changes only that branch. Cursors and players made from the
original remain attached to it, observe edits, and rebase their positions when content moves.
`duration()` reports the current known length; `video.dimensions()` is `None` for an empty track.

`video.frames(range)` returns `Arc<Vec<Arc<VideoFrame>>>`; `audio.frames(range, duration)` returns
`Arc<Vec<Arc<AudioFrame>>>` using the requested audio chunk duration. `media.frames(range, duration)`
returns both snapshots. Readers at different positions reuse cached decoded frame allocations.
Snapshots remain immutable after edits; a new request reflects the updated timeline. A cursor's
`video()` retrieves the current or next video frame, including when the current position is in a gap.

`Media::read(reader)` accepts an owned `Read + Seek + Send + 'static` source and retains it for
lazy frame access. It probes metadata and may scan encoded packets for missing duration information,
without decoding the whole recording. Pass an owned file to avoid loading its contents into RAM.
`Audio::read` and `Video::read` extract the corresponding track from the same input formats.
`write(writer, format)` accepts an owned `Write + Seek + Send + 'static` destination and finalizes
one file. The explicit `Format` chooses codecs; a file extension does not select the encoder.

The format selects output tracks as well as codecs. Audio-only formats extract the audio track;
when the source has no audio, they write an empty audio track with zero samples and duration.
Video-only formats omit audio.
Combined formats preserve an absent audio track instead of adding one.

| Format | Container | Video codec | Audio codec |
|---|---|---|---|
| `Mp4_Mpeg4p2_Aac { bitrate }` | MP4 | MPEG-4 Part 2 | AAC |
| `Mp4_Mpeg4p2 { bitrate }` | MP4 | MPEG-4 Part 2 | — |
| `Mkv_Ffv1_Flac` | Matroska | FFV1 | FLAC |
| `Mkv_Ffv1_PcmF32le` | Matroska | FFV1 | Float PCM, 32-bit |
| `Mkv_Ffv1` | Matroska | FFV1 | — |
| `Mkv_Opus { bitrate }` | Matroska | — | Opus |
| `Mov_ProRes422_PcmS16le` | MOV | ProRes 422 | Signed PCM, 16-bit |
| `Mov_ProRes422` | MOV | ProRes 422 | — |
| `M4a_Aac` | M4A | — | AAC |
| `M4a_Alac` | M4A | — | ALAC |
| `Wav_PcmS16le` | WAV | — | Signed PCM, 16-bit |
| `Wav_PcmF32le` | WAV | — | Float PCM, 32-bit |
| `Flac_Flac` | FLAC | — | FLAC |

```rust,ignore
let recording = Media::read(std::fs::File::open("recording.mp4")?)?;
let mut excerpt = recording.clone();
excerpt.remove(end..excerpt.duration());
excerpt.remove(std::time::Duration::ZERO..start);
excerpt.write(std::fs::File::create("excerpt.mkv")?, Format::Mkv_Ffv1_PcmF32le)?;
```

`encode(format)` returns an iterator of encoded byte chunks from a fixed timeline snapshot.
It reuses compatible encoded packets where possible and encodes edited or incompatible content
as needed. The encoder chooses chunk boundaries; successive chunks continue one stream and are
not independent files. Bitrates are bits per second and apply when encoding is needed.
`Audio::format()` and `Video::format()` report a recognized original preset when available.

```rust,ignore
for chunk in excerpt.encode(Format::Mp4_Mpeg4p2_Aac {bitrate: 8_000_000})? {
    uploader.send(chunk?).await?;
}
```

The `media` crate owns timelines, decoding, encoding, and playback policy. Maverick owns native
capture, audio output, and OS media controls. Ramp owns shared playback. Applications own network transport and
decide which captured frames to retain. Live playback does not discard recording history.

## Keyboard

`Hardware::keyboard()` returns the shared, clonable `Keyboard` capability without showing it.
Call `show(dismiss_padding)` to request the native keyboard and `hide()` to dismiss it.
Obtaining or dropping a clone does not change visibility. The hardware owner retains the same
capability, so it can be reused after hiding or user dismissal. Native resources are released when
the hardware and its remaining keyboard handles are dropped. On macOS and Linux, `show` and `hide`
do nothing and `visible()` returns false; physical keyboard input continues normally.

Downward-drag dismissal in the surrounding app area is always enabled on iOS and Android.
On iOS the keyboard follows the finger once it reaches `dismiss_padding` logical pixels above
its edge; reversing the drag can cancel dismissal. Pass the height of an app toolbar above the
keyboard, or `0.0` to begin at the keyboard edge. Calling `show` updates this padding even when
the keyboard is already open. The padding is ignored on Android, macOS, and Linux.
Android uses its normal closing animation. Native text selection and native controls keep their
gestures.

The local Winit dependency keeps iOS events and redraws running during UIKit gesture tracking
and prevents its render view from taking keyboard focus unless IME was explicitly enabled.

```rust,ignore
let keyboard = hardware.keyboard();
keyboard.show(0.0);
let shared = keyboard.clone();
drop(keyboard); // Does not dismiss the keyboard.
shared.hide();
shared.show(0.0); // Reuses the same capability.
```

Native text is delivered automatically to `Application::on_input` using the existing
`Input::Device(_, DeviceInput::Keyboard(key, state, modifiers))` events. Each committed Unicode
scalar produces `Key::Character` pressed/released events, including every scalar in pasted
text and emoji sequences. Return, tab and deletion use `Key::Enter`, `Key::Tab` and
`Key::Backspace`/`Key::Delete`. Native text input uses device ID `u64::MAX - 1` and default
modifiers. IME composition stays in the native editor until committed, avoiding duplicate
preedit text.

This is an event stream: the native editor does not own the application's text or selection,
so surrounding-text autocorrection is disabled. Android applications must package Maverick's
`MaverickActivity.java` and `KeyboardView.java`; the provided Android build script includes both.
The hardware demo's **keyboard** button calls `show` and `hide` and shows received text.

`LogicalWindow::size` reports the available area above native keyboard obstruction.
`Input::Resized` carries the changing size throughout opening and closing. Apply only
`window.insets` for the remaining safe area; Maverick removes the covered bottom safe area
along with the keyboard. Space already removed by OS resizing is not subtracted again.
On iOS, undocked and floating keyboards also shorten the available area to their top edge
while they overlap the app. This avoids the keyboard across the full width because window size
and safe-area insets describe one rectangle. On Android, floating keyboards provide no IME
insets and are ignored for layout; their actual visibility is still available from the handle.

`Window::physical()` continues to describe the rendering surface, preserving pixel scale and
touch coordinates. Applications lay out and clip their content to the reported logical size.

iOS tracks the keyboard layout guide's presentation geometry (iOS 17+) with a display link
that runs only during keyboard transitions, including gesture tracking. Native layout callbacks
also track movement of a floating keyboard. Android 11+ supplies native IME animation frames.

`Keyboard::visible()` reports actual native visibility. It is true while any part is visible,
including floating keyboards, opening, closing, and partial dismissal. It becomes false only
when the keyboard is fully gone; a cancelled dismissal keeps it true. `show` and `hide` request
asynchronous native changes, so visibility does not change immediately when those methods return.
`Input::Resized` also arrives for visibility changes that leave the logical size unchanged;
read `keyboard.visible()` when handling it. Window size follows every animation frame independently
of visibility. There is no separate visibility event or public keyboard bounds.

## Installation identity and push registration

`Hardware::device_id()` returns a random 128-bit hexadecimal identifier persisted in `DEVICE_ID`
in the application's support directory. It is shared by cloned handles and survives restarts.
The identifier file is excluded from Apple backups. Removing the application's stored data
creates a new identity on the next launch. On macOS,
removing only the app bundle leaves application support data, so the identifier may survive a
reinstall. It is not a hardware identifier and must not be used as a push address.

`Hardware::push_destination()` returns a `PushDestination` handle that owns native registration
and token updates. Its first `next().await` waits for the platform's current token, even when it
matches a result from an earlier handle or app launch. Subsequent calls wait for a different token.
Repeated native callbacks with the same token do not complete that wait.

```rust,ignore
let mut destination = hardware.push_destination();
let token = destination.next().await;
let replacement = destination.next().await;
```

`PushToken::Apns(token)` contains the Apple token as hexadecimal text;
`PushToken::Fcm(token)` contains the opaque Android FCM token. This result supports serde
serialization and equality comparisons; the handle retains the subscription and last delivered
token. Each handle observes updates independently. Maverick obtains the current token from the
platform SDK each launch rather than trusting a saved token.

The Apple platform adapter shares one native registration for the lifetime of the application. Native
callbacks wake waiting callers. Maverick retries unsuccessful registration internally at most
every 30 seconds while callers wait; concurrent callers share attempts. Waiting for a change to an
available token does not trigger registration retries. Dropping a future cancels only that wait.
Changes received between waits are retained; a later wait returns the latest token when it
differs from the last delivered result. The application's event loop must keep
running to deliver native callbacks and retries. Unchanged or unavailable destinations remain
pending. Existing application delegates continue receiving callbacks.

APNs registration requires a bundled, correctly signed app with the Push Notifications capability.
The notification server must supply its app's APNs topic, environment, and credentials. This API
does not request permission to display notifications or expose permission status; notification
presentation and RAMP's server registration will be added separately. Neither identifier is
exposed through Prism.

## Building

The `media2` dependency supplies validation, export, and playback using statically linked FFmpeg through
`ramp-ffmpeg` 9.0.4 or later, which requires `ramp-ffmpeg-sys` 9.0.5 or later.
iOS device and simulator builds use
bundled precompiled FFmpeg 9.0.2 libraries and Rust bindings. Cargo selects ARM64 device,
ARM64 simulator, or Intel simulator assets;
application builds do not compile FFmpeg or NASM, run Make, or compile a C feature probe.
Rust and the matching Xcode SDK/linker remain required. Minimum deployment targets are iOS
10.0 for ARM64 devices and Intel simulators, and iOS 14.0 for ARM64 simulators.
These minimum dependency versions prevent older lockfiles from selecting sys 9.0.4.

Other platforms compile the FFmpeg 9.0.2 source bundled in the transitive binding crate.
macOS builds use pregenerated bindings and require Rust, Xcode's native compiler and target
SDK, and Make. Intel targets outside iOS build bundled NASM automatically for optimized
assembly. No installed FFmpeg, separate NASM installation, libclang, or FFmpeg source download
is needed for the bundled Apple builds. Subsequent source builds reuse Cargo's build cache.

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
events and `Events::next()`, which schedules active scrolling momentum. Keyboard animation
uses native frame callbacks. An idle application has no recurring 16 ms wake-up. Suspension stops
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

## Linux

The Linux adapter provides window/input integration, XDG application data storage,
persistent installation identity, the existing SQLite cache, clipboard text/images/media/files,
typed file, image, video, and audio picking through XDG Desktop Portal, and local portal notifications.
`share` uses the portal's application chooser for files, text, images, and media. It does not
provide an Apple-style share sheet. Haptic requests do nothing on Linux.

Install a session D-Bus, an appropriate XDG Desktop Portal backend for your desktop,
and the native build dependencies. On Ubuntu/Debian:

```sh
sudo apt install build-essential pkg-config libasound2-dev libwayland-dev libxkbcommon-dev \
  libxkbcommon-x11-0 libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-pulseaudio
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
non-media files as `Data::File`; decodable audio and video become `Data::Media`.
Camera and microphone capture use GStreamer; playback uses the shared CPAL speaker. MPRIS system
controls deliver play, pause, and toggle inputs. Clipboard and share exports use the new media encoder.
There is no Linux remote-push provider, so `PushDestination::next()` remains pending; local
`notify` is independent. Portal availability, permissions, and application identity are
managed by the desktop; unavailable best-effort actions log their errors.

## Android

The Android adapter provides the native window/touch lifecycle, application-private storage,
persistent installation identity, SQLite cache, text/image/media/file clipboard, queued system
typed file, image, video, and audio selection, the Android share sheet, view haptics, and local
notifications. Image selection uses Android's decoder to normalize orientation before returning RGBA pixels.
File selections are imported into app-private storage and stay usable after the picker closes.
Typed media selections are decoded before their temporary imports are removed. `Visual` filters
for images and videos; `Media` filters for audio and video. Generic data picking decodes audio and
video into `Data::Media` and retains other files as `Data::File`.
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

`PushDestination::next()` returns the current SDK token first, then waits for a different token.
Concurrent waits share registration and retries; dropping one future cancels only that wait.
Token rotation while no activity is running is retained by Firebase and read on the next launch.
Unconfigured builds keep the push wait pending and log the missing configuration once per attempt.

Push registration requests Android's notification permission when the activity is resumed.
Foreground local and remote notifications emit `Input::Notification`; background notifications
use the native notification tray. Tapping a notification opens the app. Firebase can deliver
server notification payloads after ordinary process death, but not while the user has force-stopped
the app. Google Play services and notification permission are required. The same AIR notification
server handles Android and Apple; FCM supplies the Android transport.
Camera2 and AudioRecord provide camera, microphone, and combined frame streams. The native bridge
owns runtime permissions and releases each device after its last capture request ends. MediaSession
publishes playback state and media notifications; audio focus and route changes become application
inputs. The shared speaker follows the platform's default output route. Clipboard and share exports
use the new media encoder.

The application cache owns its SQLite connection at `ApplicationSupport/CACHE`; startup does not
change the process working directory. Opening this required cache is a startup invariant and panics
if it fails. `get_cache(key)` returns `Option<Data>`: missing or unreadable entries return `None`,
with read and decoding failures logged internally. Existing SQLite busy handling waits for transient
locks. `set_cache` still returns write and media export errors. RAMP replaces an unavailable or invalid
saved AIR secret with a newly generated one and attempts to save it for the next launch.
