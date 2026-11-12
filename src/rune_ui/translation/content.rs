//! Shared query field and result card for the Translation pane and popup.

use rune::{El, Key, TextEdit, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, TextArea};

use super::super::controls::{DESCRIPTION, Paint, PushSize};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::translation::{
    CompatiblePreview, DictionaryStatus, InstallStage, LookupError, MAX_QUERY_CHARS, Outcome,
    Phase, PhraseResult, Recovery, ResourceStatus, Snapshot, Source, SystemTranslationStatus,
    WordResult,
};

pub(super) fn query_field<S: 'static>(
    p: Paint,
    edit: &TextEdit,
    on_edit: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + Copy + 'static,
    on_submit: impl Fn(&mut S, &mut Tx) + Copy + 'static,
    on_query: impl Fn(&mut S, String) + Copy + 'static,
) -> El {
    let count = edit.text.chars().count();
    let too_long = count > MAX_QUERY_CHARS;
    col()
        .key("translation-query")
        .gap(6.0)
        .child(
            TextArea::new(edit)
                .placeholder(tr("Enter an English word or phrase"))
                .label(tr("Translation query"))
                .rows(3)
                .auto_grow(2, 6)
                .width(None)
                .invalid(too_long)
                .on_edit(on_edit)
                .on_change(move |s, _| {
                    let text = on_edit(s).text.clone();
                    on_query(s, text);
                })
                .on_key(move |s, event, tx| {
                    if event.key == Key::Enter && !event.mods.shift {
                        // Rune `route_key` swallows Enter while `composing()`
                        // before a field `on_input_key`. Keep the same gate if
                        // a preedit is still visible, and ignore key repeat.
                        let edit = on_edit(s);
                        if !enter_submits(edit, tx.key_repeat()) {
                            return false;
                        }
                        on_submit(s, tx);
                        true
                    } else {
                        false
                    }
                })
                .build(p.k)
                .key("translation-query-field")
                .label(tr("Translation query")),
        )
        .child(query_meta(p, count, too_long))
}

pub(super) fn enter_submits(edit: &TextEdit, key_repeat: bool) -> bool {
    enter_submits_given(edit.preedit().is_none(), &edit.text, key_repeat)
}

/// [`enter_submits`] without the edit, so tests name an active composition.
pub(super) fn enter_submits_given(composing: bool, text: &str, key_repeat: bool) -> bool {
    !composing && !key_repeat && !text.trim().is_empty() && text.chars().count() <= MAX_QUERY_CHARS
}

pub(super) fn live_engine() -> bool {
    crate::labs::translation_loaded()
}

pub(super) fn live_edit(text: String) {
    if live_engine() {
        crate::translation::edit(text);
    }
}

pub(super) fn live_submit() {
    if live_engine() {
        crate::translation::submit_local();
    }
}

pub(super) fn live_cancel() {
    if live_engine() {
        crate::translation::cancel();
    }
}

/// Both surfaces render the authoritative text, even if their focus flag
/// survives a window switch. Defer only active IME composition, and retry
/// after it ends without requiring another backend revision.
pub(super) fn sync_query(edit: &mut TextEdit, revision: &mut u64, snap: &Snapshot) -> bool {
    sync_query_given(edit, edit.preedit().is_some(), revision, snap)
}

/// [`sync_query`] with the composition named, so tests can set one: Rune's
/// `TextEdit` only reads its input method state.
pub(super) fn sync_query_given(
    edit: &mut TextEdit,
    composing: bool,
    revision: &mut u64,
    snap: &Snapshot,
) -> bool {
    if composing {
        return false;
    }
    let changed = *revision != snap.revision || edit.text != snap.text;
    *revision = snap.revision;
    if edit.text != snap.text {
        let (anchor, focus) = (edit.anchor, edit.focus);
        set_query(edit, snap.text.clone());
        edit.anchor = text_boundary(&edit.text, anchor);
        edit.focus = text_boundary(&edit.text, focus);
    }
    changed
}

fn text_boundary(text: &str, position: usize) -> usize {
    let mut position = position.min(text.len());
    while !text.is_char_boundary(position) {
        position -= 1;
    }
    position
}

fn query_meta(p: Paint, count: usize, too_long: bool) -> El {
    let copy = if too_long {
        tr("Use at most 1000 characters")
    } else {
        ""
    };
    row()
        .key("translation-query-meta")
        .gap(8.0)
        .child(
            text(tr_fill("{} / 1000", &count.to_string()))
                .key("count")
                .font(DESCRIPTION)
                .color(if too_long { p.danger() } else { p.faint() }),
        )
        .child(
            text(copy)
                .key("limit")
                .font(DESCRIPTION)
                .color(p.danger())
                .grow(1.0),
        )
}

pub(super) fn query_actions<S: 'static>(
    p: Paint,
    can_submit: bool,
    phrase: bool,
    on_paste: impl Fn(&mut S, &mut Tx) + 'static,
    on_clear: impl Fn(&mut S, &mut Tx) + 'static,
    on_submit: impl Fn(&mut S, &mut Tx) + 'static,
) -> El {
    row()
        .key("translation-query-actions")
        .gap(8.0)
        .child(action_button(
            p,
            "translation-paste",
            tr("Paste"),
            ButtonVariant::Neutral,
            on_paste,
        ))
        .child(action_button(
            p,
            "translation-clear",
            tr("Clear query"),
            ButtonVariant::Ghost,
            on_clear,
        ))
        .child(el().key("spacer").grow(1.0))
        .child(
            Button::new(if phrase {
                tr("Translate")
            } else {
                tr("Look up")
            })
            .variant(if can_submit {
                ButtonVariant::Contrast
            } else {
                ButtonVariant::Neutral
            })
            .disabled(!can_submit)
            .on_press_tx(on_submit)
            .build_push(p.k)
            .key("translation-submit"),
        )
}

pub(super) fn paste_clipboard<S: 'static>(tx: &mut Tx, apply: impl Fn(&mut S, String) + 'static) {
    tx.read_clipboard(move |s: &mut S, content, _| {
        let text = content.text().trim();
        if !text.is_empty() {
            apply(s, text.to_string());
        }
    });
}

pub(super) fn set_query(edit: &mut TextEdit, text: String) {
    edit.replace_text(text);
}

pub(super) fn submit_local(text: &str) {
    // Edits already publish the query. A stale window must never submit its
    // old buffer over the current shared text.
    if live_engine() && crate::translation::snapshot().text == text {
        live_submit();
    }
}

pub(super) fn can_submit(text: &str, phase: Phase) -> bool {
    let count = text.chars().count();
    !text.trim().is_empty()
        && count <= MAX_QUERY_CHARS
        && !matches!(phase, Phase::LocalPending | Phase::OnlinePending)
}

pub(super) fn looks_like_phrase(text: &str) -> bool {
    text.split_whitespace().nth(1).is_some()
}

pub(super) fn result_area<S: 'static>(
    p: Paint,
    snap: &Snapshot,
    preview_open: bool,
    on_recover: impl Fn(&mut S, Recovery, &mut Tx) + Copy + 'static,
    on_confirm: impl Fn(&mut S, CompatiblePreview, &mut Tx) + Copy + 'static,
    on_cancel_preview: impl Fn(&mut S, &mut Tx) + Copy + 'static,
) -> El {
    let mut body = col().key("translation-result").gap(12.0);
    if let Some(install) = &snap.dictionary_install {
        body = body.child(install_note(
            p,
            install.received,
            install.total,
            install.stage,
        ));
    }
    match snap.phase {
        Phase::LocalPending | Phase::OnlinePending => {
            body = body.child(status_note(
                p,
                "translation-loading",
                if snap.phase == Phase::OnlinePending {
                    tr("Sending to the translation service…")
                } else {
                    tr("Looking up…")
                },
            ));
        }
        Phase::OnlinePreview => {
            if let Some(preview) = &snap.online_preview {
                body = body.child(preview_card(p, preview, on_confirm, on_cancel_preview));
            }
        }
        Phase::Ready | Phase::Idle => {}
    }
    if let Some(error) = &snap.error {
        body = body.child(error_card(p, error, preview_open, on_recover));
    } else if let Some(outcome) = &snap.outcome {
        body = body.child(match outcome {
            Outcome::Word(word) => word_card::<S>(p, word, snap),
            Outcome::Phrase(phrase) => phrase_card::<S>(p, phrase, snap),
        });
    } else if snap.phase == Phase::Idle && snap.text.trim().is_empty() {
        body = body.child(status_note(
            p,
            "translation-empty",
            tr("Enter English text to translate"),
        ));
    }
    body.child(resources_row(p, &snap.resources, on_recover))
}

fn word_card<S: 'static>(p: Paint, word: &WordResult, snap: &Snapshot) -> El {
    let mut card = col().key("translation-word").gap(8.0).child(
        text(&word.headword)
            .key("headword")
            .role("heading")
            .level(2)
            .font(p.k.ty.ui_medium)
            .color(p.text()),
    );
    if let Some(mapped) = word.mapped_from.as_ref().map(|_| &word.query) {
        card = card.child(
            text(format!("{mapped} → {}", word.headword))
                .key("mapped-from")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
    }
    let phonetic = word
        .phonetic
        .as_deref()
        .map(|ipa| format!("/{ipa}/"))
        .unwrap_or_else(|| tr("No phonetic").to_string());
    card = card.child(
        text(phonetic)
            .key("phonetic")
            .font(p.k.ty.ui)
            .color(p.muted()),
    );
    card = card.child(super::speech::voices_row::<S>(p, snap, &word.headword));
    for (index, sense) in word.senses.iter().enumerate() {
        let pos = sense.pos.as_deref().unwrap_or("");
        card = card.child(
            row()
                .key(format!("sense-{index}"))
                .gap(8.0)
                .child(
                    text(pos)
                        .key("pos")
                        .font(p.k.ty.ui_medium)
                        .color(p.accent())
                        .w(36.0),
                )
                .child(
                    text(&sense.gloss)
                        .key("gloss")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .wrap()
                        .grow(1.0),
                ),
        );
    }
    let copy = word
        .senses
        .iter()
        .map(|sense| match sense.pos.as_deref() {
            Some(pos) if !pos.is_empty() => format!("{pos} {}", sense.gloss),
            _ => sense.gloss.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    card.child(source_row::<S>(p, word.source, copy, tr("Copy gloss")))
}

fn phrase_card<S: 'static>(p: Paint, phrase: &PhraseResult, snap: &Snapshot) -> El {
    col()
        .key("translation-phrase")
        .gap(8.0)
        .child(
            text(tr("Original"))
                .key("original-label")
                .font(DESCRIPTION)
                .color(p.muted()),
        )
        .child(
            text(&phrase.query)
                .key("original")
                .font(p.k.ty.ui)
                .color(p.text())
                .wrap(),
        )
        .child(super::speech::voices_row::<S>(p, snap, &phrase.query))
        .child(
            text(tr("Chinese"))
                .key("chinese-label")
                .font(DESCRIPTION)
                .color(p.muted()),
        )
        .child(
            text(&phrase.translation)
                .key("chinese")
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .wrap(),
        )
        .child(source_row::<S>(
            p,
            phrase.source,
            phrase.translation.clone(),
            tr("Copy translation"),
        ))
}

fn source_row<S: 'static>(p: Paint, source: Source, copy: String, copy_label: &'static str) -> El {
    row()
        .key("translation-source")
        .gap(8.0)
        .center_y()
        .child(
            text(source_label(source))
                .key("label")
                .font(DESCRIPTION)
                .color(p.muted())
                .grow(1.0),
        )
        .child(action_button(
            p,
            "translation-copy",
            copy_label,
            ButtonVariant::Neutral,
            move |_: &mut S, tx| tx.copy(copy.clone()),
        ))
}

pub(super) fn source_label(source: Source) -> &'static str {
    match source {
        Source::Dictionary => tr("Local dictionary"),
        Source::DictionaryLemma => tr("Local dictionary · lemma"),
        Source::System => tr("On-device translation"),
        Source::Compatible => tr("Compatible API"),
    }
}

fn error_card<S: 'static>(
    p: Paint,
    error: &LookupError,
    preview_open: bool,
    on_recover: impl Fn(&mut S, Recovery, &mut Tx) + Copy + 'static,
) -> El {
    let mut card = col().key("translation-error").gap(8.0).child(
        text(tr(error.message))
            .key("message")
            .font(p.k.ty.ui)
            .color(p.danger())
            .wrap(),
    );
    if !error.recovery.is_empty() && !preview_open {
        let mut actions = row().key("translation-recovery").gap(8.0).flex_wrap();
        for &recovery in &error.recovery {
            let label = recovery_label(recovery);
            actions = actions.child(action_button(
                p,
                recovery_key(recovery),
                label,
                if matches!(recovery, Recovery::PrepareOnline) {
                    ButtonVariant::Neutral
                } else {
                    ButtonVariant::Contrast
                },
                move |s, tx| on_recover(s, recovery, tx),
            ));
        }
        card = card.child(actions);
    }
    card
}

fn preview_card<S: 'static>(
    p: Paint,
    preview: &CompatiblePreview,
    on_confirm: impl Fn(&mut S, CompatiblePreview, &mut Tx) + Copy + 'static,
    on_cancel: impl Fn(&mut S, &mut Tx) + Copy + 'static,
) -> El {
    let send = preview.clone();
    col()
        .key("translation-online-preview")
        .gap(8.0)
        .child(
            text(tr("Send this text to the compatible API?"))
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text(&preview.text)
                .key("preview-text")
                .font(p.k.ty.ui)
                .color(p.text())
                .wrap(),
        )
        .child(
            text(tr_fill("Service: {}", &preview.host))
                .key("preview-host")
                .font(DESCRIPTION)
                .color(p.muted()),
        )
        .child(
            text(tr_fill("Model: {}", &preview.model))
                .key("preview-model")
                .font(DESCRIPTION)
                .color(p.muted()),
        )
        .child(
            row()
                .key("preview-actions")
                .gap(8.0)
                .child(action_button(
                    p,
                    "translation-online-send",
                    tr("Send"),
                    ButtonVariant::Contrast,
                    move |s, tx| on_confirm(s, send.clone(), tx),
                ))
                .child(action_button(
                    p,
                    "translation-online-cancel",
                    tr("Cancel"),
                    ButtonVariant::Neutral,
                    on_cancel,
                )),
        )
}

fn resources_row<S: 'static>(
    p: Paint,
    resources: &ResourceStatus,
    on_recover: impl Fn(&mut S, Recovery, &mut Tx) + Copy + 'static,
) -> El {
    col()
        .key("translation-resources")
        .gap(6.0)
        .child(
            text(dictionary_status(resources))
                .key("dictionary-status")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
        .child(
            text(system_status(resources))
                .key("system-status")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
        .child(
            row()
                .key("translation-resource-actions")
                .gap(8.0)
                .flex_wrap()
                .child(action_button(
                    p,
                    "translation-recheck",
                    tr("Recheck resources"),
                    ButtonVariant::Neutral,
                    move |s, tx| on_recover(s, Recovery::RecheckSystem, tx),
                ))
                .children(
                    matches!(
                        resources.dictionary,
                        DictionaryStatus::Missing { .. } | DictionaryStatus::Failed { .. }
                    )
                    .then(|| {
                        action_button(
                            p,
                            "translation-install",
                            tr("Install dictionary"),
                            ButtonVariant::Neutral,
                            move |s, tx| on_recover(s, Recovery::InstallDictionary, tx),
                        )
                    }),
                )
                .children(
                    matches!(resources.dictionary, DictionaryStatus::Installing { .. }).then(
                        || {
                            action_button(
                                p,
                                "translation-cancel-install",
                                tr("Cancel"),
                                ButtonVariant::Neutral,
                                |_: &mut S, _| {
                                    if live_engine() {
                                        crate::translation::cancel_install();
                                    }
                                },
                            )
                        },
                    ),
                )
                .child(action_button(
                    p,
                    "translation-dictionary-help",
                    tr("About the dictionary"),
                    ButtonVariant::Ghost,
                    |_: &mut S, tx| {
                        let _ = tx.open_url("https://github.com/skywind3000/ECDICT");
                    },
                )),
        )
}

fn dictionary_status(resources: &ResourceStatus) -> String {
    match &resources.dictionary {
        DictionaryStatus::Ready { entries } => {
            tr_fill("Dictionary ready · {} entries", &entries.to_string())
        }
        DictionaryStatus::Missing { bytes } => tr_fill(
            "Dictionary not installed · {} to download",
            &format_bytes(*bytes),
        ),
        DictionaryStatus::Installing {
            received,
            total,
            stage,
        } => tr_named(
            "Installing dictionary ({stage})… {received} / {total}",
            &[
                ("stage", stage_label(*stage)),
                ("received", &format_bytes(*received)),
                ("total", &format_bytes(*total)),
            ],
        ),
        DictionaryStatus::Failed { message } => tr_fill("Dictionary failed: {}", message),
    }
}

fn system_status(resources: &ResourceStatus) -> String {
    match &resources.system {
        SystemTranslationStatus::Ready => tr("On-device translation is ready").to_string(),
        SystemTranslationStatus::NotInstalled => {
            tr("Install English and Simplified Chinese in System Settings › Language & Region")
                .to_string()
        }
        SystemTranslationStatus::Unsupported => {
            tr("On-device translation needs macOS 26 or later").to_string()
        }
        SystemTranslationStatus::Failed { message } => {
            tr_fill("On-device translation failed: {}", message)
        }
    }
}

fn install_note(p: Paint, received: u64, total: u64, stage: InstallStage) -> El {
    let copy = tr_named(
        "Installing dictionary ({stage})… {received} / {total}",
        &[
            ("stage", stage_label(stage)),
            ("received", &format_bytes(received)),
            ("total", &format_bytes(total)),
        ],
    );
    status_note(p, "translation-install", &copy)
}

fn stage_label(stage: InstallStage) -> &'static str {
    match stage {
        InstallStage::Dictionary => tr("dictionary file"),
        InstallStage::Lemma => tr("lemma list"),
        InstallStage::Convert => tr("converting"),
        InstallStage::Verify => tr("verifying"),
    }
}

fn status_note(p: Paint, key: &'static str, copy: &str) -> El {
    text(copy).key(key).font(p.k.ty.ui).color(p.muted()).wrap()
}

pub(super) fn recovery_label(recovery: Recovery) -> &'static str {
    match recovery {
        Recovery::InstallDictionary => tr("Install dictionary"),
        Recovery::RecheckSystem => tr("Recheck resources"),
        Recovery::OpenSystemLanguageSettings => tr("Open Language & Region settings"),
        Recovery::OpenCompatibleSettings => tr("Open compatible API settings"),
        Recovery::PrepareOnline => tr("Translate online…"),
    }
}

fn recovery_key(recovery: Recovery) -> &'static str {
    match recovery {
        Recovery::InstallDictionary => "translation-recover-install",
        Recovery::RecheckSystem => "translation-recover-recheck",
        Recovery::OpenSystemLanguageSettings => "translation-recover-language",
        Recovery::OpenCompatibleSettings => "translation-recover-compatible",
        Recovery::PrepareOnline => "translation-recover-online",
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn action_button<S: 'static>(
    p: Paint,
    key: &'static str,
    label: impl Into<String>,
    variant: ButtonVariant,
    press: impl Fn(&mut S, &mut Tx) + 'static,
) -> El {
    Button::new(label.into())
        .variant(variant)
        .on_press_tx(press)
        .build_push(p.k)
        .key(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::TextEdit;

    #[test]
    fn enter_does_not_submit_ime_composition_or_repeat() {
        let mut edit = TextEdit::new("hex");
        assert!(enter_submits_given(false, &edit.text, false));
        assert!(!enter_submits_given(false, &edit.text, true));
        assert!(!enter_submits_given(true, &edit.text, false));
        edit.replace_text("");
        assert!(!enter_submits(&edit, false));
        edit.replace_text("a".repeat(MAX_QUERY_CHARS + 1));
        assert!(!enter_submits(&edit, false));
    }

    #[test]
    fn shared_query_updates_a_focused_window_without_moving_its_selection() {
        let mut query = TextEdit::new("hello");
        query.focused = true;
        query.anchor = 1;
        query.focus = 3;
        let mut revision = 1;
        let mut snap = super::super::fixtures::word();
        snap.revision = 2;
        snap.text = "world".into();
        assert!(sync_query_given(&mut query, false, &mut revision, &snap));
        assert_eq!(query.text, "world");
        assert_eq!((query.anchor, query.focus), (1, 3));
        assert!(query.focused);
        assert_eq!(revision, 2);
        assert!(!sync_query_given(&mut query, false, &mut revision, &snap));
        snap.text = "中".into();
        assert!(sync_query_given(&mut query, false, &mut revision, &snap));
        assert!(query.text.is_char_boundary(query.anchor));
        assert!(query.text.is_char_boundary(query.focus));
    }

    #[test]
    fn preedit_blocks_snapshot_replace() {
        let mut query = TextEdit::new("he");
        let mut revision = 1;
        let mut snap = super::super::fixtures::word();
        snap.revision = 4;
        snap.text = "running".into();
        sync_query_given(&mut query, true, &mut revision, &snap);
        assert_eq!(query.text, "he");
        assert_eq!(revision, 1);
        assert!(sync_query_given(&mut query, false, &mut revision, &snap));
        assert_eq!(query.text, "running");
        assert_eq!(revision, 4);
    }
}
