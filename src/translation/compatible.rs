use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use url::Url;

use super::{CompatiblePreview, LookupError, LookupErrorKind};

const TRANSLATE_PROMPT: &str = "Translate the following English text into Simplified Chinese. Return only the Chinese translation. Do not add notes, romanization, phonetic transcription, or quotation marks.\n\n";

pub fn preview(
    generation: u64,
    text: &str,
    base_url: &str,
    model: &str,
) -> Result<CompatiblePreview, LookupError> {
    if base_url.trim().is_empty() || model.trim().is_empty() {
        return Err(LookupError::new(LookupErrorKind::CompatibleUnconfigured));
    }
    let host = Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| base_url.to_owned());
    Ok(CompatiblePreview {
        generation,
        text: text.to_owned(),
        base_url: base_url.to_owned(),
        model: model.to_owned(),
        host,
    })
}

#[cfg(test)]
pub fn preview_matches(
    preview: &CompatiblePreview,
    generation: u64,
    text: &str,
    base_url: &str,
    model: &str,
) -> bool {
    preview.generation == generation
        && preview.text == text
        && preview.base_url == base_url
        && preview.model == model
}

pub fn capture_credential(path: &str, base_url: &str) -> Result<Option<String>, LookupError> {
    crate::dictation::compatible_api::credential(path, base_url)
        .map_err(|_| LookupError::new(LookupErrorKind::CompatibleUnconfigured))
}

pub fn send(
    preview: &CompatiblePreview,
    credential: Option<&str>,
    deadline: Duration,
    canceled: &AtomicBool,
) -> Result<String, LookupError> {
    if canceled.load(Ordering::Acquire) {
        return Err(LookupError::new(LookupErrorKind::Canceled));
    }
    let prompt = format!("{TRANSLATE_PROMPT}{}", preview.text);
    let text = crate::dictation::compatible_api::generate(
        &preview.base_url,
        &preview.model,
        credential,
        &prompt,
        deadline,
    )
    .map_err(|error| classify(&error.to_string()))?;
    if canceled.load(Ordering::Acquire) {
        return Err(LookupError::new(LookupErrorKind::Canceled));
    }
    if text.trim().is_empty() {
        return Err(LookupError::new(LookupErrorKind::CompatibleFailed));
    }
    Ok(text)
}

fn classify(message: &str) -> LookupError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        LookupError::new(LookupErrorKind::TimedOut)
    } else if lower.contains("credential") || lower.contains("choose a text model") {
        LookupError::new(LookupErrorKind::CompatibleUnconfigured)
    } else {
        LookupError::new(LookupErrorKind::CompatibleFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_requires_service_and_model_and_captures_exact_text() {
        assert_eq!(
            preview(3, "hello", "", "m").unwrap_err().kind,
            LookupErrorKind::CompatibleUnconfigured
        );
        assert_eq!(
            preview(3, "hello", "https://api.example.test/v1", "")
                .unwrap_err()
                .kind,
            LookupErrorKind::CompatibleUnconfigured
        );
        let ready = preview(7, "take off", "https://api.example.test/v1", "flash").unwrap();
        assert_eq!(ready.text, "take off");
        assert_eq!(ready.generation, 7);
        assert_eq!(ready.host, "api.example.test");
        assert!(preview_matches(
            &ready,
            7,
            "take off",
            "https://api.example.test/v1",
            "flash"
        ));
        assert!(!preview_matches(
            &ready,
            7,
            "hello",
            "https://api.example.test/v1",
            "flash"
        ));
        assert!(!preview_matches(
            &ready,
            8,
            "take off",
            "https://api.example.test/v1",
            "flash"
        ));
    }

    #[test]
    fn empty_credential_path_captures_none() {
        assert!(
            capture_credential("", "http://127.0.0.1/v1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prompt_is_not_the_dictation_protocol() {
        assert!(!TRANSLATE_PROMPT.contains("dictated speech"));
        assert!(!TRANSLATE_PROMPT.contains("OpenCode"));
        assert!(TRANSLATE_PROMPT.contains("Simplified Chinese"));
    }
}
