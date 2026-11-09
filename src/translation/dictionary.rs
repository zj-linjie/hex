use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
use color_eyre::eyre::eyre;
use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{DictionaryStatus, InstallProgress, InstallStage};

pub const GIT: &str = "39cfb9672c0906b63569e0453c2e6abfb9ab182a";
pub const CSV_BYTES: u64 = 66_009_548;
pub const CSV_SHA256: &str = "d1ecbdf9ee2b920afb6b430ca6b6bb9f559588cf0dff86e04bd255f9390843cc";
pub const LEMMA_BYTES: u64 = 2_318_694;
pub const LEMMA_SHA256: &str = "e255b097404e3e0052060e2ddf6e15a1414f577071d63d51d2ca0ce9dacee0fc";
pub const DOWNLOAD_BYTES: u64 = CSV_BYTES + LEMMA_BYTES;

const CSV_URL: &str = "https://raw.githubusercontent.com/skywind3000/ECDICT/39cfb9672c0906b63569e0453c2e6abfb9ab182a/ecdict.csv";
const LEMMA_URL: &str = "https://raw.githubusercontent.com/skywind3000/ECDICT/39cfb9672c0906b63569e0453c2e6abfb9ab182a/lemma.en.txt";

const DICT_NAME: &str = "dictionary.tsv";
const LEMMA_NAME: &str = "lemma.tsv";
const RECEIPT_NAME: &str = "receipt.json";
const CURRENT_NAME: &str = "CURRENT";

struct PinState {
    counts: HashMap<(PathBuf, String), u32>,
}

fn pin_table() -> std::sync::MutexGuard<'static, PinState> {
    static PINS: OnceLock<Mutex<PinState>> = OnceLock::new();
    PINS.get_or_init(|| {
        Mutex::new(PinState {
            counts: HashMap::new(),
        })
    })
    .lock()
    .unwrap_or_else(|error| error.into_inner())
}

fn commit_lock() -> std::sync::MutexGuard<'static, ()> {
    static COMMIT: Mutex<()> = Mutex::new(());
    COMMIT.lock().unwrap_or_else(|error| error.into_inner())
}

struct BundlePin {
    root: PathBuf,
    name: String,
}

impl Drop for BundlePin {
    fn drop(&mut self) {
        drop_pin(&self.root, &self.name);
    }
}

pub(super) struct LoadedDictionary {
    pub key: String,
    pub dictionary: Dictionary,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub headword: Box<str>,
    pub phonetic: Option<Box<str>>,
    pub translation: Box<str>,
    pub pos: String,
}

#[derive(Clone, Debug)]
pub struct Dictionary {
    entries: HashMap<Box<str>, Entry>,
    lemmas: HashMap<Box<str>, Box<str>>,
}

impl Dictionary {
    pub fn lookup(&self, key: &str) -> Option<Entry> {
        self.entries.get(key).cloned()
    }

    pub fn lemma(&self, key: &str) -> Option<String> {
        self.lemmas.get(key).map(|lemma| lemma.to_string())
    }

    #[cfg(test)]
    pub fn from_parts(
        entries: impl IntoIterator<Item = (&'static str, Entry)>,
        lemmas: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(key, entry)| (Box::from(key), entry))
                .collect(),
            lemmas: lemmas
                .into_iter()
                .map(|(form, lemma)| (Box::from(form), Box::from(lemma)))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Receipt {
    git: String,
    csv_sha256: String,
    csv_bytes: u64,
    lemma_sha256: String,
    lemma_bytes: u64,
    entries: u64,
    dictionary_sha256: String,
    dictionary_bytes: u64,
    lemma_tsv_sha256: String,
    lemma_tsv_bytes: u64,
}

pub fn status_on_disk() -> DictionaryStatus {
    let Ok(dir) = directory() else {
        return DictionaryStatus::Missing {
            bytes: DOWNLOAD_BYTES,
        };
    };
    status_in(&dir)
}

pub(super) fn status_in(dir: &Path) -> DictionaryStatus {
    match committed_receipt(dir) {
        Ok(Some(receipt)) => DictionaryStatus::Ready {
            entries: receipt.entries,
        },
        Ok(None) => DictionaryStatus::Missing {
            bytes: DOWNLOAD_BYTES,
        },
        Err(error) => DictionaryStatus::Failed {
            message: error.to_string(),
        },
    }
}

#[allow(dead_code)]
pub fn load() -> Result<Option<Dictionary>> {
    load_from(&directory()?)
}

pub fn load_from(dir: &Path) -> Result<Option<Dictionary>> {
    Ok(load_keyed_from(dir)?.map(|loaded| loaded.dictionary))
}

pub(super) fn load_keyed() -> Result<Option<LoadedDictionary>> {
    load_keyed_from(&directory()?)
}

pub(super) fn load_keyed_from(dir: &Path) -> Result<Option<LoadedDictionary>> {
    let Some(pin) = pin_current(dir)? else {
        return Ok(None);
    };
    #[cfg(test)]
    invoke_after_pin();
    let bundle = dir.join(&pin.name);
    let loaded = (|| {
        if !bundle.is_dir() {
            return Ok(None);
        }
        let Some(receipt) = load_receipt(&bundle)? else {
            return Ok(None);
        };
        if !receipt_matches(&receipt) {
            return Ok(None);
        }
        verify_bundle_files(&bundle, &receipt, &AtomicBool::new(false))?;
        let entries = load_entries(&bundle.join(DICT_NAME))?;
        let lemmas = load_lemmas(&bundle.join(LEMMA_NAME))?;
        Ok(Some(LoadedDictionary {
            key: pin.name.clone(),
            dictionary: Dictionary { entries, lemmas },
        }))
    })();
    drop(pin);
    loaded
}

pub(super) fn current_bundle_key(dir: &Path) -> Result<Option<String>> {
    committed_name(dir)
}

pub fn current_key() -> Result<Option<String>> {
    current_bundle_key(&directory()?)
}

pub fn install(
    progress: impl FnMut(InstallProgress),
    canceled: &AtomicBool,
) -> Result<DictionaryStatus> {
    install_into(&directory()?, progress, canceled)
}

pub fn install_into(
    dir: &Path,
    mut progress: impl FnMut(InstallProgress),
    canceled: &AtomicBool,
) -> Result<DictionaryStatus> {
    crate::app_paths::create_private_dir(dir)?;
    let staging = unique_staging(dir);
    crate::app_paths::create_private_dir(&staging)?;
    let result = (|| {
        let csv = staging.join("ecdict.csv");
        let lemma = staging.join("lemma.en.txt");
        download(CSV_URL, CSV_BYTES, &csv, canceled, |received| {
            progress(InstallProgress {
                received,
                total: DOWNLOAD_BYTES,
                stage: InstallStage::Dictionary,
            });
        })?;
        verify_file(&csv, CSV_BYTES, CSV_SHA256, canceled)?;
        download(LEMMA_URL, LEMMA_BYTES, &lemma, canceled, |received| {
            progress(InstallProgress {
                received: CSV_BYTES + received,
                total: DOWNLOAD_BYTES,
                stage: InstallStage::Lemma,
            });
        })?;
        verify_file(&lemma, LEMMA_BYTES, LEMMA_SHA256, canceled)?;
        finish_staging(dir, &staging, &csv, &lemma, &mut progress, canceled)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn directory() -> Result<PathBuf> {
    crate::app_paths::translation_dir()
}

fn finish_staging(
    root: &Path,
    staging: &Path,
    csv: &Path,
    lemma: &Path,
    progress: &mut impl FnMut(InstallProgress),
    canceled: &AtomicBool,
) -> Result<DictionaryStatus> {
    check(canceled)?;
    progress(InstallProgress {
        received: DOWNLOAD_BYTES,
        total: DOWNLOAD_BYTES,
        stage: InstallStage::Convert,
    });
    let dict_path = staging.join(DICT_NAME);
    let lemma_path = staging.join(LEMMA_NAME);
    let entries = convert_csv(csv, &dict_path, canceled)?;
    convert_lemma(lemma, &lemma_path, canceled)?;
    let _ = fs::remove_file(csv);
    let _ = fs::remove_file(lemma);
    progress(InstallProgress {
        received: DOWNLOAD_BYTES,
        total: DOWNLOAD_BYTES,
        stage: InstallStage::Verify,
    });
    let dictionary_sha256 = sha256_hex(&dict_path, canceled)?;
    let lemma_tsv_sha256 = sha256_hex(&lemma_path, canceled)?;
    let receipt = Receipt {
        git: GIT.into(),
        csv_sha256: CSV_SHA256.into(),
        csv_bytes: CSV_BYTES,
        lemma_sha256: LEMMA_SHA256.into(),
        lemma_bytes: LEMMA_BYTES,
        entries,
        dictionary_sha256,
        dictionary_bytes: fs::metadata(&dict_path)?.len(),
        lemma_tsv_sha256,
        lemma_tsv_bytes: fs::metadata(&lemma_path)?.len(),
    };
    crate::durable::write(
        &staging.join(RECEIPT_NAME),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    verify_bundle_files(staging, &receipt, canceled)?;
    commit_staging(root, staging, canceled)?;
    Ok(DictionaryStatus::Ready { entries })
}

fn commit_staging(root: &Path, staging: &Path, canceled: &AtomicBool) -> Result<()> {
    let _commit = commit_lock();
    check(canceled)?;
    let next = next_bundle_id(root)?;
    let dest = root.join(&next);
    fs::rename(staging, &dest)?;
    crate::durable::sync_directory(root)?;
    crate::durable::write(&root.join(CURRENT_NAME), next.as_bytes())?;
    drop(_commit);
    remove_other_bundles(root, &next);
    Ok(())
}

fn pin_current(root: &Path) -> Result<Option<BundlePin>> {
    let mut pins = pin_table();
    let Some(name) = committed_name(root)? else {
        return Ok(None);
    };
    *pins
        .counts
        .entry((root.to_path_buf(), name.clone()))
        .or_insert(0) += 1;
    Ok(Some(BundlePin {
        root: root.to_path_buf(),
        name,
    }))
}

fn drop_pin(root: &Path, name: &str) {
    let mut pins = pin_table();
    let key = (root.to_path_buf(), name.to_owned());
    match pins.counts.get_mut(&key) {
        Some(count) if *count > 1 => {
            *count -= 1;
            return;
        }
        Some(_) => {
            pins.counts.remove(&key);
        }
        None => {}
    }
    let current = committed_name(root).ok().flatten();
    if current.as_deref() == Some(name) {
        return;
    }
    if is_bundle_name(name) {
        let path = root.join(name);
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn unique_staging(root: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    root.join(format!("staging-{}-{seq}", std::process::id()))
}

fn next_bundle_id(root: &Path) -> Result<String> {
    let mut n = committed_name(root)?
        .and_then(|name| name.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    while root.join(n.to_string()).exists() {
        n += 1;
    }
    Ok(n.to_string())
}

fn committed_name(root: &Path) -> Result<Option<String>> {
    let path = root.join(CURRENT_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let name = String::from_utf8(fs::read(path)?)?;
    let name = name.trim();
    if is_bundle_name(name) {
        Ok(Some(name.to_owned()))
    } else {
        Ok(None)
    }
}

fn committed_bundle(root: &Path) -> Result<Option<PathBuf>> {
    let Some(name) = committed_name(root)? else {
        return Ok(None);
    };
    let bundle = root.join(name);
    Ok(bundle.is_dir().then_some(bundle))
}

fn committed_receipt(root: &Path) -> Result<Option<Receipt>> {
    let Some(bundle) = committed_bundle(root)? else {
        return Ok(None);
    };
    let Some(receipt) = load_receipt(&bundle)? else {
        return Ok(None);
    };
    if !receipt_matches(&receipt) {
        return Ok(None);
    }
    verify_bundle_files(&bundle, &receipt, &AtomicBool::new(false))?;
    Ok(Some(receipt))
}

fn is_bundle_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit())
}

fn remove_other_bundles(root: &Path, keep: &str) {
    #[cfg(test)]
    invoke_before_cleanup();
    let pins = pin_table();
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == keep || name == CURRENT_NAME {
            continue;
        }
        if pins
            .counts
            .get(&(root.to_path_buf(), name.to_owned()))
            .copied()
            .unwrap_or(0)
            > 0
        {
            continue;
        }
        if is_bundle_name(name) || name.starts_with("staging-") {
            let path = entry.path();
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            }
        }
    }
}

fn verify_bundle_files(bundle: &Path, receipt: &Receipt, canceled: &AtomicBool) -> Result<()> {
    verify_file(
        &bundle.join(DICT_NAME),
        receipt.dictionary_bytes,
        &receipt.dictionary_sha256,
        canceled,
    )?;
    verify_file(
        &bundle.join(LEMMA_NAME),
        receipt.lemma_tsv_bytes,
        &receipt.lemma_tsv_sha256,
        canceled,
    )
}

fn receipt_matches(receipt: &Receipt) -> bool {
    receipt.git == GIT
        && receipt.csv_sha256 == CSV_SHA256
        && receipt.csv_bytes == CSV_BYTES
        && receipt.lemma_sha256 == LEMMA_SHA256
        && receipt.lemma_bytes == LEMMA_BYTES
}

fn load_receipt(dir: &Path) -> Result<Option<Receipt>> {
    let path = dir.join(RECEIPT_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn download(
    url: &str,
    size: u64,
    partial: &Path,
    canceled: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<()> {
    crate::net::download(
        &crate::net::Download {
            url,
            user_agent: &format!("hex/{}", crate::VERSION),
            size: Some(size),
            canceled: Some(canceled),
        },
        partial,
        progress,
    )
}

fn verify_file(path: &Path, bytes: u64, sha256: &str, canceled: &AtomicBool) -> Result<()> {
    let got = fs::metadata(path)?.len();
    if got != bytes {
        bail!("downloaded {got} bytes, expected {bytes}");
    }
    if sha256_hex(path, canceled)? != sha256 {
        bail!("checksum mismatch");
    }
    Ok(())
}

fn sha256_hex(path: &Path, canceled: &AtomicBool) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check(canceled)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn convert_csv(source: &Path, dest: &Path, canceled: &AtomicBool) -> Result<u64> {
    let mut reader = BufReader::new(File::open(source)?);
    let mut out = File::create(dest)?;
    writeln!(out, "# hex-translation-dictionary 1")?;
    writeln!(out, "# git {GIT}")?;
    let header = next_csv_record(&mut reader)?;
    if header
        .as_deref()
        .and_then(|fields| fields.first())
        .map(String::as_str)
        != Some("word")
    {
        bail!("ECDICT CSV header is missing");
    }
    let mut entries = 0u64;
    while let Some(fields) = next_csv_record(&mut reader)? {
        check(canceled)?;
        if fields.len() < 4 {
            continue;
        }
        let word = fields[0].trim();
        if word.is_empty() {
            continue;
        }
        let phonetic = fields[1].trim();
        let translation = fields[3].trim();
        if translation.is_empty() {
            continue;
        }
        let pos = fields.get(4).map(String::as_str).unwrap_or("").trim();
        writeln!(
            out,
            "{}\t{}\t{}\t{}",
            tsv_escape(word),
            tsv_escape(phonetic),
            tsv_escape(pos),
            tsv_escape(translation)
        )?;
        entries += 1;
    }
    out.flush()?;
    Ok(entries)
}

fn convert_lemma(source: &Path, dest: &Path, canceled: &AtomicBool) -> Result<()> {
    let reader = BufReader::new(File::open(source)?);
    let mut out = File::create(dest)?;
    writeln!(out, "# hex-translation-lemma 1")?;
    for line in reader.lines() {
        check(canceled)?;
        let line = line?;
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let Some((lemma_side, forms)) = line.split_once("->") else {
            continue;
        };
        let lemma = lemma_side
            .split('/')
            .next()
            .unwrap_or(lemma_side)
            .trim()
            .to_lowercase();
        if lemma.is_empty() {
            continue;
        }
        for form in forms.split(',') {
            let form = form.trim().trim_matches('\'').to_lowercase();
            if form.is_empty() || form == lemma {
                continue;
            }
            writeln!(out, "{}\t{}", tsv_escape(&form), tsv_escape(&lemma))?;
        }
    }
    out.flush()?;
    Ok(())
}

fn load_entries(path: &Path) -> Result<HashMap<Box<str>, Entry>> {
    let mut map = HashMap::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let Some(word) = parts.next() else { continue };
        let phonetic = parts.next().unwrap_or("");
        let pos = parts.next().unwrap_or("");
        let translation = parts.next().unwrap_or("");
        if word.is_empty() || translation.is_empty() {
            continue;
        }
        map.insert(
            Box::from(word.to_lowercase()),
            Entry {
                headword: Box::from(word),
                phonetic: (!phonetic.is_empty()).then(|| Box::from(phonetic)),
                translation: Box::from(translation.replace("\\n", "\n")),
                pos: pos.to_owned(),
            },
        );
    }
    Ok(map)
}

fn load_lemmas(path: &Path) -> Result<HashMap<Box<str>, Box<str>>> {
    let mut map = HashMap::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((form, lemma)) = line.split_once('\t') else {
            continue;
        };
        if form.is_empty() || lemma.is_empty() {
            continue;
        }
        map.insert(Box::from(form), Box::from(lemma));
    }
    Ok(map)
}

fn tsv_escape(value: &str) -> String {
    value
        .replace('\t', " ")
        .replace('\n', "\\n")
        .replace('\r', "")
}

fn check(canceled: &AtomicBool) -> Result<()> {
    if canceled.load(Ordering::Relaxed) {
        bail!("download canceled");
    }
    Ok(())
}

fn next_csv_record(reader: &mut impl BufRead) -> Result<Option<Vec<String>>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut started = false;
    loop {
        let mut buf = String::new();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            if !started {
                return Ok(None);
            }
            fields.push(std::mem::take(&mut field));
            return Ok(Some(fields));
        }
        started = true;
        let mut chars = buf.chars().peekable();
        while let Some(ch) = chars.next() {
            if in_quotes {
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else if ch != '\r' {
                    field.push(ch);
                }
            } else if ch == '"' {
                in_quotes = true;
            } else if ch == ',' {
                fields.push(std::mem::take(&mut field));
            } else if ch == '\n' {
                fields.push(std::mem::take(&mut field));
                return Ok(Some(fields));
            } else if ch != '\r' {
                field.push(ch);
            }
        }
        if !in_quotes {
            fields.push(std::mem::take(&mut field));
            return Ok(Some(fields));
        }
    }
}

#[cfg(test)]
fn parse_csv_record_for_tests(record: &str) -> Result<Vec<String>> {
    let mut bytes = record.as_bytes();
    next_csv_record(&mut bytes).and_then(|row| row.ok_or_else(|| eyre!("empty CSV record")))
}

#[cfg(test)]
std::thread_local! {
    static AFTER_PIN: std::cell::RefCell<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> =
        const { std::cell::RefCell::new(None) };
    static BEFORE_CLEANUP: std::cell::RefCell<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_after_pin_hook(hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
    AFTER_PIN.with(|cell| *cell.borrow_mut() = hook);
}

#[cfg(test)]
fn set_before_cleanup_hook(hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
    BEFORE_CLEANUP.with(|cell| *cell.borrow_mut() = hook);
}

#[cfg(test)]
fn invoke_after_pin() {
    let hook = AFTER_PIN.with(|cell| cell.borrow().clone());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
fn invoke_before_cleanup() {
    let hook = BEFORE_CLEANUP.with(|cell| cell.borrow().clone());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hex-translation-dict-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn csv_parser_keeps_quoted_commas_and_newlines() {
        let row = parse_csv_record_for_tests(
            "hello,hә'lәu,\"int. hi\",\"interj. 喂, 嘿\",int,3,1,,,1,s:hellos,,\n",
        )
        .unwrap();
        assert_eq!(row[0], "hello");
        assert_eq!(row[1], "hә'lәu");
        assert_eq!(row[3], "interj. 喂, 嘿");
    }

    #[test]
    fn convert_roundtrip_preserves_phonetic_absence() {
        let dir = scratch();
        let csv = dir.join("src.csv");
        fs::write(
            &csv,
            "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio\n\
hello,hә'lәu,,interj. 喂,int,,,,,,,\n\
hex,,,n. 十六进制,n,,,,,,,\n\
give,giv,,vt. 给,,d:given/p:gave,,,,,\n",
        )
        .unwrap();
        let lemma = dir.join("src.lemma");
        fs::write(&lemma, "give/1 -> gave,given,gives,giving\n").unwrap();
        let dict_out = dir.join("dictionary.tsv");
        let lemma_out = dir.join("lemma.tsv");
        convert_csv(&csv, &dict_out, &AtomicBool::new(false)).unwrap();
        convert_lemma(&lemma, &lemma_out, &AtomicBool::new(false)).unwrap();
        let dict = Dictionary {
            entries: load_entries(&dict_out).unwrap(),
            lemmas: load_lemmas(&lemma_out).unwrap(),
        };
        let hello = dict.lookup("hello").unwrap();
        assert_eq!(hello.phonetic.as_deref(), Some("hә'lәu"));
        assert!(dict.lookup("hex").unwrap().phonetic.is_none());
        assert_eq!(dict.lemma("gave").as_deref(), Some("give"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checksum_rejects_a_wrong_size() {
        let dir = scratch();
        let path = dir.join("file");
        fs::write(&path, b"hello").unwrap();
        assert!(verify_file(&path, 4, CSV_SHA256, &AtomicBool::new(false)).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    fn write_fixture_sources(dir: &Path) -> (PathBuf, PathBuf) {
        let csv = dir.join("src.csv");
        fs::write(
            &csv,
            "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio\n\
hello,hә'lәu,,interj. 喂,int,,,,,,,\n",
        )
        .unwrap();
        let lemma = dir.join("src.lemma");
        fs::write(&lemma, "give/1 -> gave,given\n").unwrap();
        (csv, lemma)
    }

    fn seed_committed(dir: &Path, word: &str, gloss: &str) -> u64 {
        crate::app_paths::create_private_dir(dir).unwrap();
        let bundle = dir.join("1");
        crate::app_paths::create_private_dir(&bundle).unwrap();
        let dict = bundle.join(DICT_NAME);
        let lemma = bundle.join(LEMMA_NAME);
        fs::write(
            &dict,
            format!("# hex-translation-dictionary 1\n{word}\t\tn\t{gloss}\n"),
        )
        .unwrap();
        fs::write(&lemma, "# hex-translation-lemma 1\n").unwrap();
        let dictionary_sha256 = sha256_hex(&dict, &AtomicBool::new(false)).unwrap();
        let lemma_tsv_sha256 = sha256_hex(&lemma, &AtomicBool::new(false)).unwrap();
        let receipt = Receipt {
            git: GIT.into(),
            csv_sha256: CSV_SHA256.into(),
            csv_bytes: CSV_BYTES,
            lemma_sha256: LEMMA_SHA256.into(),
            lemma_bytes: LEMMA_BYTES,
            entries: 1,
            dictionary_sha256,
            dictionary_bytes: fs::metadata(&dict).unwrap().len(),
            lemma_tsv_sha256,
            lemma_tsv_bytes: fs::metadata(&lemma).unwrap().len(),
        };
        fs::write(
            bundle.join(RECEIPT_NAME),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        crate::durable::write(&dir.join(CURRENT_NAME), b"1").unwrap();
        1
    }

    fn install_from_files(
        dir: &Path,
        csv: &Path,
        lemma: &Path,
        mut progress: impl FnMut(InstallProgress),
        canceled: &AtomicBool,
    ) -> Result<DictionaryStatus> {
        crate::app_paths::create_private_dir(dir)?;
        let staging = unique_staging(dir);
        crate::app_paths::create_private_dir(&staging)?;
        let staging_csv = staging.join("ecdict.csv");
        let staging_lemma = staging.join("lemma.en.txt");
        fs::copy(csv, &staging_csv)?;
        fs::copy(lemma, &staging_lemma)?;
        let result = finish_staging(
            dir,
            &staging,
            &staging_csv,
            &staging_lemma,
            &mut progress,
            canceled,
        );
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    #[test]
    fn cancel_before_commit_leaves_the_existing_bundle() {
        let dir = scratch();
        seed_committed(&dir, "keep", "保留");
        let (csv, lemma) = write_fixture_sources(&dir);
        let canceled = AtomicBool::new(false);
        let err = install_from_files(
            &dir,
            &csv,
            &lemma,
            |progress| {
                if progress.stage == InstallStage::Verify {
                    canceled.store(true, Ordering::Release);
                }
            },
            &canceled,
        )
        .unwrap_err();
        assert!(err.to_string().contains("canceled"));
        assert_eq!(committed_name(&dir).unwrap().as_deref(), Some("1"));
        let loaded = load_from(&dir).unwrap().unwrap();
        assert_eq!(loaded.lookup("keep").unwrap().translation.as_ref(), "保留");
        assert!(loaded.lookup("hello").is_none());
        match status_in(&dir) {
            DictionaryStatus::Ready { entries } => assert_eq!(entries, 1),
            other => panic!("committed bundle must stay Ready, got {other:?}"),
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn successful_commit_replaces_current_and_verifies_files() {
        let dir = scratch();
        seed_committed(&dir, "keep", "保留");
        let (csv, lemma) = write_fixture_sources(&dir);
        let status =
            install_from_files(&dir, &csv, &lemma, |_| {}, &AtomicBool::new(false)).unwrap();
        assert!(matches!(status, DictionaryStatus::Ready { entries } if entries == 1));
        assert_eq!(committed_name(&dir).unwrap().as_deref(), Some("2"));
        assert!(!dir.join("1").exists());
        let loaded = load_from(&dir).unwrap().unwrap();
        assert_eq!(
            loaded.lookup("hello").unwrap().translation.as_ref(),
            "interj. 喂"
        );
        assert!(loaded.lookup("keep").is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hash_mismatch_is_not_ready() {
        let dir = scratch();
        seed_committed(&dir, "keep", "保留");
        let bundle = dir.join("1");
        fs::write(bundle.join(DICT_NAME), "tampered\t\tn\t坏\n").unwrap();
        match status_in(&dir) {
            DictionaryStatus::Failed { .. } => {}
            other => panic!("tampered files must not be Ready, got {other:?}"),
        }
        assert!(load_from(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_during_commit_keeps_pinned_files() {
        use std::sync::{Arc, Barrier};
        let dir = scratch();
        seed_committed(&dir, "keep", "保留");
        let (csv, lemma) = write_fixture_sources(&dir);
        let pinned = Arc::new(Barrier::new(2));
        let go_read = Arc::new(Barrier::new(2));
        let cleanup_ready = Arc::new(Barrier::new(2));
        let release_cleanup = Arc::new(Barrier::new(2));
        let after_pin = Arc::new({
            let pinned = Arc::clone(&pinned);
            let go_read = Arc::clone(&go_read);
            move || {
                pinned.wait();
                go_read.wait();
            }
        });
        let before_cleanup = Arc::new({
            let cleanup_ready = Arc::clone(&cleanup_ready);
            let release_cleanup = Arc::clone(&release_cleanup);
            move || {
                cleanup_ready.wait();
                release_cleanup.wait();
            }
        });
        let load_dir = dir.clone();
        let loader = std::thread::spawn(move || {
            set_after_pin_hook(Some(after_pin));
            let loaded = load_keyed_from(&load_dir);
            set_after_pin_hook(None);
            loaded
        });
        pinned.wait();
        let install_dir = dir.clone();
        let installer = std::thread::spawn(move || {
            set_before_cleanup_hook(Some(before_cleanup));
            let status =
                install_from_files(&install_dir, &csv, &lemma, |_| {}, &AtomicBool::new(false));
            set_before_cleanup_hook(None);
            status
        });
        cleanup_ready.wait();
        assert!(
            dir.join("1").is_dir(),
            "commit must not delete a bundle that is still being loaded"
        );
        go_read.wait();
        let loaded = loader.join().unwrap().unwrap().unwrap();
        assert_eq!(loaded.key, "1");
        assert_eq!(
            loaded
                .dictionary
                .lookup("keep")
                .unwrap()
                .translation
                .as_ref(),
            "保留"
        );
        release_cleanup.wait();
        let status = installer.join().unwrap().unwrap();
        assert!(matches!(status, DictionaryStatus::Ready { entries } if entries == 1));
        assert_eq!(committed_name(&dir).unwrap().as_deref(), Some("2"));
        let current = load_from(&dir).unwrap().unwrap();
        assert_eq!(
            current.lookup("hello").unwrap().translation.as_ref(),
            "interj. 喂"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unpublished_staging_is_not_ready() {
        let dir = scratch();
        crate::app_paths::create_private_dir(&dir).unwrap();
        let staging = dir.join("staging-orphan");
        crate::app_paths::create_private_dir(&staging).unwrap();
        fs::write(staging.join(DICT_NAME), "hello\t\tn\t喂\n").unwrap();
        fs::write(staging.join(RECEIPT_NAME), "{}").unwrap();
        match status_in(&dir) {
            DictionaryStatus::Missing { .. } => {}
            other => panic!("staging without CURRENT must be Missing, got {other:?}"),
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
