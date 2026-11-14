//! The Mac's retained copies of explicitly queued Mobile transfers.

pub(crate) mod grok;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use color_eyre::eyre::{Result, ensure, eyre};
use serde::{Deserialize, Serialize};

static WRITE: Mutex<()> = Mutex::new(());
#[cfg(target_os = "macos")]
static HYDRATING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static REVISION: AtomicU64 = AtomicU64::new(1);
const FILE_LIMIT: u64 = 100 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 200 * 1024 * 1024;
/// A dataless file is an iCloud placeholder whose bytes are still in the
/// cloud; the local copy cannot import until the file provider materializes
/// it (the `SF_DATALESS` st_flags bit).
#[cfg(target_os = "macos")]
const DATALESS: u32 = 0x40000000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Text,
    Image,
    File,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub name: String,
    pub uti: String,
    pub bytes: u64,
    pub kind: Kind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Held,
    Queued,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub status: Status,
    pub created_at: String,
    pub title: String,
    pub note: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ImportReply {
    pub schema_version: u32,
    pub id: String,
    pub imported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub(crate) fn root() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("transfers"))
}
pub(crate) fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

pub(crate) fn enabled(settings: &crate::app_settings::AppSettings) -> bool {
    let feature = crate::labs::Feature::Transfers;
    feature.on_this_os() && feature.shipped() && feature.managed().is_none() && feature.on(settings)
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn safe_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !["manifest.json", "receipt.json"].contains(&name)
}

fn regular(path: &Path) -> Result<File> {
    let before = fs::symlink_metadata(path)?;
    ensure!(before.is_file(), "attachment is not a regular file");
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        ensure!(
            before.st_flags() & DATALESS == 0,
            "iCloud file is not downloaded"
        );
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        &mut options,
        libc::O_NOFOLLOW | libc::O_NONBLOCK,
    );
    let file = options.open(path)?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "attachment is not a regular file");
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        ensure!(
            meta.st_flags() & DATALESS == 0,
            "iCloud file is not downloaded"
        );
    }
    Ok(file)
}

fn read_manifest(source: &Path) -> Result<Manifest> {
    let file = regular(&source.join("manifest.json"))?;
    ensure!(file.metadata()?.len() <= 64 * 1024, "manifest is too large");
    let manifest: Manifest = serde_json::from_reader(file)?;
    ensure!(manifest.schema_version == 1, "unsupported transfer schema");
    ensure!(valid_id(&manifest.id), "invalid transfer id");
    ensure!(
        !manifest.attachments.is_empty() && manifest.attachments.len() <= 10,
        "invalid attachment count"
    );
    ensure!(
        manifest.title.chars().count() <= 1024 && manifest.note.chars().count() <= 200,
        "transfer metadata is too long"
    );
    let mut names = HashSet::new();
    let mut total = 0;
    for attachment in &manifest.attachments {
        ensure!(
            safe_name(&attachment.name) && names.insert(attachment.name.to_lowercase()),
            "invalid or duplicate attachment name"
        );
        ensure!(attachment.bytes <= FILE_LIMIT, "attachment is too large");
        total += attachment.bytes;
        ensure!(total <= TOTAL_LIMIT, "transfer is too large");
        ensure!(
            attachment.kind != Kind::Text || attachment.name == "body.txt",
            "text must be body.txt"
        );
    }
    Ok(manifest)
}

fn private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)?;
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "transfer directory is not a directory"
    );
    Ok(())
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    Ok(options.open(path)?)
}

pub(crate) fn import(source: &Path) -> Result<ImportReply> {
    import_into(source, &root()?)
}

fn import_into(source: &Path, root: &Path) -> Result<ImportReply> {
    ensure!(source.is_absolute(), "transfer directory must be absolute");
    ensure!(
        fs::symlink_metadata(source)?.is_dir(),
        "transfer source is not a directory"
    );
    let _guard = WRITE
        .lock()
        .map_err(|_| eyre!("transfer store is unavailable"))?;
    let mut manifest = read_manifest(source)?;
    let mut reply = ImportReply {
        schema_version: 1,
        id: manifest.id.clone(),
        imported: false,
        reason: None,
    };
    if manifest.status == Status::Held {
        reply.reason = Some("held".into());
        return Ok(reply);
    }
    manifest.id.make_ascii_lowercase();
    let target = root.join(&manifest.id);
    if target.exists() {
        reply.reason = Some("already".into());
        return Ok(reply);
    }
    let mut files = Vec::new();
    for attachment in &manifest.attachments {
        let file = regular(&source.join(&attachment.name))?;
        ensure!(
            file.metadata()?.len() == attachment.bytes,
            "attachment size does not match"
        );
        files.push(file);
    }
    private_dir(root)?;
    let stage = root.join(format!(".import-{}", manifest.id));
    // A prior interrupted import is never exposed as a completed copy.
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    private_dir(&stage)?;
    let copied = (|| -> Result<()> {
        for (attachment, mut input) in manifest.attachments.iter().zip(files) {
            let mut output = private_file(&stage.join(&attachment.name))?;
            let bytes = std::io::copy(
                &mut Read::by_ref(&mut input).take(attachment.bytes + 1),
                &mut output,
            )?;
            ensure!(
                bytes == attachment.bytes && input.metadata()?.len() == attachment.bytes,
                "attachment changed while copying"
            );
            output.sync_all()?;
        }
        let mut output = private_file(&stage.join("manifest.json"))?;
        output.write_all(&serde_json::to_vec(&manifest)?)?;
        output.sync_all()?;
        #[cfg(unix)]
        File::open(&stage)?.sync_all()?;
        fs::rename(&stage, &target)?;
        #[cfg(unix)]
        File::open(root)?.sync_all()?;
        Ok(())
    })();
    if copied.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    copied?;
    reply.imported = true;
    REVISION.fetch_add(1, Ordering::Release);
    Ok(reply)
}

pub(crate) fn list() -> Result<Vec<Manifest>> {
    list_in(&root()?)
}
fn list_in(root: &Path) -> Result<Vec<Manifest>> {
    let directories = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut entries = Vec::new();
    for entry in directories.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && let Ok(manifest) = read_manifest(&entry.path())
            && entry.file_name().to_str() == Some(&manifest.id)
        {
            entries.push(manifest);
        }
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Ok(entries)
}

/// The manifest records UTC (`…Z`); the pane shows the moment in this
/// computer's wall clock. Like `screenshots/save.rs`, this is one small
/// per-module adapter over the C `localtime` functions, with no time crate.
pub(crate) fn local_time(created_at: &str) -> String {
    let Some(seconds) = utc_seconds(created_at).filter(|seconds| *seconds >= 0) else {
        return created_at.to_string();
    };
    let seconds = seconds as libc::time_t;
    // SAFETY: `localtime_r` and `localtime_s` write only into the provided
    // zeroed `tm`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    #[cfg(unix)]
    unsafe {
        libc::localtime_r(&seconds, &mut tm)
    };
    #[cfg(windows)]
    unsafe {
        libc::localtime_s(&mut tm, &seconds)
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1_900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// The UTC instant in a manifest timestamp, seconds since the epoch. The
/// schema's RFC 3339 form is `YYYY-MM-DDTHH:MM:SS(.…)?(Z|±HH:MM)`; anything
/// else parses as nothing and the pane passes the raw string through.
fn utc_seconds(created_at: &str) -> Option<i64> {
    let bytes = created_at.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        let slice = bytes.get(range)?;
        if slice.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(slice).ok()?.parse::<i64>().ok()
        } else {
            None
        }
    };
    let year = digits(0..4)?;
    if bytes.get(4) != Some(&b'-') {
        return None;
    }
    let month = digits(5..7)?;
    if bytes.get(7) != Some(&b'-') {
        return None;
    }
    let day = digits(8..10)?;
    if !matches!(bytes.get(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let hour = digits(11..13)?;
    if bytes.get(13) != Some(&b':') {
        return None;
    }
    let minute = digits(14..16)?;
    if bytes.get(16) != Some(&b':') {
        return None;
    }
    let second = digits(17..19)?;
    let mut rest = &bytes[19..];
    if rest.first() == Some(&b'.') {
        let end = rest[1..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .map_or(rest.len(), |end| end + 1);
        rest = &rest[end..];
    }
    let offset = match rest {
        [b'Z' | b'z'] | [] => 0,
        [sign @ (b'-' | b'+'), h1, h2, b':' | b' ', m1, m2] => {
            if !(h1.is_ascii_digit()
                && h2.is_ascii_digit()
                && m1.is_ascii_digit()
                && m2.is_ascii_digit())
            {
                return None;
            }
            let magnitude = i64::from((h1 - b'0') * 10 + (h2 - b'0')) * 3_600
                + i64::from((m1 - b'0') * 10 + (m2 - b'0')) * 60;
            if *sign == b'-' { -magnitude } else { magnitude }
        }
        _ => return None,
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub(crate) fn delete(id: &str) -> Result<()> {
    delete_in(&root()?, id)
}
fn delete_in(root: &Path, id: &str) -> Result<()> {
    ensure!(valid_id(id), "invalid transfer id");
    let _guard = WRITE
        .lock()
        .map_err(|_| eyre!("transfer store is unavailable"))?;
    fs::remove_dir_all(root.join(id.to_ascii_lowercase()))?;
    REVISION.fetch_add(1, Ordering::Release);
    Ok(())
}

pub(crate) fn scan_default() {
    if crate::app_settings::AppSettings::read().is_ok_and(|settings| enabled(&settings))
        && let Some(home) = crate::app_paths::home_dir()
        && let Ok(store) = root()
    {
        scan_in(
            &home.join("Library/Mobile Documents/com~apple~CloudDocs/Hex Mobile/transfers"),
            &store,
        );
    }
}

fn scan_in(sync_root: &Path, store: &Path) {
    let Ok(entries) = fs::read_dir(sync_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || path.join("receipt.json").exists()
        {
            continue;
        }
        match import_into(&path, store) {
            #[cfg(target_os = "macos")]
            Ok(reply) if reply.imported => macos::notify_if_hidden(),
            Ok(_) => (),
            Err(_) => {
                tracing::debug!("transfer skipped: unavailable or invalid");
                // A dataless attachment is the one recoverable refusal: ask
                // the file provider for the bytes and import when they land.
                #[cfg(target_os = "macos")]
                if !dataless_files(&path).is_empty() {
                    hydrate(&path, store);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn dataless(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.st_flags() & DATALESS != 0)
}

#[cfg(target_os = "macos")]
fn dataless_files(source: &Path) -> Vec<PathBuf> {
    fs::read_dir(source)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| dataless(path))
                .collect()
        })
        .unwrap_or_default()
}

/// One download attempt at a time per transfer folder, so repeated scans
/// while a file downloads never stack hydration threads.
#[cfg(target_os = "macos")]
fn hydrate(source: &Path, store: &Path) {
    let key = source.to_path_buf();
    let Ok(mut hydrating) = HYDRATING.lock() else {
        return;
    };
    if hydrating.contains(&key) {
        return;
    }
    let started = std::thread::Builder::new()
        .name("hex-transfers-hydrate".into())
        .spawn({
            let key = key.clone();
            let store = store.to_path_buf();
            move || {
                for file in dataless_files(&key) {
                    materialize(&file);
                }
                if let Ok(mut hydrating) = HYDRATING.lock() {
                    hydrating.retain(|path| path != &key);
                }
                match import_into(&key, &store) {
                    Ok(reply) if reply.imported => macos::notify_if_hidden(),
                    Ok(_) => (),
                    Err(_) => tracing::debug!("transfer skipped: unavailable or invalid"),
                }
            }
        });
    if started.is_err() {
        hydrating.retain(|path| path != &key);
    }
}

/// Reading the first and last byte asks the file provider to fetch the whole
/// file; without a read, "Optimize Mac Storage" may keep the placeholder on
/// disk forever. The read may block for the whole download, so it runs only
/// on the hydration thread.
#[cfg(target_os = "macos")]
fn materialize(path: &Path) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    let end = match file.metadata() {
        Ok(meta) => meta.len().saturating_sub(1),
        Err(_) => return,
    };
    if file.seek(std::io::SeekFrom::Start(end)).is_err() {
        return;
    }
    let _ = file.read_exact(&mut [0; 1]);
    let _ = file.seek(std::io::SeekFrom::Start(0));
    let _ = file.read_exact(&mut [0; 1]);
}
