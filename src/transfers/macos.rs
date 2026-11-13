use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use color_eyre::eyre::{Result, ensure, eyre};
use objc2::AllocAnyThread;
use objc2_app_kit::{NSImage, NSPasteboard, NSPasteboardTypeTIFF};
use objc2_foundation::NSString;

static WINDOW: OnceLock<rune::AppHandle<crate::rune_ui::Ui>> = OnceLock::new();
static PENDING: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn install(window: rune::AppHandle<crate::rune_ui::Ui>) {
    let _ = WINDOW.set(window);
    let count = PENDING.swap(0, Ordering::AcqRel);
    for _ in 0..count {
        notify_if_hidden();
    }
}

pub(crate) fn notify_if_hidden() {
    if let Some(window) = WINDOW.get() {
        window.update(crate::rune_ui::transfer_imported);
    } else {
        PENDING.fetch_add(1, Ordering::Release);
        // The window may have been installed between the check and increment.
        if let Some(window) = WINDOW.get() {
            let count = PENDING.swap(0, Ordering::AcqRel);
            for _ in 0..count {
                window.update(crate::rune_ui::transfer_imported);
            }
        }
    }
}

#[allow(deprecated)]
pub(crate) fn notify() {
    use crate::desktop::i18n::tr;
    use objc2_foundation::{NSUserNotification, NSUserNotificationCenter};
    let notification = NSUserNotification::new();
    notification.setTitle(Some(&NSString::from_str(tr("Transfers"))));
    notification.setInformativeText(Some(&NSString::from_str(tr(
        "A transfer arrived from Hex Mobile.",
    ))));
    NSUserNotificationCenter::defaultUserNotificationCenter().deliverNotification(&notification);
}

pub(crate) fn reveal(path: &Path) -> Result<()> {
    let status = crate::desktop::background_process::command("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .status()?;
    ensure!(status.success(), "could not reveal the attachment");
    Ok(())
}

pub(crate) fn copy_image(path: &Path) -> Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| eyre!("invalid attachment path"))?;
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path))
        .ok_or_else(|| eyre!("could not decode the image"))?;
    let data = image
        .TIFFRepresentation()
        .ok_or_else(|| eyre!("could not copy the image"))?;
    let pasteboard = NSPasteboard::generalPasteboard();
    // SAFETY: this type is an immutable AppKit string constant.
    let kind = unsafe { NSPasteboardTypeTIFF };
    pasteboard.clearContents();
    ensure!(
        pasteboard.setData_forType(Some(&data), kind),
        "could not copy the image"
    );
    Ok(())
}
