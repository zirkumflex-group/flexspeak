//! System audio without this app's own sound, for sharing (Windows only).
//!
//! A plain loopback of an output records everything played on it, voices of the people in
//! the channel and the app's sounds included, so viewers heard the call twice. Windows' process
//! loopback (Windows 10 build 20348 and later) records what every *other* process plays
//! instead. Where it is unavailable the caller falls back to the plain loopback.

use anyhow::{Context, Result, ensure};
use crossbeam_queue::ArrayQueue;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
            AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
            IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
            IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
            PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVE_FORMAT_PCM, WAVEFORMATEX,
        },
        System::{
            Com::{
                BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                StructuredStorage::PROPVARIANT,
            },
            Threading::{CreateEventW, WaitForSingleObject},
            Variant::VT_BLOB,
        },
    },
    core::{HRESULT, IUnknown, Interface, PCWSTR, Ref, implement},
};

/// The format asked of Windows, which converts to it: what the share sends.
pub const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
/// How long the activation may take; it is normally a few milliseconds.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Records every other process's sound while it is held.
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Capture {
    /// Starts recording into `queue` at [`RATE`] in stereo; errors after the start go to
    /// `errors`. While `active` is false the sound is read and thrown away.
    pub fn open(
        queue: Arc<ArrayQueue<[f32; 2]>>,
        errors: Arc<ArrayQueue<String>>,
        active: Arc<AtomicBool>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (started, ready) = std::sync::mpsc::sync_channel(1);
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("share-audio".into())
            .spawn(move || {
                // SAFETY: paired with `CoUninitialize` below, on this thread.
                let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                match Stream::open() {
                    Ok(stream) => {
                        let _ = started.send(Ok(()));
                        if let Err(error) = stream.run(&queue, &stopping, &active) {
                            let _ = errors.force_push(format!("{error:#}"));
                        }
                    }
                    Err(error) => {
                        let _ = started.send(Err(error));
                    }
                }
                if com {
                    // SAFETY: this thread initialised COM above.
                    unsafe { CoUninitialize() };
                }
            })?;
        let mut capture = Self {
            stop,
            thread: Some(thread),
        };
        match ready.recv_timeout(ACTIVATION_TIMEOUT * 2) {
            Ok(Ok(())) => Ok(capture),
            Ok(Err(error)) => Err(error),
            Err(_) => {
                // Leave the thread to finish by itself rather than wait on a stuck activation.
                capture.thread = None;
                anyhow::bail!("Starting the system audio capture timed out")
            }
        }
    }
}

/// Wakes the thread waiting for an activation.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(Arc<(Mutex<bool>, Condvar)>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let (done, wake) = &*self.0;
        *done.lock().unwrap() = true;
        wake.notify_all();
        Ok(())
    }
}

struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the client was started by `open`, and the event is ours to close.
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

impl Stream {
    fn open() -> Result<Self> {
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: std::process::id(),
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // Never dropped: the `windows` crate drops a PROPVARIANT with PropVariantClear, which
        // would free the blob, here a pointer to `params` on the stack.
        let mut variant = std::mem::ManuallyDrop::new(PROPVARIANT::default());
        // SAFETY: a zeroed PROPVARIANT is VT_EMPTY; this makes it a VT_BLOB that points at
        // `params`, which outlives the activation below.
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VT_BLOB;
            inner.Anonymous.blob = BLOB {
                cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                pBlobData: (&raw const params).cast_mut().cast(),
            };
        }
        let state = Arc::new((Mutex::new(false), Condvar::new()));
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(state.clone()).into();
        // SAFETY: plain COM calls with valid arguments; `variant` and `params` live until the
        // activation finished below.
        let client: IAudioClient = unsafe {
            let operation = ActivateAudioInterfaceAsync(
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                &IAudioClient::IID,
                Some(&raw const *variant),
                &handler,
            )
            .context("Process audio capture is unavailable on this version of Windows")?;
            let (done, wake) = &*state;
            let (finished, _) = wake
                .wait_timeout_while(done.lock().unwrap(), ACTIVATION_TIMEOUT, |done| !*done)
                .unwrap();
            ensure!(*finished, "Starting the system audio capture timed out");
            drop(finished);
            let mut result = HRESULT(0);
            let mut activated: Option<IUnknown> = None;
            operation.GetActivateResult(&mut result, &mut activated)?;
            result
                .ok()
                .context("Windows refused the system audio capture")?;
            activated
                .context("Windows returned no system audio capture")?
                .cast()?
        };
        let block = CHANNELS * 2;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: CHANNELS,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * u32::from(block),
            nBlockAlign: block,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // SAFETY: plain COM calls with valid arguments; the event is closed by `Drop`.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                // 100 ms of buffer, in 100 ns units.
                1_000_000,
                0,
                &format,
                None,
            )?;
            let capture: IAudioCaptureClient = client.GetService()?;
            let event = CreateEventW(None, false, false, PCWSTR::null())?;
            let stream = Self {
                client,
                capture,
                event,
            };
            stream.client.SetEventHandle(stream.event)?;
            stream.client.Start()?;
            Ok(stream)
        }
    }

    fn run(
        &self,
        queue: &ArrayQueue<[f32; 2]>,
        stop: &AtomicBool,
        active: &AtomicBool,
    ) -> Result<()> {
        while !stop.load(Ordering::Acquire) {
            // SAFETY: the event belongs to this stream. A timeout only rechecks `stop`.
            if unsafe { WaitForSingleObject(self.event, 100) } != WAIT_OBJECT_0 {
                continue;
            }
            loop {
                // SAFETY: plain COM calls; the buffer is read only between GetBuffer and
                // ReleaseBuffer, for the frames Windows says it holds.
                unsafe {
                    if self.capture.GetNextPacketSize()? == 0 {
                        break;
                    }
                    let mut data = std::ptr::null_mut();
                    let mut frames = 0;
                    let mut flags = 0;
                    self.capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    if active.load(Ordering::Acquire) {
                        let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                        if silent || data.is_null() {
                            for _ in 0..frames {
                                let _ = queue.force_push([0.0; 2]);
                            }
                        } else {
                            let samples = std::slice::from_raw_parts(
                                data.cast::<i16>(),
                                frames as usize * usize::from(CHANNELS),
                            );
                            for frame in samples.as_chunks::<2>().0 {
                                // Prefer recent sound to a growing backlog when encoding stalls.
                                let _ = queue.force_push(frame.map(|s| f32::from(s) / 32768.0));
                            }
                        }
                    }
                    self.capture.ReleaseBuffer(frames)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Native audio: needs Windows 10 build 20348 or later and an audio service"]
    fn opens_the_capture_of_every_other_app_and_stops_cleanly() {
        let queue = Arc::new(ArrayQueue::new(RATE as usize));
        let errors = Arc::new(ArrayQueue::new(4));
        let capture = Capture::open(queue, errors.clone(), Arc::new(AtomicBool::new(true)))
            .expect("process loopback opens");
        std::thread::sleep(Duration::from_millis(500));
        drop(capture);
        assert!(errors.pop().is_none(), "the capture ran without errors");
    }

    /// The share of `samples`' energy at `frequency` (Goertzel), 0 to 1.
    fn tone_share(samples: &[f32], frequency: f32) -> f32 {
        let omega = 2.0 * std::f32::consts::PI * frequency / RATE as f32;
        let coefficient = 2.0 * omega.cos();
        let (mut previous, mut before) = (0.0_f32, 0.0_f32);
        for &sample in samples {
            let next = sample + coefficient * previous - before;
            before = previous;
            previous = next;
        }
        let power = previous * previous + before * before - coefficient * previous * before;
        let total: f32 = samples.iter().map(|s| s * s).sum();
        if total <= 1e-9 {
            0.0
        } else {
            (2.0 * power / (samples.len() as f32 * total)).min(1.0)
        }
    }

    fn record(queue: &ArrayQueue<[f32; 2]>, during: impl FnOnce()) -> Vec<f32> {
        while queue.pop().is_some() {}
        during();
        std::iter::from_fn(|| queue.pop())
            .map(|[left, _]| left)
            .collect()
    }

    /// Plays a quiet 1 kHz tone (2% of full scale) on the default output for 0.7 s.
    fn play_tone() {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let device = cpal::default_host().default_output_device().unwrap();
        let config = device.default_output_config().unwrap();
        assert_eq!(config.sample_format(), cpal::SampleFormat::F32);
        let (rate, channels) = (config.sample_rate() as f32, usize::from(config.channels()));
        let mut phase = 0.0_f32;
        let tone = device
            .build_output_stream(
                config.config(),
                move |data: &mut [f32], _| {
                    for frame in data.chunks_mut(channels) {
                        phase = (phase + 1000.0 / rate).fract();
                        frame.fill(0.02 * (phase * std::f32::consts::TAU).sin());
                    }
                },
                |_| {},
                None,
            )
            .unwrap();
        tone.play().unwrap();
        std::thread::sleep(Duration::from_millis(700));
        tone.pause().unwrap();
    }

    /// Where the test below asks its other app to play.
    fn tone_marker() -> std::path::PathBuf {
        std::env::temp_dir().join("flexspeak-process-audio-tone")
    }

    /// The other app of the test below: this test binary run again, playing the tone.
    #[test]
    #[ignore = "Run by leaves_this_process_out_and_records_other_apps"]
    fn tone_from_another_process() {
        if tone_marker().exists() {
            play_tone();
        }
    }

    /// Plays the same 1 kHz tone from this process and from another one: only the other one
    /// is recorded. The tone is quiet, but audible: run it with the speakers turned down.
    #[test]
    #[ignore = "Native audio: plays sound on the default output"]
    fn leaves_this_process_out_and_records_other_apps() {
        let queue = Arc::new(ArrayQueue::new(RATE as usize * 4));
        let errors = Arc::new(ArrayQueue::new(4));
        let _capture = Capture::open(
            queue.clone(),
            errors.clone(),
            Arc::new(AtomicBool::new(true)),
        )
        .unwrap();
        let own = record(&queue, play_tone);
        // Started by WMI, so it is outside this process's tree, which the capture leaves out
        // as a whole (a child of this test was not recorded either).
        let other = record(&queue, || {
            std::fs::write(tone_marker(), b"").unwrap();
            let command = format!(
                "\"{}\" --ignored --exact streaming::process_audio::tests::tone_from_another_process",
                std::env::current_exe().unwrap().display()
            );
            let started = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine = '{}'}} | Out-Null",
                        command.replace('\'', "''")
                    ),
                ])
                .status()
                .unwrap();
            assert!(started.success());
            std::thread::sleep(Duration::from_millis(2500));
            let _ = std::fs::remove_file(tone_marker());
        });
        let (own_share, other_share) = (tone_share(&own, 1000.0), tone_share(&other, 1000.0));
        println!(
            "1 kHz share of what was recorded: this process {own_share:.3} ({} samples), other process {other_share:.3} ({} samples)",
            own.len(),
            other.len()
        );
        assert!(errors.pop().is_none());
        assert!(own_share < 0.02, "this process's tone was recorded");
        // Other sound playing meanwhile dilutes the share; this process's own reads as 0.000.
        assert!(other_share > 0.1, "another process's tone was not recorded");
    }
}
