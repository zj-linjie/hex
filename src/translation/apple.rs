#[cfg(target_os = "macos")]
use super::Accent;
use super::{SpeechRate, SystemTranslationStatus, VoiceInfo};

#[cfg(target_os = "macos")]
use serde::Deserialize;
#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString, c_char};
#[cfg(target_os = "macos")]
use std::ptr;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
const STATUS_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(target_os = "macos")]
const TRANSLATE_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn hex_apple_translation_status(timeout_ms: u64, error: *mut *mut c_char) -> i32;
    fn hex_apple_translation_translate(
        text: *const c_char,
        timeout_ms: u64,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn hex_apple_translation_voices() -> *mut c_char;
    fn hex_apple_translation_speak(
        text: *const c_char,
        voice_id: *const c_char,
        slow: bool,
        error: *mut *mut c_char,
    ) -> i32;
    fn hex_apple_translation_stop_speech();
    fn hex_apple_translation_is_speaking() -> bool;
    fn hex_apple_translation_free_string(string: *mut c_char);
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
struct BridgeVoice {
    id: String,
    name: String,
    accent: String,
}

pub fn system_status() -> SystemTranslationStatus {
    #[cfg(not(target_os = "macos"))]
    {
        return SystemTranslationStatus::Unsupported;
    }
    #[cfg(target_os = "macos")]
    {
        let mut error = ptr::null_mut();
        // SAFETY: A bounded status probe; the bridge writes only `error`.
        let code = unsafe { hex_apple_translation_status(timeout_ms(STATUS_TIMEOUT), &mut error) };
        let message = take_string(error);
        match code {
            0 => SystemTranslationStatus::Ready,
            1 => SystemTranslationStatus::NotInstalled,
            2 => SystemTranslationStatus::Unsupported,
            3 => SystemTranslationStatus::Unsupported,
            _ => SystemTranslationStatus::Failed {
                message: message.unwrap_or_else(|| "On-device translation failed".into()),
            },
        }
    }
}

pub fn translate(text: &str) -> Result<String, super::LookupError> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        return Err(super::LookupError::new(
            super::LookupErrorKind::SystemUnsupported,
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let source = cstring(text);
        let mut error = ptr::null_mut();
        // SAFETY: `source` outlives the call; the bridge writes only `error`.
        let result = unsafe {
            hex_apple_translation_translate(
                source.as_ptr(),
                timeout_ms(TRANSLATE_TIMEOUT),
                &mut error,
            )
        };
        if result.is_null() {
            let message = take_string(error).unwrap_or_default();
            return Err(classify_translate(&message));
        }
        let text = take_string(result).unwrap_or_default();
        if text.trim().is_empty() {
            return Err(super::LookupError::new(
                super::LookupErrorKind::SystemUnavailable,
            ));
        }
        Ok(text)
    }
}

pub fn voices() -> Vec<VoiceInfo> {
    #[cfg(not(target_os = "macos"))]
    {
        return Vec::new();
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: The bridge returns malloc JSON or an empty array.
        let json = take_string(unsafe { hex_apple_translation_voices() }).unwrap_or_default();
        let parsed: Vec<BridgeVoice> = serde_json::from_str(&json).unwrap_or_default();
        parsed
            .into_iter()
            .filter_map(|voice| {
                let accent = match voice.accent.as_str() {
                    "us" => Accent::Us,
                    "gb" => Accent::Gb,
                    _ => return None,
                };
                Some(VoiceInfo {
                    id: voice.id,
                    name: voice.name,
                    accent,
                })
            })
            .collect()
    }
}

pub fn speak(text: &str, voice_id: &str, rate: SpeechRate) -> Result<(), super::LookupError> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (text, voice_id, rate);
        return Err(super::LookupError::new(
            super::LookupErrorKind::SystemUnsupported,
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let text = cstring(text);
        let voice = cstring(voice_id);
        let mut error = ptr::null_mut();
        // SAFETY: Both strings outlive the call. Speech is dispatched to the main thread.
        let code = unsafe {
            hex_apple_translation_speak(
                text.as_ptr(),
                voice.as_ptr(),
                matches!(rate, SpeechRate::Slow),
                &mut error,
            )
        };
        if code != 0 {
            let _ = take_string(error);
            return Err(super::LookupError::new(
                super::LookupErrorKind::SystemUnavailable,
            ));
        }
        Ok(())
    }
}

pub fn stop_speech() {
    #[cfg(target_os = "macos")]
    // SAFETY: Stop is dispatched to the main thread inside the bridge.
    unsafe {
        hex_apple_translation_stop_speech();
    }
}

pub fn is_speaking() -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
    #[cfg(target_os = "macos")]
    // SAFETY: A flag read.
    unsafe {
        hex_apple_translation_is_speaking()
    }
}

#[cfg(target_os = "macos")]
fn classify_translate(message: &str) -> super::LookupError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") {
        super::LookupError::new(super::LookupErrorKind::TimedOut)
    } else if lower.contains("not installed") || lower.contains("aren't installed") {
        super::LookupError::new(super::LookupErrorKind::SystemNotReady)
    } else if lower.contains("macos 26") {
        super::LookupError::new(super::LookupErrorKind::SystemUnsupported)
    } else {
        super::LookupError::new(super::LookupErrorKind::SystemUnavailable)
    }
}

#[cfg(target_os = "macos")]
fn timeout_ms(timeout: Duration) -> u64 {
    timeout.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(target_os = "macos")]
fn cstring(value: &str) -> CString {
    CString::new(value.replace('\0', "")).unwrap_or_else(|_| CString::new("").unwrap())
}

#[cfg(target_os = "macos")]
fn take_string(string: *mut c_char) -> Option<String> {
    if string.is_null() {
        return None;
    }
    // SAFETY: The bridge returns NUL-terminated strings from `strdup`.
    let value = unsafe { CStr::from_ptr(string) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: Ownership returns to the bridge allocator exactly once.
    unsafe { hex_apple_translation_free_string(string) };
    Some(value)
}
