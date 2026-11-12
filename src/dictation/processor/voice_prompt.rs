//! Labs › Voice prompt's request and answer: the spoken instruction replaces
//! the mode's rewrite with one request of its own, and a failed or empty one
//! pastes what the speaker dictated before it.

use super::*;

/// One voice-prompt request instead of the mode's rewrite: the spoken
/// instruction over the text before it, or what is there when one side is
/// empty. A phrase with nothing around it asks nothing and pastes nothing.
pub(super) fn voice_prompt_processed(
    profile: &Profile,
    instruction: &crate::dictation::voice_prompt::Instruction,
    corrected: &str,
    context: &ContextSnapshot,
    cancelled: &AtomicBool,
) -> Processed {
    use crate::dictation::voice_prompt::Instruction;
    if matches!(instruction, Instruction::Nothing) {
        tracing::info!(
            profile = profile.name,
            "voice prompt had nothing to ask; nothing is pasted"
        );
        // No request ran, and no transformation gets a chance to turn an
        // empty result into something that would paste.
        return Processed {
            text: String::new(),
            nothing: true,
            observation: None,
            transformations: Vec::new(),
        };
    }
    let request = voice_prompt_request(
        profile,
        instruction,
        context,
        &crate::dictation::vocabulary::merged(
            &profile.vocabulary,
            &crate::dictation::vocabulary::entries(),
        ),
    );
    // A voice prompt is one request: the mode's own rewrite never runs, even
    // while its switch is off, with its model and deadline when it names one.
    let (text, observation) = generate_observed(
        &request,
        profile.model.as_ref(),
        profile.compatible.as_ref(),
        profile.deadline,
        cancelled,
        &profile.name,
        Instant::now(),
    );
    match &observation.fallback {
        None => tracing::info!(
            profile = profile.name,
            latency_ms = observation.latency_ms,
            "voice prompt completed"
        ),
        Some(reason) => tracing::warn!(
            profile = profile.name,
            latency_ms = observation.latency_ms,
            %reason,
            "voice prompt failed; pasting what the speaker dictated"
        ),
    }
    voice_prompt_answer(
        instruction,
        text,
        corrected,
        &profile.transformations,
        Some(observation),
    )
}

/// The result a voice prompt pastes: the model's answer when it has one, else
/// [`voice_prompt_fallback`], then the mode's transformations as usual.
pub(super) fn voice_prompt_answer(
    instruction: &crate::dictation::voice_prompt::Instruction,
    generated: Option<String>,
    corrected: &str,
    transformations: &[String],
    observation: Option<ProcessingObservation>,
) -> Processed {
    Processed {
        text: generated.unwrap_or_else(|| voice_prompt_fallback(instruction, corrected)),
        nothing: false,
        observation,
        transformations: transformations.to_vec(),
    }
}

/// The request one voice prompt makes: the protocol, the mode's instructions
/// as background while its rewrite is on, the vocabulary and foreground
/// context a rewrite carries, and the spoken task.
pub(super) fn voice_prompt_request(
    profile: &Profile,
    instruction: &crate::dictation::voice_prompt::Instruction,
    context: &ContextSnapshot,
    vocabulary: &[VocabularyEntry],
) -> String {
    use crate::dictation::voice_prompt::Instruction;
    let application = context.application.as_deref().unwrap_or("unknown");
    let host = context.browser_host().unwrap_or("none");
    let background = if profile.ai_enabled && !profile.prompt.trim().is_empty() {
        format!(
            "Background instructions for this mode (the spoken instruction overrides them when they conflict):\n{}\n\n",
            profile.prompt
        )
    } else {
        String::new()
    };
    let task = match instruction {
        Instruction::Rewrite { text, prompt } => format!(
            "The speaker dictated some text, then gave an instruction about it. Apply the spoken instruction to the dictated text and return only the result.\n\nSpoken instruction:\n<instruction>\n{prompt}\n</instruction>\n\nDictated text:\n<text>\n{text}\n</text>"
        ),
        Instruction::RewriteText { text } => format!(
            "Rewrite the dictated text below so it is clear and concise. Keep its meaning, facts, numbers, and names. Return only the rewritten text.\n\nDictated text:\n<text>\n{text}\n</text>"
        ),
        Instruction::Write { prompt } => format!(
            "The speaker asked you to write something. Write exactly what the spoken instruction asks for and return only that text.\n\nSpoken instruction:\n<instruction>\n{prompt}\n</instruction>"
        ),
        Instruction::Nothing => unreachable!("nothing is never asked"),
    };
    format!(
        "{PROTOCOL_PROMPT}\n\n{background}{}Foreground application: {application}\nBrowser host: {host}\n\n{task}",
        vocabulary_lines(vocabulary)
    )
}

/// What a failed, empty, or timed-out voice prompt pastes: the text before
/// the phrase, or the whole corrected transcript when there is none.
pub(super) fn voice_prompt_fallback(
    instruction: &crate::dictation::voice_prompt::Instruction,
    corrected: &str,
) -> String {
    use crate::dictation::voice_prompt::Instruction;
    match instruction {
        Instruction::Rewrite { text, .. } | Instruction::RewriteText { text } => text.clone(),
        Instruction::Write { .. } => corrected.to_owned(),
        Instruction::Nothing => String::new(),
    }
}
