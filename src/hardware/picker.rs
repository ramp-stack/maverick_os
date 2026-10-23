use crossfire::{spsc, AsyncRx, AsyncTx, TryRecvError};

use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use super::{Audio, Data, Hardware, Image, Media, Video, Visual, _Hardware};

pub type PickResult = Result<Vec<Data>, Cancled>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancled;

impl std::fmt::Display for Cancled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Selection cancelled")
    }
}

impl std::error::Error for Cancled {}

#[derive(Clone, Copy)]
pub enum PickKind {Image, Video, Audio, Visual, Media, File, Data}

impl PickKind {
    pub fn image(self) -> bool {matches!(self, Self::Image | Self::Visual | Self::Data)}
    pub fn video(self) -> bool {matches!(self, Self::Video | Self::Visual | Self::Media | Self::Data)}
    pub fn audio(self) -> bool {matches!(self, Self::Audio | Self::Media | Self::Data)}
    pub fn media(self) -> bool {self.video() || self.audio()}
}

pub trait Pickable: Clone {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest;
    fn from_data(items: Vec<Data>) -> Self;
}

impl Pickable for Data {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Data, tx)
    }
    fn from_data(mut items: Vec<Data>) -> Self {
        assert_eq!(items.len(), 1, "A single-item picker must return one item");
        items.pop().unwrap()
    }
}

impl Pickable for Image {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Image, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::Image(image) => image,
            _ => unreachable!("The photo picker returned a different data type"),
        }
    }
}

impl Pickable for Media {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Media, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::Media(media) => media,
            _ => unreachable!("The media picker returned a different data type"),
        }
    }
}

impl Pickable for Visual {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Visual, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::Image(image) => Self::Image(image),
            Data::Media(media) => Self::Video(Video(media)),
            _ => unreachable!("The visual picker returned a different data type"),
        }
    }
}

impl Pickable for Video {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Video, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {Self(Media::from_data(items))}
}

impl Pickable for Audio {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::Audio, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {Self(Media::from_data(items))}
}

impl Pickable for PathBuf {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        PickRequest::new(PickKind::File, tx)
    }
    fn from_data(items: Vec<Data>) -> Self {
        match Data::from_data(items) {
            Data::File(path) => path,
            _ => unreachable!("The file picker returned a different data type"),
        }
    }
}

impl<T: Pickable> Pickable for Vec<T> {
    fn request(tx: AsyncTx<spsc::One<PickResult>>) -> PickRequest {
        let mut request = T::request(tx);
        request.multiple = true;
        request
    }
    fn from_data(items: Vec<Data>) -> Self {
        items.into_iter().map(|item| T::from_data(vec![item])).collect()
    }
}

pub struct Picked<T: Pickable> {
    rx: Option<AsyncRx<spsc::One<PickResult>>>,
    item: Option<Result<T, Cancled>>,
    hardware: Weak<_Hardware>,
}

impl<T: Pickable> Picked<T> {
    pub(crate) fn new(rx: AsyncRx<spsc::One<PickResult>>, hardware: &Hardware) -> Self {
        Self {rx: Some(rx), item: None, hardware: Rc::downgrade(&hardware.0)}
    }

    pub async fn get(&mut self) -> Result<T, Cancled> {
        if let Some(item) = &self.item {return item.clone()}
        let item = self.rx.as_ref().unwrap().recv().await.unwrap_or(Err(Cancled)).map(T::from_data);
        self.item = Some(item.clone());
        item
    }

    pub fn try_get(&mut self) -> Option<Result<T, Cancled>> {
        if let Some(item) = &self.item {return Some(item.clone())}
        let item = match self.rx.as_ref().unwrap().try_recv() {
            Ok(items) => items.map(T::from_data),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(Cancled),
        };
        self.item = Some(item.clone());
        Some(item)
    }
}

impl<T: Pickable> Drop for Picked<T> {
    fn drop(&mut self) {
        drop(self.rx.take());
        if let Some(hardware) = self.hardware.upgrade() {hardware.picker.get().unwrap().cancel();}
    }
}

pub struct PickRequest {
    id: Arc<()>,
    tx: AsyncTx<spsc::One<PickResult>>,
    kind: PickKind,
    multiple: bool,
    items: Vec<Data>,
}

impl PickRequest {
    fn new(kind: PickKind, tx: AsyncTx<spsc::One<PickResult>>) -> Self {
        PickRequest{id: Arc::new(()), tx, kind, multiple: false, items: Vec::new()}
    }

    pub fn id(&self) -> Arc<()> {self.id.clone()}
    pub fn matches(&self, id: &Arc<()>) -> bool {Arc::ptr_eq(&self.id, id)}
    pub fn disconnected(&self) -> bool {self.tx.is_disconnected()}

    pub fn finish(self) {let _ = self.tx.try_send(Ok(self.items));}
    pub fn cancel(self) {let _ = self.tx.try_send(Err(Cancled));}
    pub fn has_items(&self) -> bool {!self.items.is_empty()}

    pub fn accept(&mut self, items: Vec<Result<Data, String>>) -> Option<String> {
        let mut errors = Vec::new();
        for item in items {
            match item {
                Ok(item) => {
                    let accepted = match &item {
                        Data::Image(_) => self.kind.image(),
                        Data::Media(media) if media.dimensions().is_some() => self.kind.video(),
                        Data::Media(_) => self.kind.audio(),
                        Data::File(_) => matches!(self.kind, PickKind::File | PickKind::Data),
                        Data::Text(_) => matches!(self.kind, PickKind::Data),
                    };
                    if accepted {self.items.push(item);} else {
                        errors.push("The selected item does not match the requested type.".into());
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {return None}
        let count = errors.len();
        let mut message = format!("{count} selected item{} could not be opened.\n\n{}",
            if count == 1 {""} else {"s"}, errors.join("\n"));
        if self.has_items() {
            let count = self.items.len();
            message.push_str(&format!(
                "\n\n{count} valid item{} kept. Choose replacements, use the valid items, or cancel.",
                if count == 1 {" is"} else {"s are"},
            ));
        } else {
            message.push_str("\n\nChoose another selection or cancel.");
        }
        Some(message)
    }

    pub fn kind(&self) -> PickKind {self.kind}

    pub fn multiple(&self) -> bool {self.multiple}
}
