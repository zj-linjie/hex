# Xiaozhi assistant implementation

Date: 2026-11-03 (Asia/Shanghai). The implementation authorization supersedes
the planning-only stage recorded in the [PRD](voice-assistant-prd.md).
The PRD remains the accepted behavior and acceptance-scenario reference.

The owner explicitly waived Linux rendering/golden work for this delivery on
2026-11-03. Verification and installation therefore target macOS; Linux UI
goldens are not regenerated in this change, and Linux support is not claimed.

## Ownership

All work uses the existing `codex/voice-assistant-prd` checkout, based on `dev`.
The pre-existing roadmap and PRD are retained. Concurrent workers have separate
write scopes and must preserve each other's changes.

- Main integrator: integration decisions, feature maps, delivery evidence,
  independent-review acceptance, Linux golden collection and visual inspection,
  personal-fork PR, installation, configuration preservation and rollback.
- `assistant_transport` (Executor): `src/voice_assistant.rs`,
  `src/voice_assistant/`, and Cargo dependency/lockfile changes. Native official
  WebSocket/Opus protocol, bounded conversation, identity import, playback and
  interruption; no model or MCP execution loop.
- `assistant_input` (Executor): `src/recognition.rs`, `src/recognition/`,
  `src/suppression.rs`, `src/suppression/`, `src/dictation/audio.rs` and its
  children, and the indicator seam if needed. Exclusive timeline capture,
  gesture validation, local buffering, physical-event boundaries and real meter.
- `assistant_ui` (Executor): settings, Labs, control settings, Rune pane/hosts,
  platform parity, fixture scenes and translations. The root module declaration
  is its only change in `src/main.rs`.
- Independent Reviewer: read-only review of the coherent integrated diff for
  correctness, omissions, resource/credential boundaries and justified
  simplification. Any resulting fixes receive a fresh review.

## Chosen first-version defaults

- Labs remains off by default for installations generally. The owner's explicitly
  requested migration is a separate installation-time settings change.
- Hold binding: right Option; dictation migration: left Option. Both are
  editable and share conflict checks with existing shortcuts.
- Official reply playback starts on, matching the previous assistant; muting
  playback never suppresses reply text.
- Conversation is bounded and memory-only. No assistant audio is retained.
- Audio stays local until the complete gesture is accepted. Sending uses the
  official manual-listen protocol and 60 ms, 16 kHz mono Opus frames.
- The official protocol has no per-turn identifier. Ordinary completed turns
  keep their connection; interruption replaces the connection to fence late
  text/audio. Cloud context after that replacement is not guaranteed.
- A transport failure never replays an operation. Abort requests cannot undo
  an operation already sent to the existing MCP bridge.

## Verification record

Observed on 2026-11-03, before installation:

- `cargo check --locked --bin hex --tests` and strict
  `cargo clippy --locked --bin hex --tests -- -D warnings` passed.
- The affected-module native regression passed all 564 selected tests,
  including the loopback WebSocket protocol, interruption and synthetic-token
  privacy tests. Golden comparisons were excluded under the owner's waiver.
  Formatting, `git diff --check`, and code-shape checks passed.
- The opt-in official-service probe used synthetic Mandarin saying to answer
  only and use no tools. It received STT, reply text, decodable official Opus
  audio and TTS stop; the successful run took 12.704 seconds.
- A second official probe said to open the system Calculator. It received all
  four response signals in 12.627 seconds (hello at 7.191 s; upload finished
  at 9.778 s). Calculator was absent before and a new process appeared after.
  The original bridge remained PID 23783. This is observed application effect,
  not a per-tool audit trail; the bridge does not expose one to Hex.
- The real Qwen/Metal prewarm regression passed in 6.032 seconds using the
  installed Qwen3-ASR 0.6B model, saved vocabulary and existing synthetic
  Mandarin fixture. The prior vocabulary-safe prewarm fix remains intact.
- macOS native-GPU renders of the dark reply and light missing-identity scenes
  were inspected: both show separate left/right Option bindings and the
  expected controls. These are fixtures, not microphone or physical-key proof.
- Independent transport/input/UI reviews led to fixes for cancel after release,
  preview isolation, shortcut recovery, audio gaps, late replies and privacy.
  Fresh final runtime and follow-up validation reviews reported no remaining
  material findings. The latter also checked CLI sided-key round-trip and
  generic preview isolation.

The old voice client was not running at preflight; its independent MCP bridge
was left running. The installed `3.2.0-local.72231d1` application reported ready
microphone/accessibility and the selected Qwen model. No identity import or
runtime setting migration was needed for the protocol probes, which read the
original assistant identity without printing it.

## Installed owner build

`/Applications/Hex.app` is now `3.2.0-local.37c9524`, built from the merged
code commit `37c9524095706fd4ca63618cabdae6f04193b6ba` (PRs #2 and #3 in
the personal fork). The driver, helper and application were re-signed with the
previous installed Apple Development identity. Designated requirement and
TeamIdentifier matched exactly; deep strict signature verification passed.
The binary has no Homebrew or dynamic Opus runtime dependency. Automatic
updates remain off for this local build.

The old app exited before a verified data backup. The owner migration changed
only `dictation_hotkey`, `voice_assistant` and `labs.voice_assistant`: left
Option local dictation, right Option assistant, assistant enabled and official
reply playback on. The original activated identity was copied with mode 0600;
its source was unchanged. All unrelated settings, 62 vocabulary entries,
personal configuration/script, model files and retained history were preserved.
The generated script-status file changed only from stopped to active.

After normal restart, the installed app reported the same bindings, listener,
Qwen model and ready microphone/accessibility permissions. Its authenticated
local transcription API returned HTTP 200 and correct synthetic Mandarin.
The native assistant pane showed Connected, separate left/right Option keys,
and a working Start recording → Recording → Cancel sequence. No End and send
was invoked during that microphone check.

Both the previous signed bundle and the new build-output bundle were zipped,
extracted, compared by file hash/mode/link and code-signature verified before
their redundant bundle directories were removed and unregistered. Spotlight's
only existing `anomaly.hex` app was `/Applications/Hex.app`. The owner-only
installation backup holds `verification-report.json`, `MAINTENANCE.md`, data
snapshots and evidence, including rollback archive hashes. No identity or user
configuration was published.

Physical-keyboard → live speech → tool interaction and audible-speaker
acceptance remain unobserved. Protocol probes, synthetic host audio, native
button capture/cancel and fixture renders do not replace those human checks.


## Recording feedback follow-up (2026-11-03)

Scope: fix right-Option recording feedback to reuse local dictation's sounds
and Dot scope without changing the official backend, MCP bridge, shortcuts,
identity, or microphone/audio submission boundaries. Linux rendering/goldens
remain waived by the owner for this same delivery.

Write ownership in the existing `codex/assistant-recording-feedback` checkout:

- `assistant_feedback` (Executor): recognition/assistant capture feedback,
  shared feedback requests, audio-owner release callback, indicator and HUD
  start/show/placement gates, plus focused regressions in those modules.
- Main integrator: dependency-pin synchronization required by AGENTS, feature
  maps, validation/evidence, personal-fork PR, signed installation, data
  preservation, archive verification and rollback instructions.
- Independent Reviewer: read-only correctness/omission and scoped simplify
  review of the completed diff; the shared workspace must preserve others' work.

Implementation: warm-microphone starts request the existing provisional cue;
cold starts wait for the existing intentional/ready admission. Discards and
cancels retract it. Assistant release uses the audio owner's zero-tail finish
and recording-environment restoration callback before the shared stop cue,
without changing the dictation completion clock. Cold readiness is claimed
once by recognition or release; a start still owed at release is sequenced
before stop in one sound source. Delayed assistant cues are fenced at the
restoration callback, playback queue, output-open completion and each sample
when any new dictation/programmatic/assistant or Annotate capture starts.
`AssistantStarted` uses the
same native overlay show/placement gate as `Started` and clears HUD Lab demo
metering. Reply playback remains a separate existing setting.

Rune is synchronized from `c8f2888` to dev `1213159` as required by the project.
The `## dev` changelog is unchanged between these commits; the 17 commits
include macOS hidden-panel ordering and renderer/runtime changes. No Hex API
adaptation was identified before compilation. Cargo pins and the Nix output
hash move together; the recursive source hash calculation was checked against
the old pin's known hash before calculating the new one. Nix itself is not
installed on this Mac.

Observed before installation:

- Strict `cargo clippy --locked --bin hex --tests -- -D warnings` passed.
- `scripts/check` / nextest passed all 501 selected feedback, audio-owner,
  recognition, indicator/HUD, shortcut, assistant and Rune UI checks. These
  include cold ready-at-release once-claim, sequential start/stop samples,
  delayed restoration across a new dictation or Annotate, output-open and
  playing-source generation fences. The HUD-specific added check classifies
  start events; it does not instantiate a native overlay.
- The initial broader run passed 497/498 checks; its meeting-capsule
  `panel_goldens` failed on macOS `call-ended-light` (4629 pixels). The retained
  old-pin (`c8f2888`) test harness reproduced that exact fixture/count, so the
  final selected run excludes that existing local baseline mismatch. No golden
  was rewritten. Linux goldens remain waived; neither a Linux/Windows build
  nor a Nix build was run. Shared assistant-only feedback entries carry the
  existing non-macOS dead-code annotation pattern.
- Formatting, code-shape and diff checks passed. Debug linking warned that its
  exception-unwind section exceeds the compact-unwind encoding limit; it did
  not fail the build or tests.

Native panel recording/cancel can establish hidden-overlay presentation without
sending microphone audio. Physical keys, spoken-wave response, audible cues
and the valid-release cloud path still require deliberate human acceptance;
no automatic environmental recording is submitted for this follow-up.
