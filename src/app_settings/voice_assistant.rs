//! Voice assistant settings and its live opt-in gate.
use super::{AppSettings, HotkeyBinding, HotkeyModifiers, HotkeySlot, ModifierSide};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

static VOICE_ASSISTANT_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct VoiceAssistantSettings {
    pub hotkey: HotkeyBinding,
    pub playback: bool,
}

impl Default for VoiceAssistantSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding {
                modifiers: HotkeyModifiers {
                    option: Some(ModifierSide::Right),
                    ..Default::default()
                },
                key: None,
            },
            playback: true,
        }
    }
}

impl AppSettings {
    pub fn active_assistant_hotkey(&self) -> Option<&HotkeyBinding> {
        (cfg!(target_os = "macos")
            && self.labs.voice_assistant
            && crate::labs::Feature::VoiceAssistant.managed().is_none()
            && !self.hotkey_in_use(HotkeySlot::Assistant, &self.voice_assistant.hotkey))
        .then_some(&self.voice_assistant.hotkey)
    }
}

#[cfg(target_os = "macos")]
pub fn voice_assistant_enabled() -> bool {
    VOICE_ASSISTANT_ENABLED.load(Ordering::Acquire)
}

pub(super) fn apply(settings: &AppSettings) {
    let enabled = cfg!(target_os = "macos")
        && settings.labs.voice_assistant
        && crate::labs::Feature::VoiceAssistant.managed().is_none();
    VOICE_ASSISTANT_ENABLED.store(enabled, Ordering::Release);
    crate::voice_assistant::set_enabled(enabled);
    crate::voice_assistant::set_playback(settings.voice_assistant.playback);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_defaults_and_saved_binding_preserve_other_settings() {
        let mut settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.labs.voice_assistant);
        assert!(settings.voice_assistant.playback);
        assert_eq!(
            settings.voice_assistant.hotkey.modifiers.option,
            Some(ModifierSide::Right)
        );
        assert_eq!(settings.voice_assistant.hotkey.key, None);
        settings.sound_effect_volume = 0.37;
        settings.voice_assistant.hotkey.modifiers.option = Some(ModifierSide::Left);
        settings.voice_assistant.playback = false;
        settings.labs.voice_assistant = true;
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.voice_assistant, settings.voice_assistant);
        assert!(restored.labs.voice_assistant);
        assert_eq!(restored.sound_effect_volume, 0.37);
    }

    #[test]
    fn assistant_conflicts_with_an_either_side_dictation_but_not_the_other_side() {
        let mut settings = AppSettings::default();
        settings.dictation_hotkey = HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key: None,
        };
        let assistant = settings.voice_assistant.hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::Assistant, &assistant));
        settings
            .dictation_hotkey
            .set_standalone_side(ModifierSide::Left);
        assert!(!settings.hotkey_in_use(HotkeySlot::Assistant, &assistant));
        settings.raw_dictation_hotkey = Some(assistant.clone());
        assert!(settings.hotkey_in_use(HotkeySlot::Assistant, &assistant));
    }
}
