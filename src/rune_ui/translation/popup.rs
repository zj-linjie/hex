//! The singleton Translation window: a normal, focusable extra Rune window.

use std::sync::OnceLock;
use std::time::Duration;

use rune::{AppSpec, Cx, El, TextEdit, Tx, col, row};

use super::super::controls::{Paint, PushSize};
use super::Ui;
use super::content;
use crate::desktop::i18n::tr;
use crate::translation::{self, CompatiblePreview, Phase, Recovery, Snapshot};
use rune_kit::{Button, ButtonVariant};

const SIZE: (f32, f32) = (540.0, 460.0);
const PENDING_WAKE: Duration = Duration::from_millis(50);

struct Handles {
    app: rune::AppHandle<Popup>,
    window: rune::WindowHandle,
}

static HANDLES: OnceLock<Handles> = OnceLock::new();

#[derive(Default)]
struct Popup {
    query: TextEdit,
    revision: u64,
}

#[cfg(target_os = "macos")]
pub(crate) fn install_popup(app: &mut rune::App<Ui>) {
    let popup = rune::App::new(spec()).keep_running().on_close(|_, _| {
        if content::live_engine() {
            crate::translation::stop_speech();
        }
    });
    let spec = rune::WindowSpec::from_app("translation", popup)
        .chrome(rune::Chrome {
            full_size_content: false,
            ..rune::Chrome::DEFAULT
        })
        .options(rune::WindowOptions {
            visible: false,
            min_size: Some((400.0, 320.0)),
            ..Default::default()
        });
    let (handle, window) = app.window(spec);
    let _ = HANDLES.set(Handles {
        app: handle,
        window,
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_popup(_: &mut rune::App<Ui>) {}

pub(crate) fn open_popup() {
    if !crate::labs::translation_loaded() {
        return;
    }
    #[cfg(target_os = "macos")]
    crate::macos_rune::on_main_thread(activate_on_main);
    #[cfg(not(target_os = "macos"))]
    summon();
}

#[cfg(target_os = "macos")]
fn activate_on_main() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    crate::app_settings::set_dock_icon_visible(true);
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).activate();
    }
    summon();
}

fn summon() {
    if let Some(handles) = HANDLES.get() {
        handles.app.open();
        handles.app.focus();
        handles.app.update(|popup, _| focus_query(popup));
        handles.app.wake();
    }
}

pub(super) fn wake_popup() {
    if let Some(handles) = HANDLES.get() {
        handles.app.wake();
    }
}

fn focus_query(popup: &mut Popup) {
    if let Some(snap) = live_snapshot() {
        content::sync_query(&mut popup.query, &mut popup.revision, &snap);
    }
    popup.query.focused = true;
}

pub(crate) fn hide_popup() {
    let Some(handles) = HANDLES.get() else {
        return;
    };
    if content::live_engine() {
        crate::translation::stop_speech();
    }
    handles.window.hide();
}

fn live_snapshot() -> Option<Snapshot> {
    crate::labs::translation_loaded().then(translation::snapshot)
}

fn spec() -> AppSpec<Popup> {
    AppSpec {
        title: "Translation",
        size: SIZE,
        min_size: Some((400.0, 320.0)),
        init: Popup::default,
        view,
        scenes: &[],
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: None,
        poll: Some(poll),
        on_key: Some(on_key),
    }
}

fn poll(popup: &mut Popup, _: f64, tx: &mut Tx) -> bool {
    if !crate::labs::translation_loaded() {
        return false;
    }
    let snap = translation::snapshot();
    if !snap.enabled {
        hide_popup();
        return false;
    }
    let changed = content::sync_query(&mut popup.query, &mut popup.revision, &snap);
    if pending_work(&snap) {
        tx.wake_after(PENDING_WAKE);
    }
    changed
}

fn pending_work(snap: &Snapshot) -> bool {
    matches!(snap.phase, Phase::LocalPending | Phase::OnlinePending)
        || snap.speaking.is_some()
        || snap.dictionary_install.is_some()
}

enum EscapeAction {
    Ignore,
    CancelPreview,
    Hide,
}

fn escape_action(popup: &Popup, snap: &Snapshot) -> EscapeAction {
    if popup.query.preedit.is_some() {
        return EscapeAction::Ignore;
    }
    if snap.phase == Phase::OnlinePreview || snap.online_preview.is_some() {
        return EscapeAction::CancelPreview;
    }
    EscapeAction::Hide
}

fn on_key(popup: &mut Popup, event: &rune::KeyEvent, _: &mut Tx) -> bool {
    if event.key != rune::Key::Escape {
        return false;
    }
    // Rune `route_key` swallows Escape while `composing()` before app `on_key`.
    let snap = live_snapshot();
    match snap
        .as_ref()
        .map(|snap| escape_action(popup, snap))
        .unwrap_or(EscapeAction::Hide)
    {
        EscapeAction::Ignore => false,
        EscapeAction::CancelPreview => {
            content::live_cancel();
            true
        }
        EscapeAction::Hide => {
            hide_popup();
            true
        }
    }
}

fn view(popup: &Popup, cx: &mut Cx<Popup>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let snap = live_snapshot().unwrap_or_else(super::idle_snapshot);
    let preview_open = snap.phase == Phase::OnlinePreview || snap.online_preview.is_some();
    let can_submit = content::can_submit(&popup.query.text, snap.phase);
    let phrase = content::looks_like_phrase(&popup.query.text);
    let body = col()
        .key("translation-popup")
        .gap(12.0)
        .px(16.0)
        .py(12.0)
        .child(
            row()
                .key("translation-popup-header")
                .gap(8.0)
                .center_y()
                .child(
                    rune::text(tr("Translation"))
                        .key("title")
                        .role("heading")
                        .level(1)
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .grow(1.0),
                )
                .child(
                    Button::new(tr("Open in Hex"))
                        .variant(ButtonVariant::Neutral)
                        .on_press(|_: &mut Popup| open_in_hex())
                        .build_push(p.k)
                        .key("translation-open-in-hex"),
                ),
        )
        .child(content::query_field(
            p,
            &popup.query,
            |popup: &mut Popup| &mut popup.query,
            submit,
            |_: &mut Popup, text| content::live_edit(text),
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
    rune_kit::root(
        col()
            .key("window")
            .role("none")
            .fill(p.k.c.bg_base)
            .child(super::super::scroll_body(
                p.k,
                "translation-popup-body",
                body,
            )),
        Vec::new(),
    )
}

fn paste(_popup: &mut Popup, tx: &mut Tx) {
    content::paste_clipboard(tx, |popup: &mut Popup, text| {
        content::set_query(&mut popup.query, text.clone());
        content::live_edit(text);
    });
}

fn clear(popup: &mut Popup, _: &mut Tx) {
    content::set_query(&mut popup.query, String::new());
    content::live_edit(String::new());
    content::live_cancel();
}

fn submit(popup: &mut Popup, _: &mut Tx) {
    let text = popup.query.text.clone();
    if !content::can_submit(
        &text,
        live_snapshot()
            .map(|snap| snap.phase)
            .unwrap_or(Phase::Idle),
    ) {
        return;
    }
    content::submit_local(&text);
    wake_popup();
}

fn recover(_: &mut Popup, recovery: Recovery, tx: &mut Tx) {
    if !content::live_engine() {
        return;
    }
    match recovery {
        Recovery::InstallDictionary => translation::install_dictionary(),
        Recovery::RecheckSystem => translation::recheck_resources(),
        Recovery::OpenSystemLanguageSettings => {
            let _ = tx.open_url("x-apple.systempreferences:com.apple.Localization-Settings");
        }
        Recovery::OpenCompatibleSettings => {
            #[cfg(target_os = "macos")]
            crate::macos_rune::show_output();
            hide_popup();
        }
        Recovery::PrepareOnline => {
            let _ = translation::prepare_online();
        }
    }
}

fn confirm_online(_: &mut Popup, preview: CompatiblePreview, _: &mut Tx) {
    if content::live_engine() {
        translation::confirm_online(preview);
    }
}

fn cancel_preview(_: &mut Popup, _: &mut Tx) {
    content::live_cancel();
}

fn open_in_hex() {
    #[cfg(target_os = "macos")]
    crate::macos_rune::show_translation();
    hide_popup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::Phase;

    #[test]
    fn hide_without_a_window_is_a_no_op() {
        hide_popup();
        assert!(HANDLES.get().is_none());
    }

    #[test]
    fn open_without_the_lab_does_nothing() {
        open_popup();
        assert!(HANDLES.get().is_none());
    }

    #[test]
    fn escape_cancels_preview_before_close() {
        let popup = Popup::default();
        let mut snap = super::super::fixtures::word();
        snap.phase = Phase::OnlinePreview;
        snap.online_preview = Some(CompatiblePreview {
            generation: 1,
            text: "hello".into(),
            base_url: "http://127.0.0.1".into(),
            model: "local".into(),
            host: "127.0.0.1".into(),
        });
        assert!(matches!(
            escape_action(&popup, &snap),
            EscapeAction::CancelPreview
        ));
        snap.phase = Phase::Ready;
        snap.online_preview = None;
        assert!(matches!(escape_action(&popup, &snap), EscapeAction::Hide));
    }

    #[test]
    fn escape_ignores_ime_preedit() {
        let mut popup = Popup::default();
        popup.query.preedit = Some(("ni".into(), None));
        let snap = super::super::fixtures::word();
        assert!(matches!(escape_action(&popup, &snap), EscapeAction::Ignore));
    }
}
