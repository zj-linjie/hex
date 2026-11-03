//! The optional Xiaozhi assistant controls and current session.
use super::controls::{self, ButtonKind, CardRow, Paint, button, section, toggle_row};
use super::platform::{AssistantRequest, PlatformRequest};
use super::{Pane, Ui};
use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::tr;
use crate::voice_assistant::{self, Status};
use rune::{El, col, row, text};

#[derive(Default)]
pub(super) struct AssistantPane {
    chooser: Option<Pending>,
    pub(super) fixture: Option<voice_assistant::Snapshot>,
}

pub(super) fn poll(ui: &mut Ui) -> bool {
    if let Some(choice) = ui.assistant.chooser.as_ref().and_then(Pending::poll) {
        ui.assistant.chooser = None;
        match choice {
            Ok(Some(path)) => {
                if let Err(error) = voice_assistant::import_identity(&path) {
                    ui.error = Some(error);
                }
            }
            Err(error) => ui.error = Some(error),
            Ok(None) => {}
        }
        return true;
    }
    ui.live() && ui.pane == Pane::VoiceAssistant && !ui.labs_open
}

fn choose_identity(ui: &mut Ui) {
    if !ui.live() || ui.assistant.chooser.is_some() {
        return;
    }
    let waker = ui.modes.waker.clone();
    ui.assistant.chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Import assistant identity").into(),
            target: Target::File(vec![Filter {
                label: tr("JSON files").into(),
                extensions: &["json"],
            }]),
            directory: None,
        },
        move || {
            if let Some(waker) = &waker {
                waker();
            }
        },
    ));
}

/// Fixtures use an idle capture projection and never read the live listener.
fn capture_status(ui: &Ui) -> &'static str {
    #[cfg(target_os = "macos")]
    if ui.live() {
        return match crate::recognition::assistant_capture_status() {
            crate::recognition::AssistantCaptureStatus::Idle => "",
            crate::recognition::AssistantCaptureStatus::Recording => tr("Recording"),
            crate::recognition::AssistantCaptureStatus::Busy => tr("Microphone is in use"),
            crate::recognition::AssistantCaptureStatus::Unavailable => {
                tr("Microphone unavailable; check permissions and listener")
            }
        };
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ui;
    ""
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let snap = ui
        .assistant
        .fixture
        .clone()
        .unwrap_or_else(voice_assistant::snapshot);
    let status = match &snap.status {
        Status::Disabled => tr("Disabled").to_string(),
        Status::NeedsIdentity => tr("Import your existing assistant identity").to_string(),
        Status::Connecting => tr("Connecting").to_string(),
        Status::Ready => tr("Connected").to_string(),
        Status::Listening => tr("Recording").to_string(),
        Status::Waiting => tr("Waiting for a reply").to_string(),
        Status::Speaking if snap.playback => tr("Playing reply").to_string(),
        Status::Speaking => tr("Receiving reply").to_string(),
        Status::Cancelled => tr("Cancelled").to_string(),
        Status::Error(error) => error.clone(),
    };
    let mut settings: Vec<CardRow> = Vec::new();
    settings.extend(super::general::shortcut_rows(
        ui,
        p,
        "dictation-shortcut",
        &super::icons::KEYBOARD,
        tr("Dictation"),
        super::ShortcutTarget::Dictation,
        None,
    ));
    settings.extend(super::general::shortcut_rows(
        ui,
        p,
        "assistant-shortcut",
        &super::icons::KEYBOARD,
        tr("Hold to talk"),
        super::ShortcutTarget::Assistant,
        None,
    ));
    settings.push(
        toggle_row(
            p,
            "assistant-playback",
            tr("Play replies"),
            None,
            ui.settings.voice_assistant.playback,
            |ui| {
                ui.update(|settings| {
                    settings.voice_assistant.playback = !settings.voice_assistant.playback
                });
            },
        )
        .into(),
    );
    let controls = row()
        .key("assistant-actions")
        .gap(8.0)
        .child(button(
            p,
            "assistant-import",
            tr("Import identity"),
            ButtonKind::Secondary,
            |ui, _| choose_identity(ui),
        ))
        .child(button(
            p,
            "assistant-reconnect",
            tr("Reconnect"),
            ButtonKind::Secondary,
            |ui, _| ui.ask(PlatformRequest::Assistant(AssistantRequest::Reconnect)),
        ));
    let shortcut_conflict = ui.settings.hotkey_in_use(
        crate::app_settings::HotkeySlot::Assistant,
        &ui.settings.voice_assistant.hotkey,
    );
    let mut content = controls::page("assistant-content")
        .child(section(p, "assistant-settings", tr("Voice Assistant"), settings))
        .child(controls::note("assistant-status", &status, p.muted()))
        .children(snap.audio_error.as_ref().map(|error| controls::note("assistant-audio-error", error, p.warning())))
        .children(shortcut_conflict.then(|| controls::note("assistant-shortcut-conflict", tr("Assistant shortcut is already in use; choose another shortcut."), p.muted())))
        .child(controls)
        .child(controls::note("assistant-privacy", tr("Your voice is sent to Xiaozhi. Tools use the MCP bridge configured in Xiaozhi; bridge status is unverified. Cancelling requests an abort but cannot undo a tool action already sent."), p.muted()));
    if ui.platform.caps.voice_assistant {
        content = content.child(
            row()
                .key("assistant-capture-actions")
                .gap(8.0)
                .child(button(
                    p,
                    "assistant-start",
                    tr("Start speaking"),
                    ButtonKind::Primary,
                    |ui, _| ui.ask(PlatformRequest::Assistant(AssistantRequest::Start)),
                ))
                .child(button(
                    p,
                    "assistant-finish",
                    tr("End and send"),
                    ButtonKind::Secondary,
                    |ui, _| ui.ask(PlatformRequest::Assistant(AssistantRequest::Finish)),
                ))
                .child(button(
                    p,
                    "assistant-cancel",
                    tr("Cancel"),
                    ButtonKind::Secondary,
                    |ui, _| ui.ask(PlatformRequest::Assistant(AssistantRequest::Cancel)),
                )),
        );
        content = content.child(controls::note(
            "assistant-capture-status",
            capture_status(ui),
            p.muted(),
        ));
    }
    for (index, turn) in snap.turns.iter().enumerate() {
        let reply = turn.reply.clone();
        content = content.child(
            col()
                .key(format!("assistant-turn-{index}"))
                .gap(4.0)
                .child(
                    text(&turn.transcript)
                        .font(p.k.ty.ui)
                        .color(p.muted())
                        .wrap(),
                )
                .child(
                    text(&turn.reply)
                        .font(p.k.ty.ui)
                        .color(p.k.c.text_base)
                        .wrap(),
                )
                .child(button(
                    p,
                    &format!("assistant-copy-{index}"),
                    tr("Copy"),
                    ButtonKind::Secondary,
                    move |_, tx| tx.copy(reply.clone()),
                )),
        );
    }
    content
}
