//! Experimental Windows screen and window capture that never waits for the GPU.
//!
//! WebRTC's own Windows capture copies each frame into CPU memory and then blocks until the
//! copy is done. While a game keeps the GPU busy that copy queues behind the game, so the
//! capture thread stalls for most of every frame and NVIDIA's driver busy-polls the GPU the
//! whole time (measured: 38–90% of a core). This capture instead:
//!
//! - takes the newest frame from Windows Graphics Capture on each tick,
//! - scales it to the share size and converts it to NV12 on the GPU (Direct3D 11's video
//!   processor), so only about a fifth of the data comes back,
//! - copies it into a small ring of CPU-readable textures and reads back only frames the GPU
//!   has already finished. When none is finished yet the tick sends nothing new, and the
//!   video source repeats the last picture. Nothing ever waits.
//!
//! Frames reach WebRTC as I420, exactly as from the standard capture, so both encoders
//! (NVENC AV1 and VP8) are unaffected.

use anyhow::{Context, Result, anyhow, bail, ensure};
use libwebrtc::{native::yuv_helper, video_frame::I420Buffer};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::{HMODULE, HWND, LPARAM, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::*,
            Dxgi::{Common::*, DXGI_ERROR_WAS_STILL_DRAWING, IDXGIDevice},
            Gdi::{
                DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC,
                HMONITOR, MONITORINFO, MONITORINFOEXW,
            },
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize,
        },
    },
    core::{BOOL, IInspectable, Interface, PCWSTR, factory},
};

/// Readback slots: one being written by the GPU, one finishing, one spare.
const SLOTS: usize = 3;
/// Frames Windows keeps for us; the newest is taken and the rest are returned at once.
const POOL_BUFFERS: i32 = 2;
const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
/// A queued readback that has not finished in this long means the GPU stopped working for
/// us (for example after a driver reset).
const STALLED: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A screen id from WebRTC's source list: its index among display devices.
    Screen(u64),
    /// A window id from WebRTC's source list: its window handle.
    Window(u64),
}

/// The picture size the share sends: the source scaled down to fit `max`, never up, in even
/// dimensions. Matches what the standard capture produces.
pub fn fit(width: u32, height: u32, (max_width, max_height): (u32, u32)) -> (u32, u32) {
    let scale = (f64::from(max_width) / f64::from(width))
        .min(f64::from(max_height) / f64::from(height))
        .min(1.0);
    let even = |value: f64| ((value as u32) / 2 * 2).max(2);
    (
        even(f64::from(width) * scale),
        even(f64::from(height) * scale),
    )
}

/// The part of a source of size `input` that is converted. A source that already fits is
/// cropped to the even size `fit` gives it, losing at most one pixel of edge instead of
/// being resampled; a larger one is scaled down whole.
fn crop(input: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    if input.0 <= max.0 && input.1 <= max.1 {
        fit(input.0, input.1, max)
    } else {
        input
    }
}

struct Slot {
    staging: ID3D11Texture2D,
    /// When its copy was queued; `None` while the slot is free.
    queued_at: Option<Instant>,
}

/// What depends on the source and share sizes; rebuilt when the source is resized.
struct Pipeline {
    input: (u32, u32),
    /// The part of the input that is converted: all of it when it is scaled down, or the
    /// even-sized part when it already fits, so an odd-sized window loses one pixel of edge
    /// instead of being resampled (which would soften all of its text).
    crop: (u32, u32),
    output: (u32, u32),
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    target: ID3D11Texture2D,
    target_view: ID3D11VideoProcessorOutputView,
    /// `target` holds a converted frame that is not yet queued for readback.
    target_fresh: bool,
    /// Input views for the pool's few textures, by texture.
    inputs: Vec<(ID3D11Texture2D, ID3D11VideoProcessorInputView)>,
    slots: Vec<Slot>,
    /// Slots in the order their copies were queued.
    queued: VecDeque<usize>,
}

pub struct Capture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    winrt_device: IDirect3DDevice,
    /// Kept alive for the session; Windows stops the capture when it is released.
    _item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    pool_size: SizeInt32,
    max_size: (u32, u32),
    pipeline: Pipeline,
    closed: Arc<AtomicBool>,
    /// Ticks on which a converted frame waited because every slot was still on the GPU.
    pub skipped: u64,
}

impl Capture {
    /// Starts capturing `target`. Fails if Windows Graphics Capture or the GPU video processor
    /// is unavailable; the caller then uses the standard capture.
    pub fn open(target: Target, max_size: (u32, u32), frame_interval: Duration) -> Result<Self> {
        // SAFETY: each call below is a plain COM/WinRT call with valid arguments; the
        // interfaces they return are reference counted by the `windows` crate.
        unsafe {
            // Already initialised on this thread (in either mode) is fine.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
            ensure!(
                GraphicsCaptureSession::IsSupported().unwrap_or(false),
                "Windows Graphics Capture is not supported"
            );
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("Cannot create the capture device")?;
            let device: ID3D11Device = device.context("No capture device")?;
            let context: ID3D11DeviceContext = context.context("No capture device context")?;
            let video_device: ID3D11VideoDevice =
                device.cast().context("The GPU has no video processor")?;
            let video_context: ID3D11VideoContext = context.cast()?;
            let dxgi: IDXGIDevice = device.cast()?;
            // A scheduling hint for this device's GPU work. Best effort and not measured on
            // its own: not waiting for the GPU is what removes the stall.
            let _ = dxgi.SetGPUThreadPriority(7);
            let winrt_device: IDirect3DDevice =
                CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;

            let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = match target {
                Target::Screen(id) => interop.CreateForMonitor(monitor(id)?)?,
                Target::Window(id) => interop.CreateForWindow(HWND(id as usize as *mut _))?,
            };
            let closed = Arc::new(AtomicBool::new(false));
            let flag = closed.clone();
            item.Closed(
                &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                    flag.store(true, Ordering::Release);
                    Ok(())
                }),
            )?;
            let pool_size = item.Size()?;
            ensure!(
                pool_size.Width > 0 && pool_size.Height > 0,
                "The selected screen or window has no size"
            );
            // Built before capture starts, so a video processor that cannot do the
            // conversion means standard capture instead of a failed share.
            let pipeline = build_pipeline(
                &device,
                &video_device,
                &video_context,
                (pool_size.Width as u32, pool_size.Height as u32),
                max_size,
            )?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &winrt_device,
                PIXEL_FORMAT,
                POOL_BUFFERS,
                pool_size,
            )?;
            let session = pool.CreateCaptureSession(&item)?;
            let _ = session.SetIsCursorCaptureEnabled(true);
            // Both are newer than the capture API itself; older Windows keeps its defaults.
            let _ = session.SetIsBorderRequired(false);
            // Half a tick: a whole one, rounded up to the screen's refresh, would make
            // Windows skip a tick's frame now and then.
            let _ = session.SetMinUpdateInterval(TimeSpan {
                Duration: (frame_interval.as_nanos() / 200) as i64,
            });
            session.StartCapture()?;
            Ok(Self {
                device,
                context,
                video_device,
                video_context,
                winrt_device,
                _item: item,
                pool,
                session,
                pool_size,
                max_size,
                pipeline,
                closed,
                skipped: 0,
            })
        }
    }

    /// Called once per video tick. Returns the newest frame the GPU has finished since the
    /// last call, if any, and converts the newest captured frame for a later call. Never
    /// waits for the GPU. Windows only sends frames when the picture changes, so the caller
    /// sends its last frame again on ticks that return `None`.
    pub fn poll(&mut self) -> Result<Option<I420Buffer>> {
        if self.closed.load(Ordering::Acquire) {
            bail!("Screen capture stopped. Check permission or select another window.");
        }
        self.step().map_err(|error| {
            anyhow!(
                "Fast capture stopped working ({error:#}). Turn off Fast capture in the share dialog and share again."
            )
        })
    }

    fn step(&mut self) -> Result<Option<I420Buffer>> {
        let ready = self.collect()?;
        self.submit()?;
        self.queue_readback();
        Ok(ready)
    }

    /// Reads back the newest queued frame the GPU has finished; older finished ones are only
    /// released.
    fn collect(&mut self) -> Result<Option<I420Buffer>> {
        let pipeline = &mut self.pipeline;
        if let Some(&front) = pipeline.queued.front()
            && pipeline.slots[front]
                .queued_at
                .is_some_and(|at| at.elapsed() > STALLED)
        {
            bail!(
                "the GPU has not finished a frame in {} s",
                STALLED.as_secs()
            );
        }
        let mut finished: Vec<(usize, D3D11_MAPPED_SUBRESOURCE)> = Vec::new();
        while let Some(&index) = pipeline.queued.front() {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: the staging texture is ours and not mapped elsewhere; DO_NOT_WAIT makes
            // an unfinished copy return WAS_STILL_DRAWING instead of blocking.
            let result = unsafe {
                self.context.Map(
                    &pipeline.slots[index].staging,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
            };
            match result {
                Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => break,
                Err(error) => {
                    // Release what was mapped before failing.
                    for (index, _) in finished {
                        // SAFETY: mapped above and not yet unmapped.
                        unsafe { self.context.Unmap(&pipeline.slots[index].staging, 0) };
                    }
                    return Err(error).context("Cannot read the captured frame");
                }
                Ok(()) => {}
            }
            pipeline.queued.pop_front();
            finished.push((index, mapped));
        }
        let (width, height) = pipeline.output;
        let mut newest = None;
        let last = finished.len().saturating_sub(1);
        for (position, (index, mapped)) in finished.into_iter().enumerate() {
            if position == last {
                newest = Some(nv12_frame(&mapped, width, height));
            }
            // SAFETY: mapped above; `nv12_frame` has finished with the memory.
            unsafe { self.context.Unmap(&pipeline.slots[index].staging, 0) };
            pipeline.slots[index].queued_at = None;
        }
        newest.transpose()
    }

    /// Converts the newest captured frame into `target` on the GPU.
    fn submit(&mut self) -> Result<()> {
        let mut frame = None;
        // Returning older frames to the pool at once lets Windows keep capturing.
        loop {
            match self.pool.TryGetNextFrame() {
                Ok(next) => {
                    if let Some(previous) = frame.replace(next) {
                        let _ = previous.Close();
                    }
                }
                // No frame waiting comes back as a null result, not a failure.
                Err(error) if error.code().is_ok() => break,
                Err(error) => return Err(error).context("Cannot take the captured frame"),
            }
        }
        let Some(frame) = frame else {
            return Ok(());
        };
        let content = frame.ContentSize().context("Cannot read the frame size")?;
        // A minimised window reports no size: keep its last picture and the pool as it is.
        if content.Width <= 0 || content.Height <= 0 {
            let _ = frame.Close();
            return Ok(());
        }
        let result = self.convert(&frame, content);
        let _ = frame.Close();
        if content.Width != self.pool_size.Width || content.Height != self.pool_size.Height {
            // The window was resized: later frames come at the new size. The frame just
            // converted (cropped to the content) is still read back.
            self.pool_size = content;
            self.pool
                .Recreate(&self.winrt_device, PIXEL_FORMAT, POOL_BUFFERS, content)
                .context("Cannot resize the capture")?;
        }
        result
    }

    fn convert(
        &mut self,
        frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
        content: SizeInt32,
    ) -> Result<()> {
        // SAFETY: the surface of a capture frame is a D3D11 texture on our device.
        let texture: ID3D11Texture2D = unsafe {
            frame
                .Surface()
                .and_then(|surface| surface.cast::<IDirect3DDxgiInterfaceAccess>())
                .and_then(|access| access.GetInterface())
                .context("Cannot reach the captured texture")?
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `desc` is a valid out pointer.
        unsafe { texture.GetDesc(&mut desc) };
        // The content can be smaller than the pool's textures right after a resize.
        let input = (
            (content.Width as u32).min(desc.Width),
            (content.Height as u32).min(desc.Height),
        );
        if self.pipeline.input != input {
            self.pipeline = build_pipeline(
                &self.device,
                &self.video_device,
                &self.video_context,
                input,
                self.max_size,
            )?;
        }
        let pipeline = &mut self.pipeline;
        let view = match pipeline.inputs.iter().find(|(t, _)| *t == texture) {
            Some((_, view)) => view.clone(),
            None => {
                let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    FourCC: 0,
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_VPIV {
                            MipSlice: 0,
                            ArraySlice: 0,
                        },
                    },
                };
                let mut view = None;
                // SAFETY: valid texture, enumerator and descriptor.
                unsafe {
                    self.video_device.CreateVideoProcessorInputView(
                        &texture,
                        &pipeline.enumerator,
                        &desc,
                        Some(&mut view),
                    )?;
                }
                let view = view.context("No video processor input view")?;
                // The pool reuses a couple of textures; anything beyond that is stale.
                if pipeline.inputs.len() >= 4 {
                    pipeline.inputs.remove(0);
                }
                pipeline.inputs.push((texture.clone(), view.clone()));
                view
            }
        };
        let source = RECT {
            left: 0,
            top: 0,
            right: pipeline.crop.0 as i32,
            bottom: pipeline.crop.1 as i32,
        };
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(view)),
            ..Default::default()
        };
        let mut streams = [stream];
        // SAFETY: all objects belong to this device; the stream array outlives the call.
        // Writing `target` while an earlier copy out of it is queued is fine: one context
        // runs its commands in order.
        let blit = unsafe {
            self.video_context.VideoProcessorSetStreamSourceRect(
                &pipeline.processor,
                0,
                true,
                Some(&source),
            );
            self.video_context.VideoProcessorBlt(
                &pipeline.processor,
                &pipeline.target_view,
                0,
                &streams,
            )
        };
        // The stream holds a reference to the input view.
        // SAFETY: dropped exactly once, after the call that used it.
        unsafe { std::mem::ManuallyDrop::drop(&mut streams[0].pInputSurface) };
        blit.context("The GPU could not convert the captured frame")?;
        pipeline.target_fresh = true;
        Ok(())
    }

    /// Queues the readback of a freshly converted frame into a free slot. With every slot
    /// still on the GPU it waits in `target` for a later tick rather than being dropped.
    fn queue_readback(&mut self) {
        let pipeline = &mut self.pipeline;
        if !pipeline.target_fresh {
            return;
        }
        let Some(index) = (0..SLOTS).find(|&i| pipeline.slots[i].queued_at.is_none()) else {
            self.skipped += 1;
            return;
        };
        let slot = &mut pipeline.slots[index];
        // SAFETY: same-size, same-format textures on one device. Flush starts the work now
        // instead of when the context's buffer fills.
        unsafe {
            self.context.CopyResource(&slot.staging, &pipeline.target);
            self.context.Flush();
        }
        slot.queued_at = Some(Instant::now());
        pipeline.queued.push_back(index);
        pipeline.target_fresh = false;
    }
}

/// The video processor, its NV12 target and the readback ring for one source size.
fn build_pipeline(
    device: &ID3D11Device,
    video_device: &ID3D11VideoDevice,
    video_context: &ID3D11VideoContext,
    input: (u32, u32),
    max_size: (u32, u32),
) -> Result<Pipeline> {
    let output = fit(input.0, input.1, max_size);
    let crop = crop(input, max_size);
    let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        },
        InputWidth: input.0,
        InputHeight: input.1,
        OutputFrameRate: DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        },
        OutputWidth: output.0,
        OutputHeight: output.1,
        Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
    };
    // SAFETY: plain D3D11 object creation with valid descriptors.
    unsafe {
        let enumerator = video_device.CreateVideoProcessorEnumerator(&content)?;
        let input_ok = enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)?;
        let output_ok = enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_NV12)?;
        ensure!(
            input_ok & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32 != 0
                && output_ok & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 != 0,
            "The GPU video processor cannot convert BGRA to NV12"
        );
        let processor = video_device.CreateVideoProcessor(&enumerator, 0)?;
        // Full-range RGB in, BT.601 limited-range YCbCr out: what libyuv's ARGBToI420
        // produces for the standard capture, so pictures look the same either way.
        // Bit layout: Usage, RGB_Range, YCbCr_Matrix, YCbCr_xvYCC, Nominal_Range(2).
        let rgb_full = D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
        let ycbcr_601_studio = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
            _bitfield: (D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235.0 as u32) << 4,
        };
        video_context.VideoProcessorSetStreamColorSpace(&processor, 0, &rgb_full);
        video_context.VideoProcessorSetOutputColorSpace(&processor, &ycbcr_601_studio);
        video_context.VideoProcessorSetStreamFrameFormat(
            &processor,
            0,
            D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        );
        // No denoise, edge enhancement or other driver "improvements" on screen content.
        video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: output.0,
            Height: output.1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut target = None;
        device.CreateTexture2D(&desc, None, Some(&mut target))?;
        let target = target.context("No NV12 target")?;
        let view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut target_view = None;
        video_device.CreateVideoProcessorOutputView(
            &target,
            &enumerator,
            &view_desc,
            Some(&mut target_view),
        )?;
        let target_view = target_view.context("No video processor output view")?;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut slots = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            let mut staging = None;
            device.CreateTexture2D(&desc, None, Some(&mut staging))?;
            slots.push(Slot {
                staging: staging.context("No readback texture")?,
                queued_at: None,
            });
        }
        Ok(Pipeline {
            input,
            crop,
            output,
            enumerator,
            processor,
            target,
            target_view,
            target_fresh: false,
            inputs: Vec::new(),
            slots,
            queued: VecDeque::new(),
        })
    }
}

/// Copies a mapped NV12 staging texture into a new I420 frame.
fn nv12_frame(mapped: &D3D11_MAPPED_SUBRESOURCE, width: u32, height: u32) -> Result<I420Buffer> {
    let pitch = mapped.RowPitch as usize;
    let luma_len = pitch * height as usize;
    let chroma_len = pitch * (height as usize / 2);
    // The chroma plane follows the luma plane in a mapped NV12 texture. Check the mapping
    // really covers both before building slices over it.
    ensure!(
        pitch >= width as usize && mapped.DepthPitch as usize >= luma_len + chroma_len,
        "Unexpected NV12 layout (row pitch {}, depth pitch {})",
        mapped.RowPitch,
        mapped.DepthPitch
    );
    // SAFETY: checked above that `DepthPitch` bytes, which cover both planes, are mapped.
    let (luma, chroma) = unsafe {
        let base = mapped.pData as *const u8;
        (
            std::slice::from_raw_parts(base, luma_len),
            std::slice::from_raw_parts(base.add(luma_len), chroma_len),
        )
    };
    let mut buffer = I420Buffer::new(width, height);
    let (sy, su, sv) = buffer.strides();
    let (y, u, v) = buffer.data_mut();
    yuv_helper::nv12_to_i420(
        luma,
        mapped.RowPitch,
        chroma,
        mapped.RowPitch,
        y,
        sy,
        u,
        su,
        v,
        sv,
        width as i32,
        height as i32,
    );
    Ok(buffer)
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// The monitor for a WebRTC screen id, which is the screen's index among display devices.
fn monitor(id: u64) -> Result<HMONITOR> {
    let index = u32::try_from(id).map_err(|_| anyhow!("Unknown screen"))?;
    let mut device = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // SAFETY: `device` is sized for the call.
    let found = unsafe { EnumDisplayDevicesW(PCWSTR::null(), index, &mut device, 0) };
    ensure!(
        found.as_bool(),
        "The selected screen is no longer available"
    );
    let name = device.DeviceName;

    struct Search {
        name: [u16; 32],
        found: Option<HMONITOR>,
    }
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Search` passed below, alive for the enumeration.
        let search = unsafe { &mut *(data.0 as *mut Search) };
        let mut info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: MONITORINFOEXW starts with MONITORINFO and cbSize says which it is.
        let ok = unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) };
        if ok.as_bool() && info.szDevice == search.name {
            search.found = Some(monitor);
            return false.into();
        }
        true.into()
    }
    let mut search = Search { name, found: None };
    // SAFETY: the callback only touches `search`, which outlives the call.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(visit),
            LPARAM(&mut search as *mut Search as isize),
        );
    }
    search
        .found
        .ok_or_else(|| anyhow!("The selected screen is no longer available"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::DISPLAY_DEVICE_PRIMARY_DEVICE;

    #[test]
    fn fit_scales_down_to_even_sizes_and_never_up() {
        assert_eq!(fit(2560, 1440, (1920, 1080)), (1920, 1080));
        assert_eq!(fit(2560, 1369, (1920, 1080)), (1920, 1026));
        assert_eq!(fit(1280, 720, (1920, 1080)), (1280, 720));
        assert_eq!(fit(1279, 719, (1920, 1080)), (1278, 718));
        assert_eq!(fit(1, 1, (1920, 1080)), (2, 2));
    }

    #[test]
    fn crop_trims_odd_sources_that_fit_and_scales_larger_ones_whole() {
        // Fits: converted 1:1 apart from the odd edge pixel, so text stays sharp.
        assert_eq!(crop((1279, 719), (1920, 1080)), (1278, 718));
        assert_eq!(
            fit(1279, 719, (1920, 1080)),
            crop((1279, 719), (1920, 1080))
        );
        assert_eq!(crop((1280, 720), (1920, 1080)), (1280, 720));
        // Larger: the whole picture is scaled down.
        assert_eq!(crop((2560, 1440), (1920, 1080)), (2560, 1440));
        assert_eq!(crop((2561, 1000), (1920, 1080)), (2561, 1000));
    }

    /// Captures the primary screen for a few seconds and checks that frames come back without
    /// the capture ever blocking. Only new frames count: a still screen may give just one.
    #[test]
    #[ignore = "Captures the real screen with the GPU"]
    fn captures_the_screen_without_waiting() -> Result<()> {
        let mut capture = Capture::open(
            Target::Screen(primary()),
            (1920, 1080),
            Duration::from_millis(33),
        )?;
        let start = std::time::Instant::now();
        let mut frames = 0;
        let mut slowest = Duration::ZERO;
        let mut size = None;
        while start.elapsed() < Duration::from_secs(3) {
            let tick = std::time::Instant::now();
            if let Some(frame) = capture.poll()? {
                use libwebrtc::video_frame::VideoBuffer;
                size = Some((frame.width(), frame.height()));
                frames += 1;
            }
            slowest = slowest.max(tick.elapsed());
            std::thread::sleep(Duration::from_millis(33));
        }
        println!(
            "Fast capture: {frames} new frames of {size:?} in 3 s, slowest poll {slowest:?}, {} skipped",
            capture.skipped
        );
        ensure!(frames > 0, "No frames arrived");
        Ok(())
    }

    fn primary() -> u64 {
        (0..16)
            .find(|&index| {
                let mut device = DISPLAY_DEVICEW {
                    cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                    ..Default::default()
                };
                // SAFETY: `device` is sized for the call.
                unsafe {
                    EnumDisplayDevicesW(PCWSTR::null(), index, &mut device, 0).as_bool()
                        && device.StateFlags.contains(DISPLAY_DEVICE_PRIMARY_DEVICE)
                }
            })
            .map_or(0, u64::from)
    }
}
