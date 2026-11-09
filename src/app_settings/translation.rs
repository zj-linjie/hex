//! Labs › Translation settings (macOS): its independent shortcut.

use super::*;

/// Where Labs › Translation stores its shortcut. The Lab's switch is
/// `labs.translation`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct TranslationSettings {
    /// Opens the translation popup: Control-Option-T by default.
    pub hotkey: HotkeyBinding,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding::translation_default(),
        }
    }
}

impl AppSettings {
    /// The translation shortcut while it takes effect: macOS, the Lab on,
    /// bound, and sharing no key with another active shortcut.
    pub fn active_translation_hotkey(&self) -> Option<&HotkeyBinding> {
        if !cfg!(target_os = "macos") || !self.labs.translation {
            return None;
        }
        Some(&self.translation.hotkey)
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::Translation, binding))
    }
}

fn engine_config(settings: &AppSettings) -> crate::translation::Config {
    let enabled = cfg!(target_os = "macos") && settings.labs.translation;
    let processing = &settings.dictation_processing.default_mode.post_processing;
    let compatible = &processing.compatible;
    let deadline = std::time::Duration::from_secs(processing.deadline_seconds.max(1));
    crate::translation::Config {
        enabled,
        compatible_base_url: compatible.base_url.clone(),
        compatible_model: compatible.model.clone(),
        compatible_credential_path: compatible.credential_path.clone(),
        compatible_deadline: deadline,
    }
}

pub(super) fn apply_translation(settings: &AppSettings) {
    let config = engine_config(settings);
    let fingerprint = ConfigFingerprint {
        enabled: config.enabled,
        base_url: config.compatible_base_url.clone(),
        model: config.compatible_model.clone(),
        credential_path: config.compatible_credential_path.clone(),
        deadline_secs: config.compatible_deadline.as_secs(),
    };
    let mut last = LAST_CONFIG
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if last.as_ref() == Some(&fingerprint) {
        return;
    }
    *last = Some(fingerprint);
    crate::translation::configure(config);
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigFingerprint {
    enabled: bool,
    base_url: String,
    model: String,
    credential_path: String,
    deadline_secs: u64,
}

static LAST_CONFIG: OnceLock<std::sync::Mutex<Option<ConfigFingerprint>>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_is_off_and_bound_to_control_option_t_by_default() {
        let settings = AppSettings::default();
        assert!(!settings.labs.translation);
        assert_eq!(settings.active_translation_hotkey(), None);
        let hotkey = HotkeyBinding::translation_default();
        assert!(hotkey.modifiers.control.is_some());
        assert!(hotkey.modifiers.option.is_some());
        assert_eq!(hotkey.key.as_ref().map(|key| key.label.as_str()), Some("T"));
    }

    #[test]
    fn an_active_translation_hotkey_conflicts_both_ways() {
        let mut settings = AppSettings::default();
        settings.labs.translation = true;
        settings.translation.hotkey = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::Translation, &settings.translation.hotkey));
        assert!(settings.hotkey_in_use(HotkeySlot::Dictation, &settings.translation.hotkey));
        assert_eq!(settings.active_translation_hotkey(), None);
        settings.translation.hotkey = HotkeyBinding::translation_default();
        assert!(!settings.hotkey_in_use(HotkeySlot::Translation, &settings.translation.hotkey));
        assert_eq!(
            settings
                .active_translation_hotkey()
                .map(|binding| binding.key.as_ref().map(|key| key.label.as_str())),
            Some(Some("T"))
        );
    }

    #[test]
    fn configure_follows_compatible_settings_with_the_rewrite_lab_off() {
        let mut settings = AppSettings::default();
        settings.labs.translation = true;
        settings.labs.compatible_rewrite = false;
        settings
            .dictation_processing
            .default_mode
            .post_processing
            .compatible
            .selected = false;
        settings
            .dictation_processing
            .default_mode
            .post_processing
            .compatible
            .base_url = "http://127.0.0.1:11434/v1".into();
        settings
            .dictation_processing
            .default_mode
            .post_processing
            .compatible
            .model = "local".into();
        let config = engine_config(&settings);
        assert_eq!(config.enabled, cfg!(target_os = "macos"));
        assert_eq!(config.compatible_base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(config.compatible_model, "local");
        settings.labs.translation = false;
        assert!(!engine_config(&settings).enabled);
    }
}
