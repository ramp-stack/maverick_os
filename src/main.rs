mod demo_render;
use demo_render::{DemoRenderer, Screen, Quad, Button};
use std::path::PathBuf;
use std::sync::Arc;

use maverick_os::inputs::{DeviceInput, Input, MouseButton, MouseState};
use maverick_os::{
    Application, Camera, Cancled, Data, Hardware, Image, LogicalWindow, Media, Picked, Player, Video,
};

pub struct Demo {
    hardware: Hardware,
    layout: Layout,
    camera: Option<Camera>,
    picked: Option<Selection>,
    clipboard: Option<Data>,
    cache: Option<Data>,
    players: Vec<(Video, Result<Player, String>)>,
    speed: f64,
}

impl Application for Demo {
    type Renderer = DemoRenderer;

    fn name() -> String {"Demo Application Name".into()}

    fn new(hardware: Hardware, window: LogicalWindow) -> Self {
        let clipboard = hardware.get_clipboard();
        let cache = hardware.get_cache("demo");
        Demo{
            hardware, clipboard, cache,
            layout: Layout::new(&window),
            camera: None,
            picked: None,
            players: vec![], speed: 1.0,
        }
    }

    fn on_tick(&mut self) -> Screen {
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
                label: "camera",
                data: self.camera.as_ref().map(|camera| Data::Image(camera.frame())),
            },
            Quad{label: "picked", data: picked},
            Quad{label: "clipboard", data: self.clipboard.clone()},
            Quad{label: "cache", data: self.cache.clone()},
        ];
        self.players.retain(|(video, _)| quads.iter().any(|quad| match &quad.data {
            Some(Data::Video(current)) => Arc::ptr_eq(video.bytes(), current.bytes()),
            _ => false,
        }));
        for quad in &mut quads {
            let Some(Data::Video(video)) = &quad.data else {continue};
            let index = self.players.iter().position(|(current, _)| {
                Arc::ptr_eq(current.bytes(), video.bytes())
            }).unwrap_or_else(|| {
                let player = Player::new(video);
                if let Ok(player) = &player {player.set_speed(self.speed); player.start();}
                self.players.push((video.clone(), player));
                self.players.len() - 1
            });
            quad.data = Some(match &self.players[index].1 {
                Ok(player) => Data::Image(player.frame()),
                Err(error) => Data::Text(format!("Unable to open video: {error}")),
            });
        }
        let playing = self.players.iter().any(|(_, player)| {
            player.as_ref().is_ok_and(|player| player.is_playing())
        });
        for button in &mut self.layout.buttons {
            match button.label {
                "play all" | "pause all" => button.label = if playing {"pause all"} else {"play all"},
                "0.5x" | "1x" | "2x" => button.label = match self.speed {
                    0.5 => "0.5x", 2.0 => "2x", _ => "1x",
                },
                _ => {}
            }
        }
        let [top_left, top_right, bottom_left, bottom_right] = quads;
        Screen{
            top_left, top_right, bottom_left, bottom_right,
            buttons: self.layout.buttons.clone(),
        }
    }

    fn on_input(&mut self, input: Input) {
        match input {
            Input::Resized(window) => self.layout = Layout::new(&window),
            Input::Device(_, DeviceInput::Mouse((x, y), MouseState::Pressed(MouseButton::Left))) => {
                if let Some(label) = self.layout.hit(x, y) {
                    self.hardware.trigger_haptic();
                    match label {
                        "camera" => if self.camera.take().is_none() {
                            self.camera = Some(self.hardware.start_camera());
                        },
                        "photo" => self.picked = Some(Selection::Photo(self.hardware.pick::<Image>())),
                        "video" => self.picked = Some(Selection::Video(self.hardware.pick::<Video>())),
                        "pick media" => {
                            self.picked = Some(Selection::Media(self.hardware.pick::<Media>()));
                        }
                        "file" => self.picked = Some(Selection::File(self.hardware.pick::<PathBuf>())),
                        "data" => self.picked = Some(Selection::Data(self.hardware.pick::<Data>())),
                        "share" => if let Some(data) = self.clipboard.as_ref() {
                            self.hardware.share(data);
                        },
                        "copy" => if let Some(data) = self.clipboard.clone() {
                            self.hardware.set_clipboard(&data);
                        },
                        "paste" => self.clipboard = self.hardware.get_clipboard(),
                        "set cache" => if let Some(data) = self.clipboard.clone() {
                            self.hardware.set_cache("demo", &data);
                            self.cache = self.hardware.get_cache("demo");
                        },
                        "play all" | "pause all" => {
                            let playing = self.players.iter().any(|(_, player)| {
                                player.as_ref().is_ok_and(|player| player.is_playing())
                            });
                            for (_, player) in &self.players {
                                if let Ok(player) = player {
                                    if playing {player.stop();} else {player.start();}
                                }
                            }
                        }
                        "0.5x" | "1x" | "2x" => {
                            self.speed = match self.speed {0.5 => 1.0, 1.0 => 2.0, _ => 0.5};
                            for (_, player) in &self.players {
                                if let Ok(player) = player {player.set_speed(self.speed);}
                            }
                        }
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
    Video(Picked<Video>),
    Media(Picked<Media>),
    File(Picked<PathBuf>),
    Data(Picked<Data>),
}

impl Selection {
    fn try_get(&mut self) -> Option<Result<Data, Cancled>> {
        Some(match self {
            Self::Photo(picked) => picked.try_get()?.map(Data::Image),
            Self::Video(picked) => picked.try_get()?.map(Data::Video),
            Self::Media(picked) => picked.try_get()?.map(|media| match media {
                Media::Image(image) => Data::Image(image),
                Media::Video(video) => Data::Video(video),
            }),
            Self::File(picked) => picked.try_get()?.map(Data::File),
            Self::Data(picked) => picked.try_get()?
        })
    }
}

struct Layout {buttons: Vec<Button>}

impl Layout {
    fn new(window: &LogicalWindow) -> Self {
        let labels = [
            "camera", "photo", "video", "pick media", "file",
            "data", "share", "copy", "paste", "set cache", "play all", "1x",
        ];
        let h = 32.0;
        let gap = 4.0;
        let columns = (((window.size.0 - gap) / 96.0) as usize).clamp(1, labels.len());
        let rows = labels.len().div_ceil(columns);
        let y = (window.size.1 - rows as f32 * (h + gap)).max(0.0);
        let slot = (window.size.0 - gap).max(0.0) / columns as f32;
        Layout{buttons: labels.into_iter().enumerate().map(|(i, label)| Button {
            label, x: gap + (i % columns) as f32 * slot, y: y + (i / columns) as f32 * (h + gap),
            w: (slot - gap).max(0.0), h,
        }).collect()}
    }

    fn hit(&self, x: f32, y: f32) -> Option<&'static str> {
        self.buttons.iter().find(|button| x >= button.x && x < button.x + button.w
            && y >= button.y && y < button.y + button.h).map(|button| button.label)
    }
}
