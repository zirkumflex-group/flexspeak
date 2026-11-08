//! Native camera lifetime: callbacks stop before their Rust context is released.
use super::publishing::{Quality, ShareSource, SourceKind, Still};
use anyhow::{Result, anyhow, ensure};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;
use tokio::sync::oneshot;

type FrameCallback = unsafe extern "C" fn(*mut c_void, *const u8, u32, u32, u32);
unsafe extern "C" {
    fn flex_camera_devices(
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
    );
    fn flex_camera_access(context: *mut c_void, callback: unsafe extern "C" fn(*mut c_void, bool));
    fn flex_camera_start(
        id: *const c_char,
        width: u32,
        height: u32,
        fps: u32,
        context: *mut c_void,
        callback: FrameCallback,
    ) -> *mut c_void;
    fn flex_camera_stop(capture: *mut c_void);
}

pub fn sources() -> Vec<ShareSource> {
    unsafe extern "C" fn found(context: *mut c_void, id: *const c_char, name: *const c_char) {
        // SAFETY: synchronous enumeration supplies null-terminated strings, valid in this call.
        unsafe {
            let sources = &mut *context.cast::<Vec<ShareSource>>();
            if sources.len() >= 128 {
                return;
            }
            sources.push(ShareSource {
                kind: SourceKind::Camera(CStr::from_ptr(id).to_string_lossy().into_owned()),
                name: format!(
                    "Camera · {}",
                    CStr::from_ptr(name)
                        .to_string_lossy()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(160)
                        .collect::<String>()
                ),
            });
        }
    }
    let mut sources = Vec::new();
    // SAFETY: enumeration completes before the vector goes out of scope.
    unsafe {
        flex_camera_devices((&mut sources as *mut Vec<ShareSource>).cast(), found);
    }
    sources
}

pub async fn request_access() -> Result<()> {
    unsafe extern "C" fn answered(context: *mut c_void, allowed: bool) {
        // SAFETY: native API calls once; it owns this sender even if capture was cancelled.
        let sender = unsafe { Box::from_raw(context.cast::<oneshot::Sender<bool>>()) };
        let _ = sender.send(allowed);
    }
    let (sender, receiver) = oneshot::channel();
    // SAFETY: ownership passes to `answered`, which frees it when the OS responds.
    unsafe {
        flex_camera_access(Box::into_raw(Box::new(sender)).cast(), answered);
    }
    ensure!(
        receiver.await.unwrap_or(false),
        "Camera access is not allowed. Enable Flexspeak in your system's camera privacy settings."
    );
    Ok(())
}

type FrameHandler = Box<dyn Fn(&[u8], u32, i32, i32) + Send + Sync>;
struct Context {
    frame: FrameHandler,
}
/// One picture from a camera, which is released again straight after.
pub fn still(id: &str) -> Result<Still> {
    // A camera's first frames are dark while it settles on an exposure.
    const SETTLE_FRAMES: u32 = 12;
    let (taken, picture) = std::sync::mpsc::sync_channel(1);
    let seen = std::sync::atomic::AtomicU32::new(0);
    let capture = Capture::open(
        id,
        Quality::default(),
        move |data, stride, width, height| {
            if seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == SETTLE_FRAMES {
                let _ = taken.try_send(Still::from_bgra(data, stride, width, height));
            }
        },
    )?;
    let still = picture
        .recv_timeout(std::time::Duration::from_secs(3))
        .ok()
        .flatten();
    drop(capture);
    still.ok_or_else(|| anyhow!("The camera did not give a picture"))
}

pub struct Capture {
    native: NonNull<c_void>,
    // Stable address, retained until StopCapture drains the native callback queue.
    _context: Box<Context>,
}
impl Capture {
    pub fn open(
        id: &str,
        quality: Quality,
        frame: impl Fn(&[u8], u32, i32, i32) + Send + Sync + 'static,
    ) -> Result<Self> {
        unsafe extern "C" fn delivered(
            context: *mut c_void,
            bytes: *const u8,
            stride: u32,
            width: u32,
            height: u32,
        ) {
            if bytes.is_null()
                || !(1..=8192).contains(&width)
                || !(1..=8192).contains(&height)
                || stride < width * 4
                || stride > 131072
            {
                return;
            }
            // SAFETY: the native callback keeps this BGRA pixel buffer locked for the call;
            // capture destruction drains callbacks before freeing their boxed context.
            let context = unsafe { &*context.cast::<Context>() };
            let pixels =
                unsafe { std::slice::from_raw_parts(bytes, stride as usize * height as usize) };
            // A Rust panic must never unwind into the system camera callback.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (context.frame)(pixels, stride, width as i32, height as i32);
            }));
        }
        let id = CString::new(id)?;
        let mut context = Box::new(Context {
            frame: Box::new(frame),
        });
        let (width, height) = quality.max_size();
        // SAFETY: the boxed context stays at a stable address for the complete capture lifetime.
        let pointer = unsafe {
            flex_camera_start(
                id.as_ptr(),
                width,
                height,
                quality.fps(),
                (&mut *context as *mut Context).cast(),
                delivered,
            )
        };
        let native = NonNull::new(pointer).ok_or_else(|| anyhow!("Could not open the selected camera. Check camera permission and whether another app is using it."))?;
        Ok(Self {
            native,
            _context: context,
        })
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: this is the sole native owner; stop synchronously drains callbacks.
        unsafe {
            flex_camera_stop(self.native.as_ptr());
        }
    }
}
