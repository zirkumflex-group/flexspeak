//! Remembered server passwords. On Windows they are sealed with DPAPI for the signed-in user
//! before they reach the settings file, so the file alone doesn't give them away. Other systems
//! have no such store here, so nothing is remembered there and the option is hidden.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Whether passwords can be remembered on this system.
pub const AVAILABLE: bool = cfg!(windows);

/// Seals `password` for the settings file, or `None` when this system can't.
pub fn protect(password: &str) -> Option<String> {
    seal(password.as_bytes()).map(|sealed| STANDARD.encode(sealed))
}

/// Opens what `protect` sealed, or `None` when it can't be read here (another user or machine).
pub fn unprotect(sealed: &str) -> Option<String> {
    let bytes = STANDARD.decode(sealed.trim()).ok()?;
    String::from_utf8(open(&bytes)?).ok()
}

#[cfg(windows)]
fn seal(data: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).ok()?,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points at `data`, which outlives the call; `output` is freed by `take`.
    unsafe {
        CryptProtectData(
            &input,
            windows::core::w!("Flexspeak server password"),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .ok()?;
        Some(take(output))
    }
}

#[cfg(windows)]
fn open(data: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).ok()?,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `seal`.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .ok()?;
        Some(take(output))
    }
}

/// Copies a blob DPAPI allocated and frees it.
///
/// # Safety
/// `blob` must come from a successful `CryptProtectData` or `CryptUnprotectData`.
#[cfg(windows)]
unsafe fn take(blob: windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) -> Vec<u8> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    if blob.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc.
    unsafe {
        let bytes = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(blob.pbData.cast())));
        bytes
    }
}

#[cfg(not(windows))]
fn seal(_: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
fn open(_: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remembered_password_reads_back_and_is_not_kept_in_the_clear() {
        let sealed = protect("hunter2 – ünïcode");
        assert_eq!(sealed.is_some(), AVAILABLE, "Windows seals passwords");
        let Some(sealed) = sealed else {
            return;
        };
        assert!(!sealed.contains("hunter2"));
        assert_eq!(unprotect(&sealed).as_deref(), Some("hunter2 – ünïcode"));
        assert_eq!(unprotect("not base64!"), None);
        assert_eq!(unprotect(&STANDARD.encode(b"not a DPAPI blob")), None);
    }
}
