//! Screen pictures from the iOS broadcast extension, on their way to the running share.

use std::sync::{
    Mutex,
    atomic::{AtomicU32, Ordering},
};

/// One NV12 picture; the slices are valid only during delivery.
pub struct Picture<'a> {
    pub y: &'a [u8],
    pub y_stride: u32,
    pub uv: &'a [u8],
    pub uv_stride: u32,
    pub width: u32,
    pub height: u32,
}

type Sink = Box<dyn FnMut(Picture<'_>) + Send>;

static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static ROTATION: AtomicU32 = AtomicU32::new(0);

/// Receives pictures until the guard is dropped.
pub struct Attached;

pub fn attach(sink: Sink) -> Attached {
    *SINK.lock().unwrap() = Some(sink);
    Attached
}

impl Drop for Attached {
    fn drop(&mut self) {
        *SINK.lock().unwrap() = None;
    }
}

pub fn deliver(picture: Picture<'_>, rotation: u32) {
    ROTATION.store(rotation, Ordering::Relaxed);
    if let Some(sink) = SINK.lock().unwrap().as_mut() {
        sink(picture);
    }
}

/// How far the newest picture is turned, in degrees.
pub fn rotation() -> u32 {
    ROTATION.load(Ordering::Relaxed)
}
