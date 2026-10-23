use futures::{StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use futures_timer::Delay;
use maverick_os::inputs::{DeviceInput, Input, Key, KeyboardState, Media as MediaInput, MouseButton, MouseState};
use maverick_os::{
    Application, Camera, Cancled, Data, Hardware, Image, Keyboard,
    LogicalWindow, Media, Picked, PlaybackMode, Player,
};

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

mod demo_render;
use demo_render::{DemoRenderer, Screen, Quad, Button};

pub struct Demo {
    hardware: Hardware,
    layout: Layout,
    camera: Option<Result<(Camera, Player), String>>,
    keyboard: Option<Keyboard>,
    typed: String,
    picked: Option<Selection>,
    progress: Option<Delay>,
    clipboard: Option<Data>,
    cache: Option<Data>,
    players: HashMap<&'static str, Result<Player, String>>,
    mode: PlaybackMode,
    speed: f64,
    volume: f64,
}

impl Demo {
    fn playbacks(&self) -> impl Iterator<Item = &Player> {
        self.camera.iter().filter_map(|camera| camera.as_ref().ok().map(|(_, player)| player))
            .chain(self.players.values().filter_map(|player| player.as_ref().ok()))
    }

    fn change_volume(&mut self, change: f64) {
        self.volume = (self.volume + change).clamp(0.0, 1.0);
        for player in self.playbacks() {player.set_volume(self.volume);}
    }

    fn mode_label(mode: PlaybackMode) -> &'static str {
        match mode {
            PlaybackMode::Default => "default", PlaybackMode::Loop => "loop",
            PlaybackMode::Live(_) => "live",
        }
    }
    fn status(player: &Player) -> String {
        let position = player.position();
        let duration = player.duration();
        let state = if player.error().is_some() {"error"} else if !player.is_playing() {"paused"}
            else if position >= duration {"waiting"} else {"playing"};
        format!("{state} {} {:.1}/{:.1}s\n{:.1}x volume {:.0}%", Self::mode_label(player.mode()),
            position.as_secs_f64(), duration.as_secs_f64(), player.speed(), player.volume() * 100.0)
    }
}

impl Application for Demo {
    type Renderer = DemoRenderer;

    fn name() -> String {"Demo Application Name".into()}

    fn new(hardware: Hardware, window: LogicalWindow) -> Self {
        let clipboard = hardware.get_clipboard();
        let cache = hardware.get_cache("demo").unwrap_or_else(|error| Some(Data::Text(error)));
        Demo{
            hardware, clipboard, cache,
            layout: Layout::new(&window),
            camera: None,
            keyboard: None, typed: String::new(),
            picked: None, progress: None,
            players: HashMap::new(), mode: PlaybackMode::Default, speed: 1.0, volume: 1.0,
        }
    }

    fn draw(&mut self) -> Screen {
        let mut clear = false;
        let picked = match self.picked.as_mut().and_then(|pick| pick.try_get()) {
            Some(Ok(item)) => Some(item),
            Some(Err(Cancled)) => {
                clear = true;
                None
            }
            None => None,
        };
        if clear {self.picked = None}
        let mut quads = [
            Quad{
                label: if self.keyboard.is_some() {"keyboard"} else {"camera"},
                data: if self.keyboard.is_some() {Some(Data::Text(self.typed.clone()))}
                else {self.camera.as_ref().and_then(|camera| match camera {
                    Ok((camera, player)) => camera.error().or_else(|| player.error()).map(Data::Text)
                        .or_else(|| player.frame().image.map(Data::Image)),
                    Err(error) => Some(Data::Text(error.clone())),
                })},
                status: if self.keyboard.is_some() {None} else {
                    self.camera.as_ref().and_then(|camera| camera.as_ref().ok())
                        .map(|(_, player)| Self::status(player))
                },
            },
            Quad{label: "picked", data: picked, status: None},
            Quad{label: "clipboard", data: self.clipboard.clone(), status: None},
            Quad{label: "cache", data: self.cache.clone(), status: None},
        ];
        self.players.retain(|label, _| quads.iter().any(|quad| {
            quad.label == *label && matches!(&quad.data, Some(Data::Media(_)))
        }));
        for quad in &mut quads {
            if !matches!(&quad.data, Some(Data::Media(_))) {continue}
            let player = self.players.entry(quad.label).or_insert_with(|| {
                let player = match quad.data.as_ref().unwrap() {
                    Data::Media(media) => Player::new(media),
                    _ => unreachable!(),
                };
                if let Ok(player) = &player {
                    player.set_mode(self.mode);
                    player.set_speed(self.speed);
                    player.set_volume(self.volume);
                    player.start();
                }
                player
            });
            match player {
                Ok(player) => {
                    quad.status = Some(Self::status(player));
                    if let Some(error) = player.error() {quad.data = Some(Data::Text(error));}
                    else if let Some(frame) = player.frame().image {quad.data = Some(Data::Image(frame));}
                }
                Err(error) => quad.data = Some(Data::Text(format!("Unable to play media: {error}"))),
            }
        }
        let playing = self.playbacks().any(Player::is_playing);
        let mode = self.playbacks().map(|player| Self::mode_label(player.mode()))
            .reduce(|mode, next| if mode == next {mode} else {"mixed"})
            .unwrap_or(Self::mode_label(self.mode));
        for button in &mut self.layout.buttons {
            match button.label {
                "play all" | "pause all" => button.label = if playing {"pause all"} else {"play all"},
                "0.5x" | "1x" | "2x" => button.label = match self.speed {
                    0.5 => "0.5x", 2.0 => "2x", _ => "1x",
                },
                "default" | "loop" | "live" | "mixed" => button.label = mode,
                _ => {}
            }
        }
        let [top_left, top_right, bottom_left, bottom_right] = quads;
        Screen{
            top_left, top_right, bottom_left, bottom_right,
            buttons: self.layout.buttons.clone(),
        }
    }

    async fn changed(&mut self) {
        let advancing = self.playbacks().any(|player| {
            player.is_playing() && player.position() < player.duration()
        });
        if advancing {
            self.progress.get_or_insert_with(|| Delay::new(Duration::from_millis(100)));
        } else {self.progress = None;}
        let mut changes: FuturesUnordered<LocalBoxFuture<'_, ()>> = self.camera.iter_mut()
            .filter_map(|camera| camera.as_mut().ok().map(|(_, player)| player))
            .chain(self.players.values_mut().filter_map(|player| player.as_mut().ok()))
            .map(|player| Box::pin(async move {player.next().await;}) as LocalBoxFuture<'_, ()>).collect();
        if let Some(selection) = &mut self.picked && !matches!(selection, Selection::Ready(_)) {
            changes.push(Box::pin(selection.changed()));
        }
        if self.progress.is_some() {
            changes.push(Box::pin(async {
                self.progress.as_mut().unwrap().await;
                self.progress = None;
            }));
        }
        if changes.is_empty() {std::future::pending::<()>().await;}
        let _ = changes.next().await;
    }

    fn on_input(&mut self, input: Input) {
        match input {
            Input::Device(_, DeviceInput::Keyboard(key, KeyboardState::Pressed | KeyboardState::Repeated, _))
                if self.keyboard.is_some() => match key {
                    Key::Character(character) => self.typed.push(character),
                    Key::Backspace => {self.typed.pop();}
                    Key::Enter => self.typed.push('\n'),
                    Key::Tab => self.typed.push('\t'),
                    Key::Escape => self.keyboard = None,
                    _ => {}
                },
            Input::Media(media @ (MediaInput::Play | MediaInput::Pause)) => {
                for player in self.playbacks() {
                    if media == MediaInput::Play {player.start();} else {player.stop();}
                }
            }
            Input::Media(MediaInput::VolumeUp) => self.change_volume(0.1),
            Input::Media(MediaInput::VolumeDown) => self.change_volume(-0.1),
            Input::Resized(window) => self.layout = Layout::new(&window),
            Input::Device(_, DeviceInput::Mouse((x, y), MouseState::Pressed(MouseButton::Left))) => {
                if let Some(label) = self.layout.hit(x, y) {
                    self.hardware.trigger_haptic();
                    let changed = match label {
                        "photo" | "pick media" | "file" | "data" => Some("picked"),
                        "paste" => Some("clipboard"), "set cache" => Some("cache"), _ => None,
                    };
                    if let Some(label) = changed {self.players.remove(label);}
                    match label {
                        "keyboard" => if self.keyboard.take().is_none() {
                            self.keyboard = Some(self.hardware.open_keyboard());
                        },
                        "camera" => if self.camera.take().is_none() {
                            self.camera = Some(self.hardware.start_camera().and_then(|camera| {
                                let player = Player::new(&camera)?;
                                player.set_mode(PlaybackMode::Live(Duration::ZERO));
                                player.set_speed(self.speed);
                                player.set_volume(self.volume);
                                player.start();
                                Ok((camera, player))
                            }));
                        },
                        "photo" => self.picked = Some(Selection::Photo(self.hardware.pick::<Image>())),
                        "pick media" => {
                            self.picked = Some(Selection::Media(self.hardware.pick::<Media>()));
                        }
                        "file" => self.picked = Some(Selection::File(self.hardware.pick::<PathBuf>())),
                        "data" => self.picked = Some(Selection::Data(self.hardware.pick::<Data>())),
                        "paste" => self.clipboard = self.hardware.get_clipboard(),
                        "share" | "copy" | "set cache" => {
                            let data = self.picked.as_ref().and_then(Selection::try_get)
                                .and_then(Result::ok).or_else(|| self.clipboard.clone());
                            if let Some(data) = data {match label {
                                "share" => self.hardware.share(&data),
                                "copy" => self.hardware.set_clipboard(&data),
                                _ => {
                                    self.cache = match self.hardware.set_cache("demo", &data) {
                                        Ok(()) => self.hardware.get_cache("demo")
                                            .unwrap_or_else(|error| Some(Data::Text(error))),
                                        Err(error) => Some(Data::Text(error)),
                                    };
                                }
                            }}
                        }
                        "play all" | "pause all" => {
                            let playing = self.playbacks().any(Player::is_playing);
                            for player in self.playbacks() {
                                if playing {player.stop();} else {player.start();}
                            }
                        }
                        "0.5x" | "1x" | "2x" => {
                            self.speed = match self.speed {0.5 => 1.0, 1.0 => 2.0, _ => 0.5};
                            for player in self.playbacks() {player.set_speed(self.speed);}
                        }
                        "default" | "loop" | "live" | "mixed" => {
                            self.mode = match label {
                                "default" => PlaybackMode::Loop,
                                "loop" => PlaybackMode::Live(Duration::ZERO),
                                _ => PlaybackMode::Default,
                            };
                            for player in self.playbacks() {player.set_mode(self.mode);}
                        }
                        "restart" | "-5s" | "+5s" => {
                            for player in self.playbacks() {
                                let position = match label {
                                    "-5s" => player.position()
                                        .saturating_sub(Duration::from_secs(5)),
                                    "+5s" => (player.position() + Duration::from_secs(5))
                                        .min(player.duration()),
                                    _ => Duration::ZERO,
                                };
                                player.seek(position);
                            }
                            if matches!(self.mode, PlaybackMode::Live(_)) {
                                self.mode = PlaybackMode::Default;
                            }
                        }
                        "volume -" => self.change_volume(-0.1),
                        "volume +" => self.change_volume(0.1),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

maverick_os::start!(Demo);

enum Selection {
    Photo(Picked<Image>),
    Media(Picked<Media>),
    File(Picked<PathBuf>),
    Data(Picked<Data>),
    Ready(Result<Data, Cancled>),
}

impl Selection {
    async fn changed(&mut self) {
        let result = match self {
            Self::Photo(picked) => picked.get().await.map(Data::Image),
            Self::Media(picked) => picked.get().await.map(Data::Media),
            Self::File(picked) => picked.get().await.map(Data::File),
            Self::Data(picked) => picked.get().await,
            Self::Ready(_) => std::future::pending().await,
        };
        *self = Self::Ready(result);
    }
    fn try_get(&self) -> Option<Result<Data, Cancled>> {
        match self {Self::Ready(result) => Some(result.clone()), _ => None}
    }
}

struct Layout {
    buttons: Vec<Button>,
}

impl Layout {
    fn new(window: &LogicalWindow) -> Self {
        let labels = [
            "camera", "photo", "pick media", "file",
            "data", "share", "copy", "paste", "set cache", "play all", "1x", "keyboard",
            "default", "restart", "-5s", "+5s", "volume -", "volume +",
        ];
        let h = 32.0;
        let gap = 4.0;
        let (top, left, bottom, right) = window.insets;
        let width = (window.size.0 - left - right).max(0.0);
        let columns = (((width - gap) / 96.0) as usize).clamp(1, labels.len());
        let rows = labels.len().div_ceil(columns);
        let y = (window.size.1 - bottom - rows as f32 * (h + gap)).max(top);
        let slot = (width - gap).max(0.0) / columns as f32;
        Layout{buttons: labels.into_iter().enumerate().map(|(i, label)| Button {
            label, x: left + gap + (i % columns) as f32 * slot, y: y + (i / columns) as f32 * (h + gap),
            w: (slot - gap).max(0.0), h,
        }).collect()}
    }

    fn hit(&self, x: f32, y: f32) -> Option<&'static str> {
        self.buttons.iter().find(|button| x >= button.x && x < button.x + button.w
            && y >= button.y && y < button.y + button.h).map(|button| button.label)
    }
}
