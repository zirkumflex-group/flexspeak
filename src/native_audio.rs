use anyhow::{Result, anyhow, ensure};
use std::{ffi::c_void, ptr::NonNull};

unsafe extern "C" {
    fn flex_apm_create(echo: bool, gain: bool, noise: bool, level: i32) -> *mut c_void;
    fn flex_apm_destroy(processor: *mut c_void);
    fn flex_apm_capture(processor: *mut c_void, pcm: *mut i16, delay_ms: i32) -> i32;
    fn flex_apm_render(processor: *mut c_void, pcm: *mut i16) -> i32;
    fn flex_vad_create() -> *mut c_void;
    fn flex_vad_destroy(vad: *mut c_void);
    fn flex_vad_process(vad: *mut c_void, pcm: *const i16) -> i32;
}

pub struct Apm(NonNull<c_void>);
impl Apm {
    pub fn new(settings: crate::audio_processing::Settings) -> Result<Self> {
        // The pointer owns one native processor, used only by the voice worker.
        let pointer = unsafe {
            flex_apm_create(
                settings.echo_cancellation,
                settings.automatic_gain,
                settings.noise_suppression,
                settings.suppression_level as i32,
            )
        };
        Ok(Self(NonNull::new(pointer).ok_or_else(|| {
            anyhow!("Cannot initialize microphone processing")
        })?))
    }
    pub fn capture(&mut self, pcm: &mut [i16; 480], delay: Option<i32>) -> Result<()> {
        ensure!(
            delay.is_none_or(|value| (0..=500).contains(&value)),
            "Invalid echo delay"
        );
        // Fixed-size slices match the native 48 kHz mono capture contract.
        ensure!(
            unsafe { flex_apm_capture(self.0.as_ptr(), pcm.as_mut_ptr(), delay.unwrap_or(-1)) }
                == 0,
            "Microphone processing failed"
        );
        Ok(())
    }
    pub fn render(&mut self, pcm: &mut [i16; 960]) -> Result<()> {
        // Fixed-size slices match the native 48 kHz stereo reference contract.
        ensure!(
            unsafe { flex_apm_render(self.0.as_ptr(), pcm.as_mut_ptr()) } == 0,
            "Echo reference processing failed"
        );
        Ok(())
    }
}
impl Drop for Apm {
    fn drop(&mut self) {
        unsafe { flex_apm_destroy(self.0.as_ptr()) };
    }
}

pub struct Vad(NonNull<c_void>);
impl Vad {
    pub fn new() -> Result<Self> {
        Ok(Self(
            NonNull::new(unsafe { flex_vad_create() })
                .ok_or_else(|| anyhow!("Cannot initialize speech detection"))?,
        ))
    }
    pub fn speech(&mut self, pcm: &[i16; 960]) -> Result<bool> {
        // The detector accepts one 20 ms mono frame at 48 kHz.
        match unsafe { flex_vad_process(self.0.as_ptr(), pcm.as_ptr()) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(anyhow!("Speech detection failed")),
        }
    }
}
impl Drop for Vad {
    fn drop(&mut self) {
        unsafe { flex_vad_destroy(self.0.as_ptr()) };
    }
}
