//! Process-wide English→Simplified Chinese lookup. Both translation surfaces
//! poll [`snapshot`]; work runs on one owned worker. Query text is never saved.

mod apple;
mod compatible;
mod dictionary;
mod query;
mod service;
#[cfg(test)]
mod speech;

use std::time::Duration;

pub const MAX_QUERY_CHARS: usize = 1000;

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub revision: u64,
    pub enabled: bool,
    pub text: String,
    pub generation: u64,
    pub phase: Phase,
    pub outcome: Option<Outcome>,
    pub error: Option<LookupError>,
    pub resources: ResourceStatus,
    pub voices: Vec<VoiceInfo>,
    pub speaking: Option<Speaking>,
    pub dictionary_install: Option<InstallProgress>,
    pub online_preview: Option<CompatiblePreview>,
}

impl Snapshot {
    fn idle() -> Self {
        Self {
            revision: 0,
            enabled: false,
            text: String::new(),
            generation: 0,
            phase: Phase::Idle,
            outcome: None,
            error: None,
            resources: ResourceStatus::unknown(),
            voices: Vec::new(),
            speaking: None,
            dictionary_install: None,
            online_preview: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Idle,
    LocalPending,
    Ready,
    OnlinePreview,
    OnlinePending,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Word(WordResult),
    Phrase(PhraseResult),
}

impl Outcome {
    fn spoken_text(&self) -> &str {
        match self {
            Self::Word(word) => &word.headword,
            Self::Phrase(phrase) => &phrase.query,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WordResult {
    pub query: String,
    pub headword: String,
    pub mapped_from: Option<String>,
    pub phonetic: Option<String>,
    pub senses: Vec<Sense>,
    pub source: Source,
}

#[derive(Clone, Debug)]
pub struct Sense {
    pub pos: Option<String>,
    pub gloss: String,
}

#[derive(Clone, Debug)]
pub struct PhraseResult {
    pub query: String,
    pub translation: String,
    pub source: Source,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Dictionary,
    DictionaryLemma,
    System,
    Compatible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LookupError {
    pub kind: LookupErrorKind,
    pub message: &'static str,
    pub recovery: Vec<Recovery>,
}

impl LookupError {
    fn new(kind: LookupErrorKind) -> Self {
        Self {
            kind,
            message: kind.message(),
            recovery: kind.recovery(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LookupErrorKind {
    Empty,
    TooLong,
    NotEnglish,
    DictionaryMissing,
    SystemUnavailable,
    SystemNotReady,
    SystemUnsupported,
    CompatibleUnconfigured,
    CompatibleFailed,
    Canceled,
    TimedOut,
    AudioBusy,
}

impl LookupErrorKind {
    fn message(self) -> &'static str {
        match self {
            Self::Empty => "Enter English text to translate",
            Self::TooLong => "Use at most 1000 characters",
            Self::NotEnglish => "This version translates English into Simplified Chinese",
            Self::DictionaryMissing => "The English–Chinese dictionary is not installed",
            Self::SystemUnavailable => "On-device translation failed",
            Self::SystemNotReady => {
                "Install English and Simplified Chinese in System Settings › Language & Region"
            }
            Self::SystemUnsupported => "On-device translation needs macOS 26 or later",
            Self::CompatibleUnconfigured => "Choose a compatible API service and model in Settings",
            Self::CompatibleFailed => "The translation service did not return a result",
            Self::Canceled => "Canceled",
            Self::TimedOut => "Translation timed out",
            Self::AudioBusy => "Hex is recording; playback is paused",
        }
    }

    fn recovery(self) -> Vec<Recovery> {
        match self {
            Self::DictionaryMissing => vec![Recovery::InstallDictionary, Recovery::PrepareOnline],
            Self::SystemNotReady => vec![
                Recovery::RecheckSystem,
                Recovery::OpenSystemLanguageSettings,
                Recovery::PrepareOnline,
            ],
            Self::SystemUnsupported | Self::SystemUnavailable => vec![Recovery::PrepareOnline],
            Self::CompatibleUnconfigured => vec![Recovery::OpenCompatibleSettings],
            Self::CompatibleFailed | Self::TimedOut => vec![Recovery::PrepareOnline],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recovery {
    InstallDictionary,
    RecheckSystem,
    OpenSystemLanguageSettings,
    OpenCompatibleSettings,
    PrepareOnline,
}

#[derive(Clone, Debug)]
pub struct ResourceStatus {
    pub dictionary: DictionaryStatus,
    pub system: SystemTranslationStatus,
}

impl ResourceStatus {
    fn unknown() -> Self {
        Self {
            dictionary: DictionaryStatus::Missing {
                bytes: dictionary::DOWNLOAD_BYTES,
            },
            system: SystemTranslationStatus::Unsupported,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DictionaryStatus {
    Missing {
        bytes: u64,
    },
    Installing {
        received: u64,
        total: u64,
        stage: InstallStage,
    },
    Ready {
        entries: u64,
    },
    Failed {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallStage {
    Dictionary,
    Lemma,
    Convert,
    Verify,
}

#[derive(Clone, Debug)]
pub enum SystemTranslationStatus {
    Unsupported,
    NotInstalled,
    Ready,
    Failed { message: String },
}

#[derive(Clone, Debug)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub accent: Accent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Accent {
    Us,
    Gb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeechRate {
    Normal,
    Slow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Speaking {
    pub text: String,
    pub voice_id: String,
    pub rate: SpeechRate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatiblePreview {
    pub generation: u64,
    pub text: String,
    pub base_url: String,
    pub model: String,
    pub host: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub enabled: bool,
    pub compatible_base_url: String,
    pub compatible_model: String,
    pub compatible_credential_path: String,
    pub compatible_deadline: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            compatible_base_url: String::new(),
            compatible_model: String::new(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstallProgress {
    pub received: u64,
    pub total: u64,
    pub stage: InstallStage,
}

pub fn configure(config: Config) {
    service::configure(config);
}

pub fn snapshot() -> Snapshot {
    service::snapshot()
}

pub fn edit(text: String) {
    service::edit(text);
}

pub fn submit_local() {
    service::submit_local();
}

pub fn cancel() {
    service::cancel();
}

pub fn prepare_online() -> Result<CompatiblePreview, LookupError> {
    service::prepare_online()
}

pub fn confirm_online(preview: CompatiblePreview) {
    service::confirm_online(preview);
}

pub fn install_dictionary() {
    service::install_dictionary();
}

pub fn cancel_install() {
    service::cancel_install();
}

pub fn recheck_resources() {
    service::recheck_resources();
}

pub fn speak(voice_id: String, rate: SpeechRate) {
    service::speak(voice_id, rate);
}

pub fn stop_speech() {
    service::stop_speech();
}

pub fn set_audio_busy(busy: bool) {
    service::set_audio_busy(busy);
}
