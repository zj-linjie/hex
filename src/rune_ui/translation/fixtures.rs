//! Named snapshots the Translation scenes and tests pin.

use crate::translation::{
    Accent, DictionaryStatus, LookupError, LookupErrorKind, Outcome, Phase, PhraseResult, Recovery,
    ResourceStatus, Sense, Snapshot, Source, SystemTranslationStatus, VoiceInfo, WordResult,
};

fn voices() -> Vec<VoiceInfo> {
    vec![
        VoiceInfo {
            id: "com.apple.voice.compact.en-GB.Daniel".into(),
            name: "Daniel".into(),
            accent: Accent::Gb,
        },
        VoiceInfo {
            id: "com.apple.voice.compact.en-US.Samantha".into(),
            name: "Samantha".into(),
            accent: Accent::Us,
        },
    ]
}

fn ready_resources() -> ResourceStatus {
    ResourceStatus {
        dictionary: DictionaryStatus::Ready { entries: 770_611 },
        system: SystemTranslationStatus::Ready,
    }
}

fn missing_resources() -> ResourceStatus {
    ResourceStatus {
        dictionary: DictionaryStatus::Missing { bytes: 66_009_548 },
        system: SystemTranslationStatus::NotInstalled,
    }
}

fn error(kind: LookupErrorKind, recovery: Vec<Recovery>) -> LookupError {
    LookupError {
        kind,
        message: match kind {
            LookupErrorKind::DictionaryMissing => "The English–Chinese dictionary is not installed",
            LookupErrorKind::SystemUnavailable => "On-device translation failed",
            LookupErrorKind::TooLong => "Use at most 1000 characters",
            LookupErrorKind::Empty => "Enter English text to translate",
            LookupErrorKind::NotEnglish => {
                "This version translates English into Simplified Chinese"
            }
            LookupErrorKind::SystemNotReady => {
                "Install English and Simplified Chinese in System Settings › Language & Region"
            }
            LookupErrorKind::SystemUnsupported => "On-device translation needs macOS 26 or later",
            LookupErrorKind::CompatibleUnconfigured => {
                "Choose a compatible API service and model in Settings"
            }
            LookupErrorKind::CompatibleFailed => "The translation service did not return a result",
            LookupErrorKind::Canceled => "Canceled",
            LookupErrorKind::TimedOut => "Translation timed out",
            LookupErrorKind::AudioBusy => "Hex is recording; playback is paused",
        },
        recovery,
    }
}

fn base(text: &str, phase: Phase) -> Snapshot {
    Snapshot {
        revision: 1,
        enabled: true,
        text: text.into(),
        generation: 1,
        phase,
        outcome: None,
        error: None,
        resources: ready_resources(),
        voices: voices(),
        speaking: None,
        dictionary_install: None,
        online_preview: None,
    }
}

pub(super) fn word() -> Snapshot {
    let mut snap = base("running", Phase::Ready);
    snap.outcome = Some(Outcome::Word(WordResult {
        query: "running".into(),
        headword: "run".into(),
        mapped_from: Some("running".into()),
        phonetic: Some("rʌn".into()),
        senses: vec![
            Sense {
                pos: Some("v.".into()),
                gloss: "跑；运行".into(),
            },
            Sense {
                pos: Some("n.".into()),
                gloss: "奔跑；运行".into(),
            },
        ],
        source: Source::DictionaryLemma,
    }));
    snap
}

pub(super) fn phrase() -> Snapshot {
    let mut snap = base("good morning", Phase::Ready);
    snap.outcome = Some(Outcome::Phrase(PhraseResult {
        query: "good morning".into(),
        translation: "早上好".into(),
        source: Source::System,
    }));
    snap
}

pub(super) fn loading() -> Snapshot {
    base("hex", Phase::LocalPending)
}

pub(super) fn missing() -> Snapshot {
    let mut snap = base("hex", Phase::Idle);
    snap.resources = missing_resources();
    snap.error = Some(error(
        LookupErrorKind::DictionaryMissing,
        vec![Recovery::InstallDictionary, Recovery::PrepareOnline],
    ));
    snap
}

pub(super) fn lookup_error() -> Snapshot {
    let mut snap = base("hex", Phase::Idle);
    snap.error = Some(error(
        LookupErrorKind::SystemUnavailable,
        vec![Recovery::PrepareOnline],
    ));
    snap
}
