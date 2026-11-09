# Translation (Labs, macOS)

Personal-fork Labs feature for English → Simplified Chinese words and short
phrases: a local ECDICT card with phonetics when the dictionary supplies them,
on-device sentence translation when Apple's language pair is installed, local
`AVSpeechSynthesizer` speech from real `en-GB` / `en-US` voices, a Hex pane,
and an independent Rune window on Control-Option-T. Online fallback uses the
already-configured compatible API and sends only after an exact preview.

macOS only, audience `Everyone`, Labs group Output, `labs.translation` off by
default. Query text, glosses, and speech audio are transient: they are not
written to settings, History, Paste Last, or ordinary logs. This map records
implemented behavior in this checkout. It is not acceptance, a merge, or an
installed-copy update. Linear
[ZJ-31](https://linear.app/zjlinjie/issue/ZJ-31/hexmacos-本地优先英译中音标朗读与独立快捷键窗口).
PRD: [translation-prd.md](../plans/translation-prd.md). Visual prototype:
[designs/translation](../designs/translation/README.md).

```ts
Labs > Output > Translation [off]                         // TR-01
├── On -> Translation joins the sidebar (macOS)
│   ├── pane: query, Look up / Translate, speech, copy, resource row, Open window
│   ├── settings: Translation shortcut recorder (also on the Labs card)
│   └── Open in Hex from the popup focuses this pane
└── bound global shortcut -> one independent Rune window   // TR-02
    └── focus the query; Paste / Enter submits; Esc hides it
```

Turning the Lab off hides the sidebar item, refuses `open_popup`, disables
the engine (`configure` with `enabled: false`), cancels in-flight work,
stops speech, and clears the in-memory query. The saved shortcut and
`labs.translation` stay in settings. In-flight compatible HTTP is not
guaranteed to abort on the wire.

## Entry and windows

```ts
Control-Option-T (editable)                               // TR-09
├── Lab off, conflict, or unbound -> no match
├── dictation listener not running -> independent CGEventTap
│   (no microphone or model; needs macOS keyboard access)
│   normal listener takes over once running
├── press once -> retract a provisional dictation start and cancel that
│   capture; an intentional dictation continues; show/focus the popup
├── already open -> focus the same window; do not create a second;
│   do not submit; do not read the clipboard
└── hide (Esc, red close, Lab off) -> stop speech; Hex keeps running
```

The popup is a normal, focusable extra Rune window in the same process
(`540×460`, min `400×320`), not the click-through dictation HUD. Chrome is
standard (not full-size content). Opening it makes the Dock icon visible and
activates Hex. The main pane and popup share `crate::translation` snapshot
state: one current query, generation, outcome, and preview. Snapshot text
synchronizes even in a focused field while preserving a valid selection;
active IME preedit defers replacement.

Sources: [labs.rs](../../src/labs.rs) (`Feature::Translation`,
`labs::translation`), [group.rs](../../src/labs/group.rs) (Output),
[app_settings/translation.rs](../../src/app_settings/translation.rs),
[sidebar.rs](../../src/rune_ui/sidebar.rs) (`Pane::Translation`),
[rune_ui/translation.rs](../../src/rune_ui/translation.rs),
[popup.rs](../../src/rune_ui/translation/popup.rs),
[macos_rune.rs](../../src/macos_rune.rs) (`install_popup`, `show_translation`,
`wake_translation`),
[translation_shortcut.rs](../../src/macos_rune/translation_shortcut.rs)
(fallback tap while the dictation listener is not running),
[recognition.rs](../../src/recognition.rs) (`HotkeyAction::Translation`),
[suppression.rs](../../src/suppression.rs) (`paste_action` translation slot).

Checks:
`translation_is_its_own_bit_and_off_by_default` (labs.rs),
`translation_is_off_and_bound_to_control_option_t_by_default` and
`an_active_translation_hotkey_conflicts_both_ways`
(app_settings/translation.rs),
`translation_labs_toggle_controls_its_sidebar_item` and
`translation_hotkey_recorder_opens_from_the_pane` (rune_ui/tests/panes.rs),
`control_option_t_opens_translation_and_not_paste` (suppression.rs),
`translation_controls_are_registered` (control/settings.rs),
`open_without_the_lab_does_nothing` (popup.rs),
`keyboard_entry_does_not_depend_on_dictation_setup` (translation_shortcut.rs).
Not observed: a physical Control-Option-T on a Mac, Space/fullscreen
activation, two-window focus with a real IME, or the fallback tap with
macOS keyboard permission granted.

## Query (TR-03)

```ts
query field                                               // TR-03
├── Cmd+V or Paste -> read clipboard once, keep plain text, trim ends
├── open window / poll / Lab on -> do not read the clipboard
├── Enter in the field -> submit; Shift+Enter -> newline
├── empty or >1000 Unicode characters -> refuse; do not truncate
├── non-English (more non-Latin letters than ASCII letters) ->
│   "This version translates English into Simplified Chinese"
├── edit -> invalidate the current generation; old replies cannot land
└── Look up (one token) / Translate (whitespace in the query)
```

`MAX_QUERY_CHARS = 1000`. Submit is disabled while `LocalPending` or
`OnlinePending`. Duplicate submit of the same in-flight or already-ready
text does not queue a second job. Clearing the field calls `edit("")` and
`cancel`. The field gates Enter during preedit and ignores repeated Enter;
real IME behavior remains unobserved.

Sources: [query.rs](../../src/translation/query.rs) (`normalize_submit`,
`looks_english`), [content.rs](../../src/rune_ui/translation/content.rs)
(`paste_clipboard`, `can_submit`, `on_change` → `translation::edit`,
query field `.label("Translation query")`).

Checks: `gates_empty_too_long_and_non_english`,
`does_not_truncate_overlong_input` (query.rs),
`submit_refuses_empty_and_overlong_text` (rune_ui/translation.rs),
`translation_submit_stays_off_for_an_empty_query` (panes.rs),
`translation_controls_have_names` (accessibility_tests.rs; query field
name is non-empty).

## Word and phrase (TR-04, TR-05)

```ts
submit_local                                              // TR-04 / TR-05
├── exact ECDICT key (lowercase, including multiword "take off")
│   -> Word: headword, phonetic or "No phonetic", pos/gloss, source Dictionary
├── single token miss -> lemma.en.txt -> Word, mapped_from, source DictionaryLemma
│   (IPA is the lemma's, never invented for the inflected form)
├── still a miss
│   ├── single token -> Apple TranslationSession as a Word, source System, no IPA
│   └── phrase -> Phrase: original + Chinese, source System, no sentence IPA
└── dictionary missing
    ├── single token -> DictionaryMissing + Install / Translate online…
    └── phrase -> still try the system session
```

Phonetics come only from ECDICT's `phonetic` field. Empty stays empty; the UI
shows `No phonetic`. Compatible and system word results never fill IPA.
Senses split ECDICT `n.` / `vt.` / `[计]` lines when present; unstructured
glosses stay one sense. Copy gloss copies the visible senses; copy
translation copies the Chinese line.

Sources: [query.rs](../../src/translation/query.rs) (`route`, `senses`),
[service.rs](../../src/translation/service.rs) (`run_local`, `system_outcome`),
[dictionary.rs](../../src/translation/dictionary.rs).

Checks: `exact_multiword_beats_phrase_and_lemma_maps_a_miss`,
`splits_pos_lines_and_keeps_unstructured_gloss`,
`never_invents_a_blank_phonetic` (query.rs),
`csv_parser_keeps_quoted_commas_and_newlines`,
`convert_roundtrip_preserves_phonetic_absence` (dictionary.rs),
`local_lookup_generation_and_cancel_drop_stale_results` (service.rs; in-memory
fixture dictionary, not the pinned 66 MB CSV),
`translation_word_scene_shows_headword_ipa_and_lemma_source`,
`translation_phrase_scene_shows_the_chinese_line` (panes.rs; fixture scenes).
Not observed: hello / world / hex / run / gave against the installed ECDICT
on a Mac, or a human check of a negated technical sentence.

## Speech (TR-06)

```ts
Speak British English / Speak US English                  // TR-06
├── buttons only for voices AVSpeechSynthesisVoice.speechVoices()
│   reports as en-GB / en-US; other accents are dropped
├── reads the word headword or the phrase original, never the gloss
├── Normal or Slow (synthesizer rate; Slow is default × 0.5)
├── one utterance; repeat the active button, Stop speaking, new query,
│   Lab off, or hide -> stop
├── no installed en-GB/en-US -> Spoken Content settings copy; no download
└── Hex recording (dictation / assistant / meeting) -> set_audio_busy
    stops speech and refuses a new start
```

Speech does not open the microphone. Voice identifiers are not saved.
`TranslationSettings` persists only the shortcut. Normal/Slow lives in a
process-wide `PREFERRED_RATE` atomic shared by pane and popup; it does not
survive restart. `voices_only_label_english_accents` enumerates whatever
this process sees; it does not prove audible output or offline synthesis.
AVFAudio exposes no `prefersOnDeviceSpeech` flag here.

Sources: [apple.rs](../../src/translation/apple.rs),
[apple_translation.swift](../../src/apple_translation.swift),
[speech.rs](../../src/rune_ui/translation/speech.rs) (`voices_row`,
`PREFERRED_RATE`),
[recording_environment.rs](../../src/audio/recording_environment.rs)
(`AudioHolds` serializes count + `set_audio_busy`; dictation
`CaptureMarker` and meeting `idle_sleep::prevent_sleep` hold/release).

Checks: `voices_only_label_english_accents` (service.rs),
`translation_controls_have_names` (accessibility_tests.rs),
`recording_notification_cannot_overtake_a_delayed_release` and
`capture_marker_holds_and_releases_translation_audio`
(recording_environment.rs).
Pending: native audible speech, missing-voice recovery, recording-interrupts-
speech on a real Mac, offline synthesis.

## Local resources (TR-07)

```ts
Application Support/hex/translation                       // TR-07
├── missing -> Install dictionary (user click only)
│   ├── net::download ECDICT git 39cfb9672c0906b63569e0453c2e6abfb9ab182a
│   │   ecdict.csv 66009548 sha256 d1ecbdf9ee2b920afb6b430ca6b6bb9f559588cf…
│   │   lemma.en.txt 2318694 sha256 e255b097404e3e0052060e2ddf6e15a1414f57…
│   ├── convert + durable write dictionary.tsv / lemma.tsv / receipt.json
│   └── Cancel install; retry after Failed
├── ready -> ~770611 entries (receipt.entries); no ecdict.mini.csv
└── Apple TranslationSession(installedSource:en, target:zh-Hans)
    ├── macOS 26.4+ preferredStrategy lowLatency
    ├── macOS 26.0–26.3 session without that strategy
    ├── earlier / no Translation framework -> SystemUnsupported
    ├── LanguageAvailability .supported or !isReady -> NotInstalled
    │   -> Language & Region settings + Recheck; Hex cannot request download
    └── ready -> UI-less translate, 30 s bound
```

Hex never auto-installs the dictionary. Closing Labs does not delete the
shared system language pack or system voices. Dictionary files stay under
the support folder. Credit: [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md)
(ECDICT, MIT). Launch does not pre-warm translation; `configure(enabled)`
and a background `Recheck` refresh status.

Earlier survey on the development Mac (macOS 27.2, not re-run here):
`LanguageAvailability` en→zh-Hans was `supported` with `isReady=false` and
`canRequestDownloads=false`; compact voices were Samantha (`en-US`) and
Daniel (`en-GB`). That is a resource snapshot, not a passing translation.

Sources: [dictionary.rs](../../src/translation/dictionary.rs)
(`DOWNLOAD_BYTES` = CSV + lemma = 68_328_242),
[app_paths.rs](../../src/app_paths.rs) (`translation_dir`),
[apple_translation.swift](../../src/apple_translation.swift),
[fixtures.rs](../../src/rune_ui/translation/fixtures.rs) (missing scene
`bytes: 68_328_242`).

Checks: `checksum_rejects_a_wrong_size` (dictionary.rs). Pending: a real
pinned download, receipt verify, and offline `translate` of one word and one
sentence. This pass did not mutate Application Support or settings.

## Compatible API (TR-08)

```ts
Translate online…                                         // TR-08
├── prepare_online -> CompatiblePreview {generation, text, base_url, model, host}
│   shown as service host, model, exact current text
├── Send -> confirm_online only if preview still matches generation+text+URL+model
├── request: dedicated "English into Simplified Chinese" prompt + that text
│   (not OpenCode, not dictation PROTOCOL_PROMPT, not labs.compatible_rewrite,
│   not compatible.selected)
├── credential from CompatibleRewriteSettings path; empty URL/model ->
│   Open compatible API settings (Output / Modes); configuring does not resend
└── timeout / fail -> no automatic retry; PrepareOnline again
```

`configure` copies `dictation_processing.default_mode.post_processing.compatible`
(`base_url`, `model`, `credential_path`, `deadline_seconds`) even when the
compatible-rewrite Lab is off. The preview is the only payload. Results are
`Source::Compatible` with no IPA.

Sources: [compatible.rs](../../src/translation/compatible.rs),
[app_settings/translation.rs](../../src/app_settings/translation.rs)
(`apply_translation`), [content.rs](../../src/rune_ui/translation/content.rs)
(`preview_card`).

Checks: `preview_requires_service_and_model_and_captures_exact_text`,
`prompt_is_not_the_dictation_protocol` (compatible.rs),
`configure_follows_compatible_settings_with_the_rewrite_lab_off`
(app_settings/translation.rs; pure `engine_config`, no process singleton),
`confirm_online_sends_only_the_captured_preview` (service.rs; loopback HTTP,
not a production model). Pending: a real compatible-API send of the previewed
text only.

## Isolation, races, presentation (TR-10 … TR-12)

```ts
generation fence                                          // TR-10
├── edit / new submit / cancel / Lab off -> generation++
├── stale local or online completion is dropped
└── Lab off -> popup hides; speech stops; snapshot text cleared

recording and product isolation                           // TR-11
├── translation never opens the microphone
├── AudioHolds (serialized count + notify) -> set_audio_busy
│   from dictation CaptureMarker and meeting idle-sleep guard
├── query/gloss/audio never enter History or Paste Last
└── compatible send does not start the assistant or OpenCode

pane + popup                                              // TR-12
├── shared content component; fixture scenes word/phrase/loading/missing/error
├── query, paste, submit, speak, recover named for accessibility
└── Mac light/dark fixtures inspected; native layout pending; Linux goldens waived
```

Sources: [service.rs](../../src/translation/service.rs) (`invalidate`,
`current`, `already_in_flight`, `bump` → `notify_changed`),
[translation.rs](../../src/translation.rs) (`set_waker`),
[macos_rune.rs](../../src/macos_rune.rs) (`wake_translation` wakes main +
popup; service callbacks never wait for the UI),
[accessibility_tests.rs](../../src/rune_ui/accessibility_tests.rs),
[scenes.rs](../../src/rune_ui/scenes.rs) (`mac-translation-*`).

Checks: `local_lookup_generation_and_cancel_drop_stale_results` (TR-10 local),
`confirm_online_sends_only_the_captured_preview` (stale preview rejected),
`translation_controls_have_names`,
`translation_loading_says_looking_up`,
`translation_missing_offers_install_and_online`,
`translation_error_offers_online_only`.
No `tests/golden` PNG for `mac-translation-*` exists in this checkout.

## Recovery

```ts
LookupError.recovery
├── DictionaryMissing -> Install dictionary, Translate online…
├── SystemNotReady -> Recheck, Open Language & Region, Translate online…
├── SystemUnsupported / SystemUnavailable -> Translate online…
├── CompatibleUnconfigured -> Open compatible API settings
└── CompatibleFailed / TimedOut -> Translate online…
```

The resource row always offers Recheck, About the dictionary (ECDICT GitHub),
Install when missing/failed, and Cancel while installing. Dictionary absence
does not hide speech once a result exists; missing IPA does not block speech.
Errors are distinct from a stuck "Looking up…": pending phases show that
status, then Ready with an outcome or error.

Shared dictation recovery lives in [recovery.md](recovery.md). Translation
does not use that capture state machine.

## Settings and CLI

`labs.translation` (bool, default false), `translation.hotkey` (default
Control-Option-T on macOS). `hex labs on|off translation`,
`hex settings set labs.translation`, `hex settings set translation.hotkey`.
`translation.hotkey` is unavailable off macOS. Conflict checks include
dictation, assistant, Paste Last, raw/second dictation, cancel, Annotate,
screenshots, and translation, both ways, only while the Lab is on.

`TranslationSettings` stores only `hotkey`. Voice ID is not saved. Speech
rate is a first-version process-wide atomic in
[speech.rs](../../src/rune_ui/translation/speech.rs), shared by pane and
popup, and does not survive restart.

## Status of proof

Delivery branch: `codex/translation` in personal fork `zj-linjie/hex`.
Source revision: `17119e5` (following `8fb409e` and `a13f07d`). Delivery is
through a PR to `dev`; the user explicitly requested no merge. The installed
app, user Application Support, settings, and `production` are unchanged.

On 2026-11-09 the user explicitly ended Linux testing and golden collection
and requested macOS delivery. Linux verification and the repository's
Linux-generated golden update are waived for this delivery. No translation
PNG was added to `tests/golden`; this is not a passing Linux result.

| Layer | Implemented in source | Observed |
| --- | --- | --- |
| Labs gate, pane, popup, shortcut wiring | yes, including fallback tap | source inspection and isolated tests; no physical key / native popup |
| Engine API, routing, generation fence, preview match | yes | Mac tests below; HTTP confirmation uses loopback |
| ECDICT pinned install | code + notices | full app install / 770611-entry load: unverified |
| Apple TranslationSession | Swift bridge macOS 26+ | typecheck passed; offline translate unverified |
| AVSpeechSynthesizer en-GB/en-US | bridge + UI buttons | audible speech unverified |
| Compatible API explicit send | preview + `generate` | real cloud send unverified |
| Rune presentation | five `mac-translation-*` scenes | ten Mac fixture frames inspected at `8fb409e`; Linux goldens waived |
| Native two-window / IME / Spaces | code | unverified |
| Merge / installed copy | outside delivery | not performed |

Architect-executed Mac checks:
`cargo test --locked --offline --bin hex translation -- --test-threads=8`
→ 69 passed on delivery source `17119e5` (includes query accessibility, all language catalogs, and shortcut pairing).
Separate `recording_notification_cannot_overtake_a_delayed_release` passed.
The three `macos_rune::translation_shortcut::tests` passed, covering setup
independence, deferred key-pair handoff, and synthetic-release isolation.
`cargo clippy --locked --offline --bin hex --tests -- -D warnings`,
`cargo fmt --all --check`, and `git diff --check` passed. Swift typecheck
and a Mac debug build also passed during integration.

Earlier at `8fb409e`, the translation filter had two failures (Russian
assistant catalog strings and the query accessible name); both were fixed.
Ten fixture frames (five scenes × light/dark) were inspected on that debug
binary. They are fixture data, not a native popup or a real dictionary.

Review findings about audio hold publication, resource wakeups, setup-time
shortcut availability, and singleton test isolation were corrected. Fresh
targeted reviews approved the final shortcut key-pair correction and the
three-file compatibility correction. These source reviews do not establish
physical shortcut, native audio, or real IME behavior.

Historical Linux attempts at `8fb409e` and `a13f07d` encountered compile
errors. Those errors were corrected in the delivery source, but Linux
success is not claimed. The remaining task runs
[42587268210](https://github.com/zj-linjie/hex/actions/runs/42587268210) and
[42588064058](https://github.com/zj-linjie/hex/actions/runs/42588064058) were
cancelled at the user's request; no further Linux work is part of delivery.

Known gaps versus the PRD, not hidden:

- `TranslationSettings` stores only `hotkey`. Shared in-process speech rate
  is the first-version limit.
- Popup Esc cancels an online preview before hiding; active preedit takes
  precedence. Unit checks exist; no native IME observation.
- System UI-less translation is macOS 26+, not macOS 15 (the PRD already
  records that floor).
