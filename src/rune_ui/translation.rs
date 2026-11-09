//! Labs › Translation: the Hex pane, shared query/result, and the extra window.

mod content;
mod fixtures;
mod popup;
mod speech;

use std::time::Duration;

use rune::{El, TextEdit, Tx, col};
use rune_kit::{Button, ButtonVariant};

use super::controls::{self, Paint, PushSize, section};
use super::general::shortcut_rows;
use super::{Destination, Pane, ShortcutTarget, Ui};
use crate::desktop::i18n::tr;
use crate::translation::{
    self, CompatiblePreview, DictionaryStatus, Phase, Recovery, ResourceStatus, Snapshot,
    SystemTranslationStatus,
};

pub(crate) use self::popup::{hide_popup, install_popup, open_popup};

const PENDING_WAKE: Duration = Duration::from_millis(50);

#[derive(Default)]
pub(super) struct TranslationPane {
    pub(super) query: TextEdit,
    pub(super) fixture: Option<Snapshot>,
    revision: u64,
}

pub(super) fn poll(ui: &mut Ui, tx: &mut Tx) -> bool {
    if ui.live() && !crate::labs::translation() {
        hide_popup();
    }
    let Some(snap) = snapshot(ui) else {
        return ui.live() && ui.pane == Pane::Translation && !ui.labs_open;
    };
    let changed = content::sync_query(
        &mut ui.translation.query,
        &mut ui.translation.revision,
        &snap,
    );
    if pending_work(&snap) {
        tx.wake_after(PENDING_WAKE);
    }
    changed || (ui.live() && ui.pane == Pane::Translation && !ui.labs_open)
}

fn pending_work(snap: &Snapshot) -> bool {
    matches!(snap.phase, Phase::LocalPending | Phase::OnlinePending)
        || snap.speaking.is_some()
        || snap.dictionary_install.is_some()
}

fn snapshot(ui: &Ui) -> Option<Snapshot> {
    if let Some(fixture) = ui.translation.fixture.clone() {
        return Some(fixture);
    }
    ui.live()
        .then(|| crate::labs::translation_loaded().then(translation::snapshot))
        .flatten()
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let snap = snapshot(ui).unwrap_or_else(idle_snapshot);
    let preview_open = snap.phase == Phase::OnlinePreview || snap.online_preview.is_some();
    let can_submit = content::can_submit(&ui.translation.query.text, snap.phase);
    let phrase = content::looks_like_phrase(&ui.translation.query.text);
    let mut settings = shortcut_rows(
        ui,
        p,
        "translation-hotkey",
        &super::icons::KEYBOARD,
        tr("Translation shortcut"),
        ShortcutTarget::Translation,
        None,
    );
    if ui.settings.hotkey_in_use(
        crate::app_settings::HotkeySlot::Translation,
        &ui.settings.translation.hotkey,
    ) {
        settings.push(
            controls::note(
                "translation-shortcut-conflict",
                tr("Translation shortcut is already in use; choose another shortcut."),
                p.muted(),
            )
            .into(),
        );
    }
    settings.push(
        Button::new(tr("Open translation window"))
            .variant(ButtonVariant::Neutral)
            .on_press(|_: &mut Ui| open_popup())
            .build_push(p.k)
            .key("translation-open-window")
            .into(),
    );
    let content = col()
        .key("translation-page")
        .gap(16.0)
        .child(section(
            p,
            "translation-shortcut",
            tr("Translation"),
            settings,
        ))
        .child(content::query_field(
            p,
            &ui.translation.query,
            |ui: &mut Ui| &mut ui.translation.query,
            submit,
            edit_query,
        ))
        .child(content::query_actions(
            p, can_submit, phrase, paste, clear, submit,
        ))
        .child(content::result_area(
            p,
            &snap,
            preview_open,
            recover,
            confirm_online,
            cancel_preview,
        ));
    super::scroll_body(p.k, "translation-body", content)
}

pub(super) fn idle_snapshot() -> Snapshot {
    Snapshot {
        revision: 0,
        enabled: false,
        text: String::new(),
        generation: 0,
        phase: Phase::Idle,
        outcome: None,
        error: None,
        resources: ResourceStatus {
            dictionary: DictionaryStatus::Missing { bytes: 0 },
            system: SystemTranslationStatus::Unsupported,
        },
        voices: Vec::new(),
        speaking: None,
        dictionary_install: None,
        online_preview: None,
    }
}

fn edit_query(ui: &mut Ui, text: String) {
    if let Some(fixture) = &mut ui.translation.fixture {
        fixture.text = text;
        fixture.generation = fixture.generation.wrapping_add(1);
        fixture.revision = fixture.revision.wrapping_add(1);
        ui.translation.revision = fixture.revision;
        return;
    }
    content::live_edit(text);
}

fn paste(_ui: &mut Ui, tx: &mut Tx) {
    content::paste_clipboard(tx, |ui: &mut Ui, text| {
        content::set_query(&mut ui.translation.query, text.clone());
        edit_query(ui, text);
    });
}

fn clear(ui: &mut Ui, _: &mut Tx) {
    content::set_query(&mut ui.translation.query, String::new());
    edit_query(ui, String::new());
    if let Some(fixture) = &mut ui.translation.fixture {
        fixture.phase = Phase::Idle;
        fixture.outcome = None;
        fixture.error = None;
        return;
    }
    content::live_cancel();
}

fn submit(ui: &mut Ui, _: &mut Tx) {
    let text = ui.translation.query.text.clone();
    if !content::can_submit(
        &text,
        snapshot(ui).map(|snap| snap.phase).unwrap_or(Phase::Idle),
    ) {
        return;
    }
    if ui.translation.fixture.is_some() {
        edit_query(ui, text);
        if let Some(fixture) = &mut ui.translation.fixture {
            fixture.phase = Phase::LocalPending;
        }
        return;
    }
    content::submit_local(&text);
    popup::wake_popup();
}

fn recover(ui: &mut Ui, recovery: Recovery, tx: &mut Tx) {
    if ui.is_fixture() {
        return;
    }
    if !content::live_engine() {
        if matches!(recovery, Recovery::OpenCompatibleSettings) {
            super::show(ui, Destination::Modes, tx);
        }
        return;
    }
    match recovery {
        Recovery::InstallDictionary => translation::install_dictionary(),
        Recovery::RecheckSystem => translation::recheck_resources(),
        Recovery::OpenSystemLanguageSettings => {
            let _ = tx.open_url("x-apple.systempreferences:com.apple.Localization-Settings");
        }
        Recovery::OpenCompatibleSettings => {
            super::show(ui, Destination::Modes, tx);
        }
        Recovery::PrepareOnline => {
            let _ = translation::prepare_online();
        }
    }
}

fn confirm_online(ui: &mut Ui, preview: CompatiblePreview, _: &mut Tx) {
    if ui.is_fixture() || !content::live_engine() {
        return;
    }
    translation::confirm_online(preview);
}

fn cancel_preview(ui: &mut Ui, _: &mut Tx) {
    if ui.is_fixture() {
        if let Some(fixture) = &mut ui.translation.fixture {
            fixture.phase = Phase::Idle;
            fixture.online_preview = None;
        }
        return;
    }
    content::live_cancel();
}

pub(super) fn apply_scene(ui: &mut Ui, snap: Snapshot) {
    ui.settings.labs.translation = true;
    ui.pane = Pane::Translation;
    content::set_query(&mut ui.translation.query, snap.text.clone());
    ui.translation.revision = snap.revision;
    ui.translation.fixture = Some(snap);
}

pub(super) fn word_scene(ui: &mut Ui) {
    apply_scene(ui, fixtures::word());
}

pub(super) fn phrase_scene(ui: &mut Ui) {
    apply_scene(ui, fixtures::phrase());
}

pub(super) fn loading_scene(ui: &mut Ui) {
    apply_scene(ui, fixtures::loading());
}

pub(super) fn missing_scene(ui: &mut Ui) {
    apply_scene(ui, fixtures::missing());
}

pub(super) fn error_scene(ui: &mut Ui) {
    apply_scene(ui, fixtures::lookup_error());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::{MAX_QUERY_CHARS, Phase, Source};

    #[test]
    fn a_second_word_is_a_phrase() {
        assert!(!content::looks_like_phrase("hex"));
        assert!(content::looks_like_phrase("good morning"));
    }

    #[test]
    fn submit_refuses_empty_and_overlong_text() {
        assert!(!content::can_submit("", Phase::Idle));
        assert!(!content::can_submit("hex", Phase::LocalPending));
        assert!(content::can_submit("hex", Phase::Idle));
        let long = "a".repeat(MAX_QUERY_CHARS + 1);
        assert!(!content::can_submit(&long, Phase::Idle));
    }

    #[test]
    fn sources_name_dictionary_lemma_and_api() {
        assert_eq!(
            content::source_label(Source::Dictionary),
            "Local dictionary"
        );
        assert_eq!(
            content::source_label(Source::DictionaryLemma),
            "Local dictionary · lemma"
        );
        assert_eq!(content::source_label(Source::Compatible), "Compatible API");
    }

    #[test]
    fn fixture_edit_stays_on_the_fixture() {
        let mut ui = Ui::with(
            crate::app_settings::AppSettings::default(),
            super::super::Service::Fixture(Vec::new()),
        );
        apply_scene(&mut ui, fixtures::word());
        let generation = ui.translation.fixture.as_ref().unwrap().generation;
        edit_query(&mut ui, "hex".into());
        let fixture = ui.translation.fixture.as_ref().unwrap();
        assert_eq!(fixture.text, "hex");
        assert_eq!(fixture.generation, generation + 1);
        assert!(ui.is_fixture());
    }

    #[test]
    fn pending_work_wakes_for_lookup_speech_and_install() {
        assert!(pending_work(&fixtures::loading()));
        assert!(!pending_work(&fixtures::word()));
        let mut speaking = fixtures::word();
        speaking.speaking = Some(crate::translation::Speaking {
            text: "run".into(),
            voice_id: "com.apple.voice.compact.en-GB.Daniel".into(),
            rate: crate::translation::SpeechRate::Normal,
        });
        assert!(pending_work(&speaking));
    }

    #[test]
    fn idle_snapshot_does_not_touch_the_live_engine() {
        let snap = idle_snapshot();
        assert!(!snap.enabled);
        assert_eq!(snap.text, "");
    }
}
