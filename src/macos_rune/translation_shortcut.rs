//! Translation's keyboard entry while dictation waits for setup (no model or
//! microphone required). The normal listener owns the shortcut once running.
use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::app_settings::{AppSettings, RuntimeHotkey};

type Event = *mut c_void;
static TAP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static TAKEN: AtomicU32 = AtomicU32::new(0);
static ACCEPTING: AtomicBool = AtomicBool::new(false);

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        location: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Event, u32, Event, Event) -> Event,
        info: Event,
    ) -> Event;
    fn CGEventTapEnable(tap: Event, enabled: bool);
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventGetFlags(event: Event) -> u64;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: Event, order: isize) -> Event;
    fn CFRunLoopGetCurrent() -> Event;
    fn CFRunLoopAddSource(run_loop: Event, source: Event, mode: *const c_void);
    fn CFRunLoopRemoveSource(run_loop: Event, source: Event, mode: *const c_void);
    fn CFRelease(value: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

struct Tap {
    port: Event,
    source: Event,
    run_loop: Event,
}
impl Tap {
    fn attach() -> Option<Self> {
        // SAFETY: the callback has static lifetime, and all setup and teardown
        // happen on AppKit's main run loop. No audio device is opened.
        unsafe {
            let port = CGEventTapCreate(2, 0, 0, (1 << 10) | (1 << 11), callback, ptr::null_mut());
            if port.is_null() {
                return None;
            }
            let source = CFMachPortCreateRunLoopSource(ptr::null(), port, 0);
            if source.is_null() {
                CFRelease(port);
                return None;
            }
            let run_loop = CFRunLoopGetCurrent();
            TAP.store(port, Ordering::Release);
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(port, true);
            Some(Self {
                port,
                source,
                run_loop,
            })
        }
    }
}
impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: this main-thread owner holds both retained native objects.
        unsafe {
            CGEventTapEnable(self.port, false);
            CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopCommonModes);
            CFRelease(self.source);
            CFRelease(self.port);
        }
        TAP.store(ptr::null_mut(), Ordering::Release);
        TAKEN.store(0, Ordering::Release);
    }
}

#[derive(Default)]
struct Slot {
    binding: Option<RuntimeHotkey>,
    tap: Option<Tap>,
    attempted: Option<Instant>,
}
thread_local! { static SLOT: RefCell<Slot> = RefCell::new(Slot::default()); }

fn binding(settings: &AppSettings, listener_running: bool) -> Option<RuntimeHotkey> {
    (!listener_running)
        .then(|| {
            settings
                .active_translation_hotkey()
                .map(|key| key.runtime())
        })
        .flatten()
}

/// Called by the main pump even while the main settings window is hidden.
pub(super) fn sync(settings: &AppSettings, listener_running: bool) {
    let wanted = binding(settings, listener_running);
    ACCEPTING.store(wanted.is_some(), Ordering::Release);
    SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let changed = slot.binding != wanted;
        if changed && !can_handoff(&TAKEN) {
            // Keep draining the physical key we took, even if setup finishes
            // or the person changes bindings while holding it.
            return;
        }
        if changed {
            slot.tap = None;
            slot.binding = wanted;
            slot.attempted = None;
        }
        if wanted.is_some()
            && slot.tap.is_none()
            && slot
                .attempted
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
        {
            slot.attempted = Some(Instant::now());
            slot.tap = Tap::attach();
            if changed && slot.tap.is_none() {
                tracing::warn!(
                    "translation shortcut needs keyboard access; check Accessibility permission"
                );
            }
        }
    });
}

unsafe extern "C" fn callback(_: Event, kind: u32, event: Event, _: Event) -> Event {
    if kind == u32::MAX || kind == u32::MAX - 1 {
        let tap = TAP.load(Ordering::Acquire);
        if !tap.is_null() {
            // SAFETY: the main-loop owner keeps the tap alive during callbacks.
            unsafe {
                CGEventTapEnable(tap, true);
            }
        }
        return event;
    }
    if event.is_null() || !matches!(kind, 10 | 11) {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid keyboard event.
    let (code, repeat, flags, source) = unsafe {
        (
            CGEventGetIntegerValueField(event, 9) as u16,
            CGEventGetIntegerValueField(event, 8) != 0,
            CGEventGetFlags(event),
            CGEventGetIntegerValueField(event, 42),
        )
    };
    let matches = ACCEPTING.load(Ordering::Acquire)
        && !crate::app_settings::hotkey_capture_active()
        && crate::app_settings::runtime_hotkeys()
            .translation
            .is_some_and(|key| key.matches_key_press(code, flags));
    match take_key(
        &TAKEN,
        code,
        kind == 10,
        repeat,
        source == crate::keyboard::SYNTHETIC_EVENT_MARKER,
        matches,
    ) {
        Take::Pass => event,
        Take::Swallow => ptr::null_mut(),
        Take::Open => {
            crate::rune_ui::translation::open_popup();
            ptr::null_mut()
        }
    }
}

fn can_handoff(taken: &AtomicU32) -> bool {
    taken.load(Ordering::Acquire) == 0
}

#[derive(Debug, Eq, PartialEq)]
enum Take {
    Pass,
    Swallow,
    Open,
}

fn take_key(
    taken: &AtomicU32,
    code: u16,
    down: bool,
    repeat: bool,
    synthetic: bool,
    matches: bool,
) -> Take {
    if synthetic {
        return Take::Pass;
    }
    let held = taken.load(Ordering::Acquire);
    if held == u32::from(code) + 1 {
        if !down {
            taken.store(0, Ordering::Release);
        }
        return Take::Swallow;
    }
    if !down || held != 0 || repeat || !matches {
        return Take::Pass;
    }
    taken.store(u32::from(code) + 1, Ordering::Release);
    Take::Open
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_key_drains_before_setup_or_binding_handoff() {
        let taken = AtomicU32::new(0);
        assert_eq!(take_key(&taken, 17, true, false, false, true), Take::Open);
        assert!(!can_handoff(&taken));
        // No longer accepting shortcuts, but neither repeats nor the physical
        // release may leak after their key-down was consumed.
        assert_eq!(
            take_key(&taken, 17, true, true, false, false),
            Take::Swallow
        );
        assert_eq!(take_key(&taken, 18, true, false, false, true), Take::Pass);
        assert_eq!(
            take_key(&taken, 17, false, false, false, false),
            Take::Swallow
        );
        assert!(can_handoff(&taken));
    }

    #[test]
    fn synthetic_release_does_not_consume_the_physical_pair() {
        let taken = AtomicU32::new(0);
        assert_eq!(take_key(&taken, 17, true, false, false, true), Take::Open);
        assert_eq!(take_key(&taken, 17, false, false, true, false), Take::Pass);
        assert!(!can_handoff(&taken));
        assert_eq!(
            take_key(&taken, 17, false, false, false, false),
            Take::Swallow
        );
        assert!(can_handoff(&taken));
    }

    #[test]
    fn keyboard_entry_does_not_depend_on_dictation_setup() {
        let mut settings = AppSettings::default();
        assert!(binding(&settings, false).is_none());
        settings.labs.translation = true;
        assert!(binding(&settings, false).is_some());
        assert!(binding(&settings, true).is_none());
        settings.translation.hotkey = settings.dictation_hotkey.clone();
        assert!(binding(&settings, false).is_none());
    }
}
