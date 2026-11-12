//! The listen loop's command and outcome handlers: hotkey actions,
//! programmatic captures' losses, control commands, and the event emitters.
//! Split from [`super`] to keep the listener readable.

use super::*;

pub(super) fn log_programmatic_ended(active: Option<ProgrammaticDictation>, outcome: &str) {
    if let Some(active) = active {
        tracing::warn!(
            dictation_id = active.id,
            source = active.source,
            outcome,
            "programmatic dictation ended"
        );
    }
}

/// Ends a programmatic capture the microphone cut short. The audio delivered
/// before the loss waits for the owner's `/finish`; without any, `/finish`
/// answers `microphone-lost`.
pub(super) fn programmatic_microphone_lost(
    active: Option<ProgrammaticDictation>,
    kept: Option<DictationClip>,
    outcome: &str,
    lost: &mut Option<LostProgrammaticDictation>,
    completions: &mut VecDeque<Completion>,
) {
    let Some(active) = active else {
        return;
    };
    let Some(mut clip) = kept else {
        remember_completion(
            completions,
            Completion {
                id: active.id,
                owner_token: active.owner_token.clone(),
                outcome: TerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
        log_programmatic_ended(Some(active), outcome);
        return;
    };
    // A held clip must not defer background work until the owner finishes.
    drop(clip.take_capture_marker());
    tracing::warn!(
        dictation_id = active.id,
        source = active.source,
        outcome,
        duration_ms = clip.duration_ms(),
        "programmatic dictation kept its audio after a microphone loss"
    );
    let previous = lost.replace(LostProgrammaticDictation {
        id: active.id,
        owner_token: active.owner_token,
        source: active.source,
        lease_deadline: active.lease_deadline,
        clip,
    });
    if let Some(previous) = previous {
        tracing::warn!(
            dictation_id = previous.id,
            source = previous.source,
            outcome = "superseded",
            "programmatic dictation ended"
        );
        remember_completion(
            completions,
            Completion {
                id: previous.id,
                owner_token: previous.owner_token,
                outcome: TerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
    }
}

pub(super) fn cancel_programmatic_capture(
    active: ProgrammaticDictation,
    input: &DictationAudio,
    completions: &mut VecDeque<Completion>,
) -> Result<()> {
    input.cancel()?;
    remember_completion(
        completions,
        Completion {
            id: active.id,
            owner_token: active.owner_token,
            outcome: TerminalOutcome::Cancelled,
        },
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_hotkey_action(
    action: HotkeyAction,
    raw: bool,
    print: Option<PrintReply>,
    action_at: CaptureInstant,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    context: &mut ContextWatch,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    release_while_idle: bool,
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            if !dictation.start(action_at)? {
                return Ok(false);
            }
            // Labs › Return to app: this paste goes back to the app in front
            // now, even if another one comes forward while the person talks.
            worker.remember_return_to_app();
            // The press started a capture, but it may still be provisional:
            // no page is read until the capture is held past its threshold.
            context.begin();
            // The tone confirms the press at once; a chord or short tap
            // retracts it. A released microphone confirms only once it opens.
            if release_while_idle {
                feedback::prepare();
            } else {
                feedback::play_provisional_start();
            }
            worker.prepare_paste();
            #[cfg(hex_capture)]
            crate::screenshots::narration::started();
            events.dictation(DictationPhase::Started, "")?;
            indicator.send(DictationIndicatorEvent::Started);
            emit_state(events, true, device)
        }
        HotkeyAction::Finish => {
            // The capture's own page, or none rather than the last
            // dictation's.
            let context = context.finish(events)?;
            finish_hotkey_dictation(
                action_at,
                dictation,
                worker,
                &context,
                device,
                events,
                indicator,
                release_while_idle,
                print,
                raw,
            )
            .map(drop)
        }
        HotkeyAction::Discard(_) | HotkeyAction::Cancel | HotkeyAction::CancelShortcut => {
            context.cancel();
            feedback::retract_start();
            #[cfg(hex_capture)]
            crate::screenshots::narration::ended();
            if action == HotkeyAction::CancelShortcut {
                feedback::play(Tone::Cancel);
            }
            end_capture(
                if matches!(action, HotkeyAction::Cancel | HotkeyAction::CancelShortcut) {
                    CaptureEnd::Cancelled
                } else {
                    CaptureEnd::Discarded(action.discard_cause())
                },
                dictation,
                device,
                events,
                indicator,
            )
        }
        HotkeyAction::PasteLast | HotkeyAction::PasteMeeting => {
            // The paste chord's modifier press started a provisional capture.
            context.cancel();
            feedback::retract_start();
            dictation.cancel()?;
            let paste_result = match action {
                HotkeyAction::PasteLast => worker.paste_last(),
                HotkeyAction::PasteMeeting => worker.paste_meeting(),
                _ => unreachable!(),
            };
            if let Err(error) = paste_result {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, device)
        }
        HotkeyAction::Translation => {
            let capturing = dictation.is_recording();
            let cancel = translation_cancels_capture(capturing, dictation.is_intentional());
            if cancel {
                // Provisional shortcut capture from the chord's modifier press.
                feedback::retract_start();
                dictation.cancel()?;
            }
            crate::rune_ui::translation::open_popup();
            emit_engine_state(events, capturing && !cancel, worker, device)
        }
    }?;
    Ok(true)
}

/// The translation shortcut cancels only a provisional capture. An
/// intentional dictation keeps recording while the popup opens.
pub(super) fn translation_cancels_capture(recording: bool, intentional: bool) -> bool {
    recording && !intentional
}

/// Ends a shortcut dictation at `action_at` and hands it to transcription.
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_hotkey_dictation(
    action_at: CaptureInstant,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    release_while_idle: bool,
    print: Option<PrintReply>,
    raw: bool,
) -> Result<FinishResult> {
    if dictation.become_intentional(action_at)? && release_while_idle {
        feedback::play(Tone::DictationStart);
    }
    // A dictation over a screenshot lands its words there, never pasted; a
    // printing capture stops that shot's editor waiting instead.
    #[cfg(hex_capture)]
    let print = print
        .inspect(|_| crate::screenshots::narration::ended())
        .or_else(crate::screenshots::narration::claim);
    // A discarded or silent clip never reaches transcription, so its waiter
    // still hears from it.
    let unanswered = print.clone();
    let finished = match print {
        None => finish_dictation(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            worker,
            if raw {
                TranscriptionTarget::RawPaste
            } else {
                TranscriptionTarget::Paste
            },
            context,
            device,
            events,
            indicator,
        )?,
        Some(print) => finish_dictation_with(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            // Narration lands on its screenshot either way, never pasted.
            |clip, _| worker.transcribe_printed(clip, print, raw, context.clone()),
            device,
            events,
            indicator,
        )?,
    };
    if matches!(finished, FinishResult::Discarded) {
        feedback::retract_start();
    }
    // A discarded or silent clip submitted no job: say so instead of leaving
    // the printing command to its cap.
    if matches!(finished, FinishResult::Discarded | FinishResult::Silent)
        && let Some(print) = unanswered
    {
        print.send(crate::control::PrintedTranscript::NoSpeech);
    }
    emit_engine_state(events, false, worker, device)?;
    Ok(finished)
}

/// A control start has no press to judge, so its capture counts as
/// intentional from its first sample: the recording environment starts and
/// the live stream opens now. `tone_owed` plays the start tone for a capture
/// that's already open; one still opening the microphone plays it on
/// `ReadyIntentional`.
pub(super) fn make_control_capture_intentional(
    input: &DictationAudio,
    tone_owed: bool,
) -> Result<()> {
    if input.mark_intentional()? && tone_owed {
        feedback::play(Tone::DictationStart);
    }
    Ok(())
}

/// The reply to a control `stop`: only a submitted clip transcribes.
pub(super) fn finish_reply(finished: Option<FinishResult>, submitted: &str) -> ControlReply {
    match finished {
        Some(FinishResult::Submitted) => ControlReply::done(submitted),
        Some(FinishResult::Discarded) | None => ControlReply::error(
            "discarded",
            "too short, discarded: no audio was recorded yet (the microphone may still have \
             been opening)",
        ),
        Some(FinishResult::Silent) => ControlReply::error(
            "discarded",
            "the recording was silent, discarded; nothing to transcribe",
        ),
        Some(FinishResult::Rejected(error)) => {
            ControlReply::error("failed", format!("couldn't transcribe it: {error}"))
        }
    }
}

/// `hex dictation`: a start locks the shortcut machine as a double tap
/// does, so the next press or `stop` finishes it. A printing start keeps the
/// command's reply here until that recording ends, however it ends.
#[allow(clippy::too_many_arguments)]
pub(super) fn control_dictation(
    action: CaptureAction,
    print: Option<PrintReply>,
    raw: bool,
    other_recording: bool,
    suspended: bool,
    hotkeys: &mut Hotkeys,
    input: &DictationAudio,
    worker: &DictationWorker,
    context: &mut ContextWatch,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    release_while_idle: bool,
) -> Result<ControlReply> {
    let recording = hotkeys.any_recording();
    let device = input.device_name();
    let start = match action {
        CaptureAction::Start => true,
        CaptureAction::Stop => false,
        CaptureAction::Toggle => !recording,
        CaptureAction::Cancel if recording => {
            hotkeys.cancel_print();
            hotkeys.suspend_recording();
            handle_hotkey_action(
                HotkeyAction::CancelShortcut,
                false,
                None,
                input.captured_through(),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
            )?;
            return Ok(ControlReply::done("Cancelled the recording"));
        }
        CaptureAction::Cancel => return Ok(ControlReply::unchanged("Dictation isn't recording")),
    };
    Ok(match start {
        true if recording && print.is_some() => ControlReply::error(
            "busy",
            "a dictation is already recording; finish it before starting a print-only one",
        ),
        true if recording => ControlReply::unchanged("Dictation is already recording"),
        true => {
            // A reply still held belongs to a recording that already ended;
            // it must never answer this one.
            hotkeys.fail_print("its recording already ended");
            if let Some(refusal) = capture_refusal(other_recording, suspended) {
                return Ok(refusal);
            }
            if !hotkeys.dictation.lock() {
                return Ok(ControlReply::error(
                    "busy",
                    "the dictation shortcut is held; release it and try again",
                ));
            }
            hotkeys.raw_capture = raw;
            let accepted = handle_hotkey_action(
                HotkeyAction::Start,
                raw,
                None,
                capture_boundary(input, release_while_idle),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
            )?;
            if accepted {
                // A warm start already queued its provisional tone. A control
                // start has no hold threshold to wait for, so its capture is
                // real now and reads its page at once.
                context.intend();
                make_control_capture_intentional(input, release_while_idle)?;
                if let Some(print) = print {
                    // However this recording ends, its text comes back here.
                    hotkeys.keep_print(print);
                    ControlReply::done("Recording; the transcript comes back here when it ends")
                } else {
                    ControlReply::done("Recording; `hex dictation stop` or the shortcut finishes")
                }
            } else {
                hotkeys.dictation.suspend();
                ControlReply::error("busy", "the microphone is busy; try again")
            }
        }
        false if !recording && print.is_some() => ControlReply::error(
            "not-recording",
            "dictation isn't recording; nothing to print",
        ),
        false if !recording => ControlReply::unchanged("Dictation isn't recording"),
        false => {
            // Whether this command prints, read before a print-only
            // recording's start reply joins it: a plain stop answers as a
            // plain stop does.
            let printing = print.is_some();
            let start_printed = hotkeys.print_capture.is_some();
            // A print-only recording answers its start command too: both it
            // and a printing stop hear the one transcript.
            let print = match (hotkeys.print_capture.take(), print) {
                (Some(start), Some(stop)) => Some(start.join(stop)),
                (Some(start), None) => Some(start),
                (None, print) => print,
            };
            hotkeys.suspend_recording();
            // A control capture asked for its page when it started; this
            // waits for that read if it is still in flight.
            let context = context.finish(events)?;
            let finished = finish_hotkey_dictation(
                capture_boundary(input, release_while_idle),
                input,
                worker,
                &context,
                &device,
                events,
                indicator,
                release_while_idle,
                print,
                hotkeys.raw_capture,
            )?;
            match finished {
                FinishResult::Discarded | FinishResult::Silent if printing => {
                    ControlReply::error("no-speech", "no speech was recorded; nothing to print")
                }
                finished if printing => finish_reply(
                    Some(finished),
                    "Finished recording; printing the transcript",
                ),
                finished if start_printed => finish_reply(
                    Some(finished),
                    "Finished recording; the transcript goes to the command that started it",
                ),
                finished => finish_reply(
                    Some(finished),
                    "Finished recording; Hex pastes the text when it's ready",
                ),
            }
        }
    })
}

/// Pastes the last dictation at the current focus.
pub(super) fn paste_last(
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
) -> Result<ControlReply> {
    let reply = match worker.paste_last() {
        Ok(()) => ControlReply::done("Pasting the last dictation"),
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            ControlReply::error("failed", format!("couldn't paste: {error}"))
        }
    };
    emit_engine_state(events, false, worker, device)?;
    Ok(reply)
}

pub(super) fn fail_capture(
    message: String,
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    feedback::play(Tone::Error);
    events.dictation(DictationPhase::Failed(message.clone()), "")?;
    indicator.send(DictationIndicatorEvent::Failed {
        message: Some(message),
    });
    emit_engine_state(events, false, worker, device)
}

#[derive(Clone, Copy)]
enum CaptureEnd {
    /// The capture never became a dictation; no feedback tone.
    Discarded(Option<DiscardCause>),
    /// The capture ended unfinished: the cancel shortcut, or Hex itself
    /// when its shortcuts were suspended or the event tap was lost.
    Cancelled,
}

/// Cancels the active capture before reporting the ending.
fn end_capture(
    end: CaptureEnd,
    dictation: &DictationAudio,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    dictation.cancel()?;
    let indicator_event = match end {
        CaptureEnd::Discarded(cause) => {
            events.discarded_dictation(cause.map(DiscardCause::as_str))?;
            DictationIndicatorEvent::Discarded
        }
        CaptureEnd::Cancelled => {
            events.dictation(DictationPhase::Cancelled, "")?;
            DictationIndicatorEvent::Cancelled
        }
    };
    indicator.send(indicator_event);
    emit_state(events, false, device)
}

pub(super) fn emit_state(events: &mut EventLog, suppressed: bool, device: &str) -> Result<()> {
    emit_voice_state(events, suppressed, false, device)
}

pub(super) fn emit_engine_state(
    events: &mut EventLog,
    capturing: bool,
    worker: &DictationWorker,
    device: &str,
) -> Result<()> {
    emit_voice_state(events, capturing, worker.is_busy(), device)
}

pub(super) fn emit_voice_state(
    events: &mut EventLog,
    capturing: bool,
    transcribing: bool,
    device: &str,
) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: if capturing {
            VoiceState::Dictating
        } else if transcribing {
            VoiceState::Transcribing
        } else {
            VoiceState::Listening
        },
        device: device.into(),
    })?;
    Ok(())
}

pub(super) fn handle_dictation_event(
    event: WorkerEvent,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    match event {
        WorkerEvent::Completed {
            job_id,
            result: Ok(text),
            ..
        } if text.trim().is_empty() => {
            events.dictation(DictationPhase::Discarded, "")?;
            indicator.send(DictationIndicatorEvent::JobCompleted {
                job_id: job_id.value(),
            });
        }
        WorkerEvent::Completed {
            job_id,
            target,
            result: Ok(text),
            processing,
        } => {
            let phase = match target {
                TranscriptionTarget::Paste
                | TranscriptionTarget::CopyOnlyPaste
                | TranscriptionTarget::RawCopyOnlyPaste
                | TranscriptionTarget::RawPaste => DictationPhase::Pasted,
                TranscriptionTarget::Print | TranscriptionTarget::RawPrint => {
                    DictationPhase::Printed
                }
                TranscriptionTarget::Service => return Ok(()),
            };
            feedback::play(Tone::DictationDone);
            events.processed_dictation(phase, text, processing)?;
            indicator.send(DictationIndicatorEvent::JobCompleted {
                job_id: job_id.value(),
            });
        }
        WorkerEvent::Completed {
            job_id,
            result: Err(error),
            ..
        } => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.clone()), "")?;
            indicator.send(DictationIndicatorEvent::JobFailed {
                job_id: job_id.value(),
                message: Some(error),
            });
        }
        WorkerEvent::Stage { job_id, stage } => {
            indicator.send(match stage {
                DictationJobStage::Transcribing => DictationIndicatorEvent::Transcribing {
                    job_id: job_id.value(),
                },
                DictationJobStage::Processing => DictationIndicatorEvent::Processing {
                    job_id: job_id.value(),
                },
            });
        }
        WorkerEvent::Cancelled { job_id } => {
            // A submitted job left the HUD's job set only when it completed
            // or failed; tell both the HUD and the light that it is gone.
            indicator.send(DictationIndicatorEvent::JobCancelled {
                job_id: job_id.value(),
            });
        }
        WorkerEvent::ModelFailed(error) => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.clone()), "")?;
            indicator.send(DictationIndicatorEvent::Failed {
                message: Some(error),
            });
        }
        WorkerEvent::Pasted {
            kind,
            result: Ok(text),
        } => {
            let phase = match kind {
                PasteKind::LastTranscript => DictationPhase::Repasted,
                PasteKind::MeetingDelta => DictationPhase::MeetingPasted,
            };
            events.dictation(phase, text)?;
        }
        WorkerEvent::Pasted {
            result: Err(error), ..
        } => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
        }
    }
    Ok(())
}
