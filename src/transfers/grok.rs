//! Atomic, local handoff to the running Grok Bot Anywhere ShareInbox.
// No SDK, process launch, credentials, or payload diagnostics belong here.

use super::*;

pub(crate) const NOT_CONFIGURED: &str = "Grok Bot Anywhere has no receiving bot turned on.";
pub(crate) const HANDOFF_FAILED: &str = "The Grok Bot Anywhere handoff failed.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    NotConfigured,
    HandoffFailed,
}
impl Error {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::NotConfigured => NOT_CONFIGURED,
            Self::HandoffFailed => HANDOFF_FAILED,
        }
    }
}

// Deliberately no Debug and no avatar/token fields. Unknown JSON fields are ignored.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Bot {
    account_id: String,
    bot_id: String,
    pub(crate) bot_name: String,
    #[serde(default)]
    session_id: Option<String>,
    mode: String,
    prefix: String,
}

#[cfg(target_os = "macos")]
fn inbox() -> std::result::Result<PathBuf, Error> {
    crate::app_paths::home_dir()
        .map(|home| home.join("Library/Application Support/Grok Bot Anywhere/ShareInbox"))
        .ok_or(Error::NotConfigured)
}

pub(crate) fn bots() -> std::result::Result<Vec<Bot>, Error> {
    #[cfg(target_os = "macos")]
    return bots_in(&inbox()?);
    #[cfg(not(target_os = "macos"))]
    Err(Error::NotConfigured)
}

pub(crate) fn send(id: &str, bot: &Bot) -> std::result::Result<(), Error> {
    #[cfg(target_os = "macos")]
    {
        if !valid_id(id) {
            return Err(Error::HandoffFailed);
        }
        let root = root().map_err(|_| Error::HandoffFailed)?;
        send_in(&root.join(id), &inbox()?, bot).map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (id, bot);
        Err(Error::NotConfigured)
    }
}

#[cfg(any(target_os = "macos", test))]
const TEXT_LIMIT: usize = 2_000_000;

#[cfg(any(target_os = "macos", test))]
fn directory_without_links(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        ensure!(fs::symlink_metadata(ancestor)?.is_dir(), "not a directory");
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn bots_in(inbox: &Path) -> std::result::Result<Vec<Bot>, Error> {
    #[derive(Deserialize)]
    struct Configuration {
        version: u32,
        entries: Vec<Bot>,
    }
    let read = || -> Result<Vec<Bot>> {
        directory_without_links(inbox)?;
        let file = regular(&inbox.join("configuration.json"))?;
        ensure!(
            file.metadata()?.len() <= 16 * 1024 * 1024,
            "configuration too large"
        );
        let configuration: Configuration = serde_json::from_reader(file)?;
        ensure!(
            configuration.version == 1 && !configuration.entries.is_empty(),
            "no bots"
        );
        for bot in &configuration.entries {
            ensure!(
                matches!(bot.mode.as_str(), "compose" | "direct"),
                "invalid mode"
            );
            ensure!(
                bot.prefix.encode_utf16().count() <= 10_000,
                "prefix too long"
            );
        }
        Ok(configuration.entries)
    };
    read().map_err(|_| Error::NotConfigured)
}

// The receiver uses JavaScript string limits; count UTF-16 units so emoji are safe too.
#[cfg(any(target_os = "macos", test))]
fn truncate(text: &str, limit: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= limit
        })
        .collect()
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn send_in(
    source: &Path,
    inbox: &Path,
    selected: &Bot,
) -> std::result::Result<PathBuf, Error> {
    // Refresh only on a send action; never send to a removed or changed entry.
    let bot = bots_in(inbox)?
        .into_iter()
        .find(|bot| bot == selected)
        .ok_or(Error::NotConfigured)?;
    let handoff = || -> Result<PathBuf> {
        directory_without_links(source)?;
        let manifest = read_manifest(source)?;
        let mut text = manifest.note.clone();
        let mut attachments = Vec::new();
        for attachment in &manifest.attachments {
            let mut input = regular(&source.join(&attachment.name))?;
            ensure!(input.metadata()?.len() == attachment.bytes, "size changed");
            if attachment.name == "body.txt" {
                let mut body = String::new();
                Read::by_ref(&mut input)
                    .take(FILE_LIMIT + 1)
                    .read_to_string(&mut body)?;
                ensure!(body.len() as u64 == attachment.bytes, "size changed");
                if !body.is_empty() {
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(&body);
                }
            } else {
                attachments.push((attachment, input));
            }
        }
        if text.is_empty() {
            text = manifest.title.clone();
        }
        ensure!(attachments.len() <= 100, "too many files");
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| eyre!("handoff failed"))?;
        let id = format!(
            "hex-{}",
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let stage = inbox.join(format!(".hex-{id}"));
        // create_new semantics: never reuse or remove somebody else's staging directory.
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&stage)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&stage)?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(stage.clone());
        let mut files = Vec::new();
        for (attachment, mut input) in attachments {
            let mut output = private_file(&stage.join(&attachment.name))?;
            let copied = std::io::copy(
                &mut Read::by_ref(&mut input).take(attachment.bytes + 1),
                &mut output,
            )?;
            ensure!(
                copied == attachment.bytes && input.metadata()?.len() == attachment.bytes,
                "size changed"
            );
            output.sync_all()?;
            files.push(
                serde_json::json!({"relativePath": attachment.name, "name": attachment.name}),
            );
        }
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let payload = serde_json::json!({
            "version": 1, "id": id, "source": "wechat",
            "accountId": bot.account_id, "botId": bot.bot_id,
            "sessionId": bot.session_id, "mode": bot.mode, "prefix": bot.prefix,
            "text": truncate(&text, TEXT_LIMIT), "title": truncate(&manifest.title, 1000),
            "createdAt": u64::try_from(created_at)?, "files": files,
        });
        let mut output = private_file(&stage.join("manifest.json"))?;
        output.write_all(&serde_json::to_vec(&payload)?)?;
        output.sync_all()?;
        #[cfg(unix)]
        File::open(&stage)?.sync_all()?;
        let ready = inbox.join("Ready");
        let created_ready = match fs::create_dir(&ready) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        let target = ready.join(&id);
        let publish = || -> Result<()> {
            directory_without_links(&ready)?;
            fs::rename(&stage, &target)?;
            Ok(())
        };
        if let Err(error) = publish() {
            if created_ready {
                let _ = fs::remove_dir(&ready);
            }
            return Err(error);
        }
        Ok(target)
    };
    handoff().map_err(|_| Error::HandoffFailed)
}
