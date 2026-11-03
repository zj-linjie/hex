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

Signed bundle installation and bounded identity/shortcut migration are the next
owner-machine step. The owner-only installation backup records the actual
installed version, hashes, unchanged signing requirement, data preservation,
runtime status, API regression and duplicate-bundle cleanup. Physical-keyboard,
live microphone and audible speaker acceptance must remain separate from the
protocol and fixture results above.
