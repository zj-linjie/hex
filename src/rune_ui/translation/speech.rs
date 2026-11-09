//! Voice buttons and the stored Normal/Slow rate for the next speak.

use std::sync::atomic::{AtomicU8, Ordering};

use rune::{El, row, text};
use rune_kit::ButtonVariant;

use super::super::controls::{DESCRIPTION, Paint};
use super::content::{action_button, live_engine};
use crate::desktop::i18n::tr;
use crate::translation::{self, Accent, Snapshot, SpeechRate, VoiceInfo};

const RATE_NORMAL: u8 = 0;
const RATE_SLOW: u8 = 1;

static PREFERRED_RATE: AtomicU8 = AtomicU8::new(RATE_NORMAL);

pub(super) fn preferred_rate() -> SpeechRate {
    match PREFERRED_RATE.load(Ordering::Acquire) {
        RATE_SLOW => SpeechRate::Slow,
        _ => SpeechRate::Normal,
    }
}

pub(super) fn set_preferred_rate(rate: SpeechRate) {
    PREFERRED_RATE.store(
        match rate {
            SpeechRate::Slow => RATE_SLOW,
            SpeechRate::Normal => RATE_NORMAL,
        },
        Ordering::Release,
    );
}

pub(super) fn voices_row<S: 'static>(p: Paint, snap: &Snapshot, spoken: &str) -> El {
    let us = snap.voices.iter().find(|voice| voice.accent == Accent::Us);
    let gb = snap.voices.iter().find(|voice| voice.accent == Accent::Gb);
    let speaking = snap.speaking.as_ref();
    let mut row = row().key("translation-voices").gap(8.0).flex_wrap();
    if let Some(voice) = gb {
        row = row.child(speak_button::<S>(
            p,
            "translation-speak-gb",
            tr("Speak British English"),
            voice,
            speaking,
            spoken,
        ));
    }
    if let Some(voice) = us {
        row = row.child(speak_button::<S>(
            p,
            "translation-speak-us",
            tr("Speak US English"),
            voice,
            speaking,
            spoken,
        ));
    }
    if snap.voices.is_empty() {
        row = row.child(
            text(tr("No English voices are installed. Add them in System Settings › Accessibility › Spoken Content."))
                .key("no-voices")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap()
                .grow(1.0),
        );
    } else {
        row = row.child(rate_buttons::<S>(p, speaking));
        if speaking.is_some() {
            row = row.child(action_button(
                p,
                "translation-stop-speech",
                tr("Stop speaking"),
                ButtonVariant::Neutral,
                |_: &mut S, _| live_stop_speech(),
            ));
        }
    }
    row
}

fn speak_button<S: 'static>(
    p: Paint,
    key: &'static str,
    label: &'static str,
    voice: &VoiceInfo,
    speaking: Option<&translation::Speaking>,
    spoken: &str,
) -> El {
    let id = voice.id.clone();
    let active = speaking.is_some_and(|now| now.voice_id == voice.id && now.text == spoken);
    let rate = preferred_rate();
    action_button(
        p,
        key,
        if active {
            tr("Stop speaking").to_string()
        } else {
            format!("{label} · {}", voice.name)
        },
        if active {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        },
        move |_: &mut S, _| {
            if active {
                live_stop_speech();
            } else {
                live_speak(id.clone(), rate);
            }
        },
    )
}

fn rate_buttons<S: 'static>(p: Paint, speaking: Option<&translation::Speaking>) -> El {
    let current = preferred_rate();
    let voice = speaking.map(|now| now.voice_id.clone());
    row()
        .key("translation-rate")
        .gap(4.0)
        .child(rate_button::<S>(
            p,
            "translation-rate-normal",
            tr("Normal"),
            SpeechRate::Normal,
            current,
            voice.clone(),
        ))
        .child(rate_button::<S>(
            p,
            "translation-rate-slow",
            tr("Slow"),
            SpeechRate::Slow,
            current,
            voice,
        ))
}

fn rate_button<S: 'static>(
    p: Paint,
    key: &'static str,
    label: &'static str,
    rate: SpeechRate,
    current: SpeechRate,
    voice: Option<String>,
) -> El {
    action_button(
        p,
        key,
        label,
        if current == rate {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        },
        move |_: &mut S, _| {
            set_preferred_rate(rate);
            if let Some(id) = voice.clone() {
                live_speak(id, rate);
            }
        },
    )
}

fn live_speak(id: String, rate: SpeechRate) {
    if live_engine() {
        crate::translation::speak(id, rate);
    }
}

fn live_stop_speech() {
    if live_engine() {
        crate::translation::stop_speech();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::{Accent, VoiceInfo};

    #[test]
    fn stored_rate_is_independent_of_playback() {
        set_preferred_rate(SpeechRate::Slow);
        assert_eq!(preferred_rate(), SpeechRate::Slow);
        set_preferred_rate(SpeechRate::Normal);
        assert_eq!(preferred_rate(), SpeechRate::Normal);
        set_preferred_rate(SpeechRate::Slow);
        assert_eq!(preferred_rate(), SpeechRate::Slow);
        set_preferred_rate(SpeechRate::Normal);
    }

    #[test]
    fn missing_gb_or_us_is_not_no_english_voices() {
        let voices = [VoiceInfo {
            id: "com.apple.voice.compact.en-AU.Karen".into(),
            name: "Karen".into(),
            accent: Accent::Us,
        }];
        assert!(!voices.is_empty());
        assert!(voices.iter().any(|voice| voice.accent == Accent::Us));
    }
}
