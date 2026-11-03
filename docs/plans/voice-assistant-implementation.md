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

Observed on 2026-11-03:

- `cargo check --locked --bin hex --tests` passed after integration.
- The first native affected-module run executed 563 tests: 560 passed. The
  missing Labs switch registration and missing Playground scene paths found
  there were corrected. A Mac comparison against a Linux capsule golden
  differed; no baseline images were changed.
- The final sandbox-compatible affected-module run passed all 561 selected
  tests. It excluded golden comparisons and three loopback-socket tests that
  the sandbox forbids (`PermissionDenied` at `TcpListener::bind`). Those three
  transport/mock/privacy tests passed in the earlier native run. The final
  source also compiles the normal Hex binary through `scripts/check`.
- Formatting, `git diff --check`, and code-shape checks passed.
- Independent transport/input/UI reviews found issues that were corrected;
  the fresh final runtime review reported no remaining material findings.
  The fresh final UI review found a CLI round-trip and fixture-status layout
  gap, both corrected. Its final follow-up was interrupted by service quota.
- Clippy's native build step is blocked inside the sandbox: Swift compiler
  cache access, then FoundationModels macro-plugin startup after using a
  temporary cache. This is not a completed Clippy pass.
- The automatic approval service subsequently exhausted its quota. A native
  test rerun requiring approval was not executed. Official-service probing,
  speaker/microphone/physical-key verification, signed release packaging,
  installation and migration have not run. No identity was imported, no
  runtime settings changed, and the installed application was not replaced.

The existing old voice client was not running at the preflight check; its
independent MCP bridge was running and was left untouched. The installed
`3.2.0-local.72231d1` application reported ready microphone/accessibility and
the selected Qwen model. These preflight observations do not establish the
new assistant's end-to-end behavior. The original Qwen prewarm changes remain
untouched in this diff.

Pending delivery steps: native protocol/audio and Qwen regression, signed
bundle using the unchanged designated requirement, verified data backup and
bounded shortcut/identity migration, installed-app checks, and duplicate-app
archive/Spotlight cleanup. Native end-to-end acceptance remains pending.
