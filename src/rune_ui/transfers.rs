//! The Mac's local transfer inbox, using History's list-and-detail pieces.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use super::controls::{self, ButtonKind, Paint};
use super::list_detail::{self as ld, Item};
use super::{Pane, Ui};
use crate::desktop::i18n::{tr, tr_fill};
use crate::transfers::{self, Kind, Manifest};
use rune::{El, TextEdit, Tx, col, el, text};
use rune_kit::TextInput;

#[derive(Default)]
pub(super) struct TransfersPane {
    entries: Vec<Manifest>,
    selected: Option<String>,
    search: TextEdit,
    revision: u64,
    body: Option<String>,
    error: Option<&'static str>,
    delete_armed: bool,
    pub(super) cache: Option<rune::ImageCache>,
    images: RefCell<HashMap<PathBuf, rune::Image>>,
    #[cfg(target_os = "macos")]
    scanned: bool,
    #[cfg(target_os = "macos")]
    grok_bots: Vec<transfers::grok::Bot>,
    #[cfg(target_os = "macos")]
    grok_worker: Option<(String, std::sync::mpsc::Receiver<GrokReply>)>,
    #[cfg(target_os = "macos")]
    grok_notice: Option<&'static str>,
}

#[cfg(target_os = "macos")]
type GrokReply = Result<Vec<transfers::grok::Bot>, transfers::grok::Error>;

pub(super) fn poll(ui: &mut Ui) -> bool {
    #[cfg(target_os = "macos")]
    let changed = poll_grok(ui);
    #[cfg(not(target_os = "macos"))]
    let changed = false;
    // Opening the pane rescans the sync folder, so a transfer queued after
    // launch is not lost to the startup scan.
    #[cfg(target_os = "macos")]
    {
        let visible = ui.live() && ui.pane == Pane::Transfers && !ui.labs_open;
        if visible && !ui.transfers.scanned {
            ui.transfers.scanned = true;
            let _ = std::thread::Builder::new()
                .name("hex-transfers-scan".into())
                .spawn(crate::transfers::scan_default);
        } else if !visible {
            ui.transfers.scanned = false;
        }
    }
    if !ui.live() || ui.pane != Pane::Transfers || ui.labs_open {
        return changed;
    }
    if ui.transfers.revision == transfers::revision() {
        return changed;
    }
    match transfers::list() {
        Ok(entries) => {
            ui.transfers.entries = entries;
            ui.transfers.error = None;
            if !ui
                .transfers
                .entries
                .iter()
                .any(|entry| Some(&entry.id) == ui.transfers.selected.as_ref())
            {
                let id = ui.transfers.entries.first().map(|entry| entry.id.clone());
                select(ui, id);
            }
        }
        Err(_) => ui.transfers.error = Some("Could not read transfers."),
    }
    ui.transfers.revision = transfers::revision();
    true
}

pub(crate) fn imported(ui: &mut Ui, _tx: &mut Tx) {
    #[cfg(target_os = "macos")]
    if !(ui.pane == Pane::Transfers
        && !ui.labs_open
        && ui.tour.is_none()
        && ui.window.as_ref().is_some_and(rune::AppHandle::is_visible))
    {
        transfers::macos::notify();
    }
    poll(ui);
}

fn select(ui: &mut Ui, id: Option<String>) {
    ui.transfers.selected = id;
    ui.transfers.delete_armed = false;
    ui.transfers.body = None;
    #[cfg(target_os = "macos")]
    {
        ui.transfers.grok_bots.clear();
        ui.transfers.grok_notice = None;
    }
    if let Some(entry) = ui
        .transfers
        .entries
        .iter()
        .find(|entry| Some(&entry.id) == ui.transfers.selected.as_ref())
        && entry
            .attachments
            .iter()
            .any(|attachment| attachment.kind == Kind::Text)
        && let Ok(root) = transfers::root()
    {
        match std::fs::read_to_string(root.join(&entry.id).join("body.txt")) {
            Ok(body) => ui.transfers.body = Some(body),
            Err(_) => ui.transfers.error = Some("Could not read the attachment."),
        }
    }
}

fn kind(kind: &Kind) -> &'static str {
    match kind {
        Kind::Text => tr("Text"),
        Kind::Image => tr("Image"),
        Kind::File => tr("File"),
    }
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let pane = &ui.transfers;
    let notice = pane
        .error
        .map(|error| controls::note("transfers-error", tr(error), p.muted()));
    let mut list = col().key("transfers-list").label(tr("Transfers"));
    let query = pane.search.text.to_lowercase();
    for entry in pane
        .entries
        .iter()
        .filter(|entry| entry.title.to_lowercase().contains(&query))
    {
        let id = entry.id.clone();
        let mut kinds = Vec::new();
        for attachment in &entry.attachments {
            let label = kind(&attachment.kind);
            if !kinds.contains(&label) {
                kinds.push(label);
            }
        }
        let mut meta = kinds.join(" · ");
        if entry.attachments.len() > 1 {
            meta.push_str(" · ");
            meta.push_str(&tr_fill(
                "{} attachments",
                &entry.attachments.len().to_string(),
            ));
        }
        list = list.child(ld::item_with_icon(
            p,
            Item {
                key: format!("transfer-{}", entry.id),
                title: entry.title.clone(),
                meta,
                meta_ink: None,
                time: {
                    let stamp = transfers::local_time(&entry.created_at);
                    stamp.get(5..16).unwrap_or(&stamp).replace('T', " ")
                },
                hover: Vec::new(),
                selected: pane.selected.as_ref() == Some(&id),
            },
            None,
            move |ui| select(ui, Some(id.clone())),
        ));
    }
    let body = if pane.entries.is_empty() {
        ld::empty_state(
            p,
            "transfers-empty",
            ld::HISTORY_ICON,
            tr("No transfers yet."),
            None,
        )
    } else {
        let search = TextInput::new(&pane.search)
            .placeholder(tr("Search transfers"))
            .width(None)
            .leading_icon(p.k.glyph.search)
            .on_edit(|ui: &mut Ui| &mut ui.transfers.search)
            .build(p.k)
            .key("transfers-search");
        ld::body(
            p,
            "transfers-body",
            ld::list_column(p, search, "transfers-scroll", list),
            detail(ui, p),
        )
    };
    col()
        .key("transfers-pane")
        .grow(1.0)
        .shrink()
        .children(notice)
        .child(body)
        .px(controls::METRICS.gutter - ld::HANG)
}

fn detail(ui: &Ui, p: Paint) -> El {
    let pane = &ui.transfers;
    let Some(entry) = pane
        .entries
        .iter()
        .find(|entry| Some(&entry.id) == pane.selected.as_ref())
    else {
        return ld::empty_state(
            p,
            "transfers-selection",
            ld::HISTORY_ICON,
            tr("Transfers"),
            None,
        );
    };
    let id = entry.id.clone();
    let actions = vec![ld::ghost_icon(
        p,
        "transfer-delete",
        ld::TRASH_ICON,
        tr("Delete…"),
        |ui: &mut Ui, _| ui.transfers.delete_armed = true,
    )];
    let mut body = col().key("transfer-content").gap(16.0);
    #[cfg(target_os = "macos")]
    {
        let mut send = col().key("transfer-grok").gap(8.0);
        if pane.grok_worker.is_some() {
            send = send.child(controls::note(
                "grok-busy",
                tr("Sending to Grok Bot…"),
                p.muted(),
            ));
        } else if pane.grok_bots.len() > 1 {
            for (index, bot) in pane.grok_bots.iter().enumerate() {
                let bot = bot.clone();
                let label = bot.bot_name.clone();
                send = send.child(controls::button(
                    p,
                    &format!("transfer-grok-{index}"),
                    &label,
                    ButtonKind::Secondary,
                    move |ui: &mut Ui, _| send_grok(ui, Some(bot.clone())),
                ));
            }
        } else {
            send = send.child(controls::button(
                p,
                "transfer-send-grok",
                tr("Send to Grok Bot"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| send_grok(ui, None),
            ));
            if let Some(bot) = pane.grok_bots.first() {
                send = send.child(
                    text(&bot.bot_name)
                        .key("grok-bot-name")
                        .font(p.k.ty.body)
                        .color(p.muted())
                        .wrap(),
                );
            }
        }
        if let Some(notice) = pane.grok_notice {
            send = send.child(controls::note("grok-notice", tr(notice), p.muted()));
        }
        body = body.child(send);
    }
    if !entry.note.is_empty() {
        body = body.child(
            text(&entry.note)
                .key("transfer-note")
                .font(p.k.ty.body)
                .color(p.muted())
                .wrap()
                .selectable(true),
        );
    }
    if pane.delete_armed {
        body = body
            .child(controls::button(
                p,
                "transfer-confirm-delete",
                tr("Confirm delete"),
                ButtonKind::Danger,
                move |ui: &mut Ui, _| match transfers::delete(&id) {
                    Ok(()) => {
                        ui.transfers.revision = 0;
                        poll(ui);
                    }
                    Err(_) => ui.transfers.error = Some("Could not delete the local transfer."),
                },
            ))
            .child(controls::button(
                p,
                "transfer-cancel-delete",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.transfers.delete_armed = false,
            ));
    }
    for (index, attachment) in entry.attachments.iter().enumerate() {
        let Ok(root) = transfers::root() else {
            continue;
        };
        let path = root.join(&entry.id).join(&attachment.name);
        let mut section = col().key(format!("attachment-{index}")).gap(8.0).child(
            text(&attachment.name)
                .key("name")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap(),
        );
        match attachment.kind {
            Kind::Text => {
                if let Some(body) = &pane.body {
                    section = section
                        .child(
                            text(body)
                                .key("text")
                                .font(p.k.ty.body)
                                .color(p.text())
                                .wrap()
                                .selectable(true),
                        )
                        .child(ld::ghost_icon(
                            p,
                            "transfer-copy-text",
                            ld::COPY_ICON,
                            tr("Copy"),
                            |ui: &mut Ui, tx| {
                                if let Some(body) = &ui.transfers.body {
                                    tx.copy(body.clone());
                                }
                            },
                        ));
                }
            }
            Kind::Image => {
                if let Some(cache) = &pane.cache {
                    let mut images = pane.images.borrow_mut();
                    if images.len() > 20 {
                        images.clear();
                    }
                    let image = images
                        .entry(path.clone())
                        .or_insert_with(|| cache.file(&path, 440.0, 2.0));
                    if image.is_ready() && image.error().is_none() {
                        section = section.child(
                            el().key("preview")
                                .h(260.0)
                                .w_full()
                                .label(&attachment.name)
                                .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
                        );
                    } else if image.error().is_some() {
                        section = section.child(
                            text(tr("Preview unavailable"))
                                .key("preview-unavailable")
                                .font(p.k.ty.body)
                                .color(p.muted()),
                        );
                    }
                }
                let copy_path = path.clone();
                section = section.child(ld::ghost_icon(
                    p,
                    "transfer-copy-image",
                    ld::COPY_ICON,
                    tr("Copy image"),
                    move |ui: &mut Ui, _| {
                        copy_image(ui, &copy_path);
                    },
                ));
            }
            Kind::File => (),
        }
        if attachment.kind != Kind::Text {
            section = section.child(ld::ghost_icon(
                p,
                "transfer-reveal",
                &rune_theme_opencode::icons::FOLDER,
                tr("Show in Finder"),
                move |ui: &mut Ui, _| {
                    reveal(ui, &path);
                },
            ));
        }
        body = body.child(section);
    }
    col()
        .key("transfer-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, &entry.title),
            vec![ld::meta_text(p, &transfers::local_time(&entry.created_at))],
            actions,
        ))
        .child(ld::card_scroll(p, "transfer-detail-scroll", body))
}

#[cfg(target_os = "macos")]
fn send_grok(ui: &mut Ui, selected: Option<transfers::grok::Bot>) {
    if !ui.live() || ui.transfers.grok_worker.is_some() {
        return;
    }
    let Some(id) = ui.transfers.selected.clone() else {
        return;
    };
    ui.transfers.error = None;
    ui.transfers.grok_notice = None;
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker_id = id.clone();
    let wake = ui.modes.waker.clone();
    let started = std::thread::Builder::new()
        .name("hex-transfer-grok".into())
        .spawn(move || {
            let reply = (|| {
                if let Some(bot) = selected {
                    transfers::grok::send(&worker_id, &bot)?;
                    Ok(Vec::new())
                } else {
                    let bots = transfers::grok::bots()?;
                    if let [bot] = bots.as_slice() {
                        transfers::grok::send(&worker_id, bot)?;
                        // Keep the one recipient's name beside the one send button.
                        Ok(bots)
                    } else {
                        Ok(bots)
                    }
                }
            })();
            let _ = sender.send(reply);
            if let Some(wake) = wake {
                wake();
            }
        });
    if started.is_ok() {
        ui.transfers.grok_worker = Some((id, receiver));
    } else {
        ui.transfers.error = Some(transfers::grok::HANDOFF_FAILED);
    }
}

#[cfg(target_os = "macos")]
fn poll_grok(ui: &mut Ui) -> bool {
    use std::sync::mpsc::TryRecvError;
    let reply = match ui
        .transfers
        .grok_worker
        .as_ref()
        .map(|(_, rx)| rx.try_recv())
    {
        Some(Ok(reply)) => reply,
        Some(Err(TryRecvError::Disconnected)) => Err(transfers::grok::Error::HandoffFailed),
        _ => return false,
    };
    let (id, _) = ui.transfers.grok_worker.take().unwrap();
    if ui.transfers.selected.as_ref() == Some(&id) {
        match reply {
            Ok(bots) => {
                if bots.len() <= 1 {
                    ui.transfers.grok_notice = Some("Sent to Grok Bot Anywhere.");
                }
                if !bots.is_empty() {
                    ui.transfers.grok_bots = bots;
                }
            }
            Err(error) => ui.transfers.error = Some(error.message()),
        }
    }
    true
}

fn copy_image(ui: &mut Ui, path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    if transfers::macos::copy_image(path).is_err() {
        ui.transfers.error = Some("Could not copy the image.");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (ui, path);
}
fn reveal(ui: &mut Ui, path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    if transfers::macos::reveal(path).is_err() {
        ui.transfers.error = Some("Could not reveal the attachment.");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (ui, path);
}

#[cfg(test)]
mod tests {
    use super::super::{Service, platform};
    use super::*;

    #[test]
    fn transfers_sidebar_is_opt_in_mac_only_and_after_history() {
        let mut ui = Ui::with(
            crate::app_settings::AppSettings::default(),
            Service::Fixture(Vec::new()),
        );
        for caps in [
            platform::Capabilities::MACOS,
            platform::Capabilities::LINUX,
            platform::Capabilities::WINDOWS,
        ] {
            ui.platform = platform::PlatformState::preview(caps);
            ui.settings.labs.transfers = false;
            assert!(!Pane::Transfers.shown(&ui));
            ui.settings.labs.transfers = true;
            let panes = super::super::labs::main_panes(&ui);
            if caps.os == platform::Os::Macos && cfg!(target_os = "macos") {
                let index = panes.iter().position(|pane| *pane == Pane::History);
                assert!(index.is_some_and(|index| panes.get(index + 1) == Some(&Pane::Transfers)));
            } else {
                assert!(!panes.contains(&Pane::Transfers));
            }
        }
    }
}
