//! The Mac's retained copies of explicitly queued Mobile transfers.

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use color_eyre::eyre::{Result, ensure, eyre};
use serde::{Deserialize, Serialize};

static WRITE: Mutex<()> = Mutex::new(());
static REVISION: AtomicU64 = AtomicU64::new(1);
const FILE_LIMIT: u64 = 100 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 200 * 1024 * 1024;

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
            before.st_flags() & 0x40000000 == 0,
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
            meta.st_flags() & 0x40000000 == 0,
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
        && let Ok(entries) = fs::read_dir(
            home.join("Library/Mobile Documents/com~apple~CloudDocs/Hex Mobile/transfers"),
        )
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir())
                || path.join("receipt.json").exists()
            {
                continue;
            }
            match import(&path) {
                #[cfg(target_os = "macos")]
                Ok(reply) if reply.imported => macos::notify_if_hidden(),
                Ok(_) => (),
                Err(_) => tracing::debug!("transfer skipped: unavailable or invalid"),
            }
        }
    }
}
