use super::*;

#[derive(Clone, Copy, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
pub enum AssistantCaptureAction {
    Start,
    Finish,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssistantCaptureStatus {
    Idle,
    Recording,
    Busy,
    Unavailable,
}

static ASSISTANT_COMMANDS: std::sync::Mutex<Option<SyncSender<AssistantCaptureAction>>> =
    std::sync::Mutex::new(None);
static ASSISTANT_STATUS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn assistant_capture(action: AssistantCaptureAction) {
    if let Some(sender) = ASSISTANT_COMMANDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = sender.try_send(action);
    }
}

pub fn assistant_capture_status() -> AssistantCaptureStatus {
    match ASSISTANT_STATUS.load(Ordering::Acquire) {
        1 => AssistantCaptureStatus::Recording,
        2 => AssistantCaptureStatus::Busy,
        3 => AssistantCaptureStatus::Unavailable,
        _ => AssistantCaptureStatus::Idle,
    }
}

pub(super) fn assistant_status(status: AssistantCaptureStatus) {
    ASSISTANT_STATUS.store(
        match status {
            AssistantCaptureStatus::Idle => 0,
            AssistantCaptureStatus::Recording => 1,
            AssistantCaptureStatus::Busy => 2,
            AssistantCaptureStatus::Unavailable => 3,
        },
        Ordering::Release,
    );
}

pub(super) struct AssistantRegistration;
impl AssistantRegistration {
    pub(super) fn new() -> (Self, Receiver<AssistantCaptureAction>) {
        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        *ASSISTANT_COMMANDS.lock().unwrap_or_else(|e| e.into_inner()) = Some(sender);
        assistant_status(AssistantCaptureStatus::Idle);
        (Self, receiver)
    }
}
impl Drop for AssistantRegistration {
    fn drop(&mut self) {
        *ASSISTANT_COMMANDS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        assistant_status(AssistantCaptureStatus::Unavailable);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn assistant_start(
    busy: bool,
    started: &mut Option<CaptureInstant>,
    input: &DictationAudio,
    indicator: &DictationSink<'_>,
    at: CaptureInstant,
    release_while_idle: bool,
) -> Result<()> {
    if busy {
        assistant_status(AssistantCaptureStatus::Busy);
        return Ok(());
    }
    if !crate::app_settings::voice_assistant_enabled() || !input.start_assistant(at)? {
        assistant_status(AssistantCaptureStatus::Unavailable);
        return Ok(());
    }
    feedback::assistant_start(release_while_idle);
    crate::voice_assistant::begin_turn();
    *started = Some(at);
    assistant_status(AssistantCaptureStatus::Recording);
    indicator.send(DictationIndicatorEvent::AssistantStarted);
    Ok(())
}

/// Cancel always interrupts the remote turn, even when microphone capture
/// ended already. Only cancel the shared audio owner when we own it.
pub(super) fn assistant_cancel(
    started: &mut Option<CaptureInstant>,
    pending: &mut Option<u64>,
    input: &DictationAudio,
    indicator: &DictationSink<'_>,
) -> Result<()> {
    cancel_with(
        started,
        pending,
        || input.cancel(),
        crate::voice_assistant::cancel,
        || {
            indicator.send(DictationIndicatorEvent::AssistantCancelled);
        },
    )?;
    assistant_status(AssistantCaptureStatus::Idle);
    Ok(())
}

fn cancel_with(
    started: &mut Option<CaptureInstant>,
    pending: &mut Option<u64>,
    cancel_input: impl FnOnce() -> Result<()>,
    cancel_runtime: impl FnOnce(),
    clear_hud: impl FnOnce(),
) -> Result<()> {
    let owns_input = started.take().is_some();
    if owns_input {
        feedback::retract_start();
    }
    let had_pending = pending.take().is_some();
    let input_result = if owns_input { cancel_input() } else { Ok(()) };
    cancel_runtime();
    if owns_input || had_pending {
        clear_hud();
    }
    input_result
}

pub(super) fn assistant_end(
    submit: bool,
    started: &mut Option<CaptureInstant>,
    pending: &mut Option<u64>,
    input: &DictationAudio,
    indicator: &DictationSink<'_>,
    at: CaptureInstant,
) -> Result<()> {
    let Some(start) = started.take() else {
        return Ok(());
    };
    let valid = submit
        && at.duration_since(start) >= crate::dictation::MINIMUM_HOLD_DURATION
        && at.duration_since(start) <= Duration::from_secs(60);
    if valid {
        input.become_intentional(at)?;
        match input.finish_assistant(at)? {
            Finish::Transcribe(clip) => {
                if let Some(samples) = clip.into_assistant_samples() {
                    match crate::voice_assistant::submit(samples) {
                        Ok(receipt) => {
                            *pending = Some(receipt);
                            indicator.send(DictationIndicatorEvent::AssistantWaiting);
                            assistant_status(AssistantCaptureStatus::Idle);
                        }
                        Err(_) => {
                            indicator.send(DictationIndicatorEvent::AssistantFailed);
                            assistant_status(AssistantCaptureStatus::Unavailable);
                        }
                    }
                    return Ok(());
                } else {
                    crate::voice_assistant::cancel();
                }
            }
            _ => crate::voice_assistant::cancel(),
        }
    } else {
        input.cancel()?;
        crate::voice_assistant::cancel();
    }
    feedback::retract_start();
    assistant_status(AssistantCaptureStatus::Idle);
    indicator.send(DictationIndicatorEvent::AssistantCancelled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn explicit_cancel_interrupts_waiting_or_speaking_without_touching_other_capture() {
        for _stage in ["waiting", "speaking"] {
            let mut started = None;
            let mut pending = Some(42);
            let input = Cell::new(false);
            let runtime = Cell::new(false);
            let hud = Cell::new(false);
            cancel_with(
                &mut started,
                &mut pending,
                || {
                    input.set(true);
                    Ok(())
                },
                || runtime.set(true),
                || hud.set(true),
            )
            .unwrap();
            assert!(!input.get());
            assert!(runtime.get() && hud.get());
            assert!(pending.is_none());
        }
    }

    #[test]
    fn explicit_cancel_of_recording_closes_only_its_capture_and_interrupts() {
        let mut started = Some(CaptureInstant::now());
        let mut pending = None;
        let input = Cell::new(false);
        let runtime = Cell::new(false);
        let hud = Cell::new(false);
        cancel_with(
            &mut started,
            &mut pending,
            || {
                input.set(true);
                Ok(())
            },
            || runtime.set(true),
            || hud.set(true),
        )
        .unwrap();
        assert!(input.get() && runtime.get() && hud.get());
        assert!(started.is_none());
    }
}
