use super::{LookupError, LookupErrorKind, MAX_QUERY_CHARS, Sense};
use crate::translation::dictionary::{Dictionary, Entry};

pub(super) fn normalize_submit(text: &str) -> Result<String, LookupError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(LookupError::new(LookupErrorKind::Empty));
    }
    if trimmed.chars().count() > MAX_QUERY_CHARS {
        return Err(LookupError::new(LookupErrorKind::TooLong));
    }
    if !looks_english(trimmed) {
        return Err(LookupError::new(LookupErrorKind::NotEnglish));
    }
    Ok(trimmed.to_owned())
}

pub(super) fn looks_english(text: &str) -> bool {
    let mut latin = 0usize;
    let mut other_letters = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else if ch.is_alphabetic() {
            other_letters += 1;
        }
    }
    latin > 0 && latin >= other_letters
}

pub(super) fn is_single_token(text: &str) -> bool {
    text.split_whitespace().nth(1).is_none()
}

#[derive(Debug)]
pub(super) enum Route {
    Word {
        entry: Entry,
        mapped_from: Option<String>,
    },
    NeedSystem {
        as_word: bool,
    },
}

pub(super) fn route(text: &str, dictionary: Option<&Dictionary>) -> Result<Route, LookupError> {
    let key = text.to_lowercase();
    if let Some(dictionary) = dictionary {
        if let Some(entry) = dictionary.lookup(&key) {
            return Ok(Route::Word {
                entry,
                mapped_from: None,
            });
        }
        if is_single_token(text)
            && let Some(lemma) = dictionary.lemma(&key)
            && lemma != key
            && let Some(entry) = dictionary.lookup(&lemma)
        {
            return Ok(Route::Word {
                entry,
                mapped_from: Some(text.to_owned()),
            });
        }
        return Ok(Route::NeedSystem {
            as_word: is_single_token(text),
        });
    }
    if is_single_token(text) {
        Err(LookupError::new(LookupErrorKind::DictionaryMissing))
    } else {
        Ok(Route::NeedSystem { as_word: false })
    }
}

pub(super) fn senses(translation: &str, pos_field: &str) -> Vec<Sense> {
    let text = translation.replace("\\n", "\n");
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((pos, gloss)) = split_pos_line(line) {
            out.push(Sense {
                pos: Some(pos),
                gloss,
            });
        } else {
            out.push(Sense {
                pos: none_if_empty(pos_field),
                gloss: line.to_owned(),
            });
        }
    }
    if out.is_empty() {
        let gloss = translation.trim();
        if !gloss.is_empty() {
            out.push(Sense {
                pos: none_if_empty(pos_field),
                gloss: gloss.to_owned(),
            });
        }
    }
    out
}

fn none_if_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn split_pos_line(line: &str) -> Option<(String, String)> {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        let pos = format!("[{}]", &rest[..end]);
        let gloss = rest[end + 1..].trim();
        if !gloss.is_empty() {
            return Some((pos, gloss.to_owned()));
        }
    }
    const MARKERS: &[&str] = &[
        "interj.", "interj", "abbr.", "abbr", "prep.", "prep", "conj.", "conj", "pron.", "pron",
        "aux.", "aux", "adj.", "adj", "adv.", "adv", "vt.", "vt", "vi.", "vi", "art.", "art",
        "num.", "num", "int.", "int", "n.", "n", "v.", "v", "a.", "a",
    ];
    for marker in MARKERS {
        if let Some(rest) = line.strip_prefix(marker)
            && rest.starts_with(|ch: char| ch.is_whitespace())
        {
            let gloss = rest.trim_start();
            if !gloss.is_empty() {
                return Some(((*marker).to_owned(), gloss.to_owned()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::dictionary::Dictionary;

    #[test]
    fn gates_empty_too_long_and_non_english() {
        assert_eq!(
            normalize_submit("   ").unwrap_err().kind,
            LookupErrorKind::Empty
        );
        let long = "a".repeat(MAX_QUERY_CHARS + 1);
        assert_eq!(
            normalize_submit(&long).unwrap_err().kind,
            LookupErrorKind::TooLong
        );
        assert_eq!(
            normalize_submit("你好世界").unwrap_err().kind,
            LookupErrorKind::NotEnglish
        );
        assert_eq!(
            normalize_submit("hex data must have 16 digits.").unwrap(),
            "hex data must have 16 digits."
        );
        assert!(normalize_submit("UTF-8 BOM").is_ok());
        assert_eq!(
            normalize_submit("  Hello, world.  ").unwrap(),
            "Hello, world."
        );
    }

    #[test]
    fn does_not_truncate_overlong_input() {
        let long = format!("{}!", "ab ".repeat(400));
        assert!(long.chars().count() > MAX_QUERY_CHARS);
        assert_eq!(
            normalize_submit(&long).unwrap_err().kind,
            LookupErrorKind::TooLong
        );
    }

    #[test]
    fn exact_multiword_beats_phrase_and_lemma_maps_a_miss() {
        let dict = Dictionary::from_parts(
            [
                (
                    "take off",
                    Entry {
                        headword: "take off".into(),
                        phonetic: None,
                        translation: "起飞".into(),
                        pos: String::new(),
                    },
                ),
                (
                    "give",
                    Entry {
                        headword: "give".into(),
                        phonetic: Some("gɪv".into()),
                        translation: "vt. 给".into(),
                        pos: String::new(),
                    },
                ),
            ],
            [("gave", "give")],
        );
        match route("take off", Some(&dict)).unwrap() {
            Route::Word { entry, mapped_from } => {
                assert_eq!(&*entry.headword, "take off");
                assert!(mapped_from.is_none());
            }
            Route::NeedSystem { .. } => panic!("multiword should hit the dictionary"),
        }
        match route("gave", Some(&dict)).unwrap() {
            Route::Word { entry, mapped_from } => {
                assert_eq!(&*entry.headword, "give");
                assert_eq!(mapped_from.as_deref(), Some("gave"));
                assert_eq!(entry.phonetic.as_deref(), Some("gɪv"));
            }
            Route::NeedSystem { .. } => panic!("lemma should find give"),
        }
        match route("unknownxyz", Some(&dict)).unwrap() {
            Route::NeedSystem { as_word } => assert!(as_word),
            Route::Word { .. } => panic!("unknown token is not a dictionary hit"),
        }
        match route("a longer phrase", Some(&dict)).unwrap() {
            Route::NeedSystem { as_word } => assert!(!as_word),
            Route::Word { .. } => panic!("phrase miss should use the system"),
        }
        assert_eq!(
            route("hello", None).unwrap_err().kind,
            LookupErrorKind::DictionaryMissing
        );
        match route("a longer phrase", None).unwrap() {
            Route::NeedSystem { as_word } => assert!(!as_word),
            Route::Word { .. } => panic!("no dictionary still translates phrases locally if able"),
        }
    }

    #[test]
    fn splits_pos_lines_and_keeps_unstructured_gloss() {
        let parsed = senses("n. 世界\nvt. 给", "");
        assert_eq!(parsed[0].pos.as_deref(), Some("n."));
        assert_eq!(parsed[0].gloss, "世界");
        assert_eq!(parsed[1].pos.as_deref(), Some("vt."));
        assert_eq!(senses("give的过去式", "").len(), 1);
        assert!(senses("give的过去式", "")[0].pos.is_none());
        assert_eq!(senses("[计] 十六进制", "")[0].pos.as_deref(), Some("[计]"));
    }

    #[test]
    fn never_invents_a_blank_phonetic() {
        let dict = Dictionary::from_parts(
            [(
                "hex",
                Entry {
                    headword: "hex".into(),
                    phonetic: None,
                    translation: "n. 十六进制".into(),
                    pos: String::new(),
                },
            )],
            [],
        );
        match route("hex", Some(&dict)).unwrap() {
            Route::Word { entry, .. } => assert!(entry.phonetic.is_none()),
            Route::NeedSystem { .. } => panic!("hex is in the fixture"),
        }
    }
}
