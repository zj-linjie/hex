//! Xiaozhi assistant: explicit opt-in, one bounded in-memory conversation.
use std::path::Path;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Disabled,
    NeedsIdentity,
    Connecting,
    Ready,
    Listening,
    Waiting,
    Speaking,
    Cancelled,
    Error(String),
}
#[derive(Clone, Debug, Default)]
pub struct Turn {
    pub transcript: String,
    pub reply: String,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub enabled: bool,
    pub playback: bool,
    pub status: Status,
    pub turns: Vec<Turn>,
    pub turn_id: Option<u64>,
    pub audio_error: Option<String>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            playback: true,
            status: Status::Disabled,
            turns: Vec::new(),
            turn_id: None,
            audio_error: None,
        }
    }
}
static STATE: OnceLock<Mutex<Snapshot>> = OnceLock::new();
fn state() -> &'static Mutex<Snapshot> {
    STATE.get_or_init(|| Mutex::new(Snapshot::default()))
}
fn update(f: impl FnOnce(&mut Snapshot)) {
    if let Ok(mut s) = state().lock() {
        f(&mut s);
    }
}
pub fn snapshot() -> Snapshot {
    state().lock().map(|s| s.clone()).unwrap_or_default()
}

#[cfg(target_os = "macos")]
mod runtime;

pub fn set_enabled(enabled: bool) {
    let mut changed = false;
    update(|s| {
        if s.enabled != enabled {
            changed = true;
            s.enabled = enabled;
            s.status = if enabled {
                Status::Connecting
            } else {
                Status::Disabled
            };
        }
    });
    #[cfg(target_os = "macos")]
    if changed {
        runtime::interrupt(runtime::Command::Enable(enabled));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = changed;
}
pub fn set_playback(playback: bool) {
    let mut changed = false;
    update(|s| {
        if s.playback != playback {
            changed = true;
            s.playback = playback;
        }
    });
    #[cfg(target_os = "macos")]
    if changed {
        runtime::playback_changed(playback);
        if snapshot().enabled {
            runtime::signal(runtime::Command::Playback(playback));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = changed;
}
pub fn import_identity(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        runtime::import_identity(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("Voice assistant is only available on macOS".into())
    }
}
pub fn reconnect() {
    #[cfg(target_os = "macos")]
    runtime::interrupt(runtime::Command::Reconnect);
}
pub fn begin_turn() {
    #[cfg(target_os = "macos")]
    runtime::interrupt(runtime::Command::Begin);
}
pub fn submit(samples: Vec<f32>) -> Result<u64, String> {
    #[cfg(target_os = "macos")]
    {
        runtime::submit(samples)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = samples;
        Err("Voice assistant is only available on macOS".into())
    }
}
pub fn cancel() {
    #[cfg(target_os = "macos")]
    runtime::interrupt(runtime::Command::Cancel);
}
