# Voice assistant (Labs, macOS)

The personal fork's optional assistant uses the existing Xiaozhi official
backend and its configured MCP bridge. Local dictation continues to use Hex's
local speech model. The assistant does not paste into another app or change
Paste Last, and contains no independent model/tool execution loop.

```ts
Labs > Voice Assistant [off by default]                  // assistant.opt-in
└── On -> Voice Assistant joins the sidebar (macOS only)
    ├── Import identity -> existing activated assistant state.json
    │   └── owner-only Application Support/hex/voice-assistant-identity.json
    ├── Hold to talk [Right Option; editable]            // assistant.capture
    │   ├── microphone already owned -> report busy; never steal it
    │   ├── hold -> shared start cue + visible Dot scope from the native microphone
    │   │   └── sound theme/volume and HUD placement/Reduce Motion follow dictation
    │   ├── valid release -> shared stop cue after restoring recording audio,
    │   │   then waiting animation; no pre-roll or release-tail audio is submitted
    │   ├── extra chord / short tap / silent / audio gap / cancel -> discard locally
    │   └── valid release -> paced 16 kHz mono Opus, manual listen
    ├── Start speaking / End and send -> accessible capture controls
    ├── transcription and reply -> bounded memory-only conversation
    ├── Play replies [on] -> official returned Opus audio // assistant.reply
    │   └── independent of recording cues; Sound volume = 0 mutes cues only
    │   └── off -> stop local playback; keep receiving text
    └── Cancel / next hold -> stop local playback, request backend abort
        └── isolate old connection; no guarantee of undoing a tool action

Left Option -> local dictation -> existing processing -> focused input
Right Option -> Xiaozhi -> configured MCP bridge -> existing Mac tools
```

The owner's migration changes the saved dictation binding from either Option
to left Option and the assistant to right Option, and enables this feature
explicitly. General installations retain the existing dictation setting and
leave the new feature off. Both shortcut recorders share side-aware conflict
checks with dictation, Paste Last, raw/second dictation and Annotate.

A microphone interruption, including one before the hold threshold, discards
the whole assistant capture. A detected timeline gap over the audio owner's
50 ms tolerance also discards it, so an incomplete instruction is never
submitted. Local dictation retains its existing recovery behavior.

The start cue uses dictation's retractable 40 ms chord grace when the microphone
stays ready. With Release microphone while idle, it waits for an intentional
hold and an open device. Short taps, extra-key chords and explicit cancellation retract that cue;
they never play the stop cue. A silent full-length hold can play the release
cue before its samples are rejected locally. An accepted release restores recording audio
before its stop cue, and does not reset an older dictation job's completion-tone
clock. The assistant start event opens and repositions a hidden HUD just as a
local dictation does, and clears any HUD Lab demo voice before real metering.
A cold start still owed at release plays before stop in one sequential source.
Delayed cues are invalidated by the next capture, including Annotate. The
official-reply switch controls returned speech independently of these cues.

The pane reports the voice connection separately from the bridge: a connected
voice session does not prove a bridge is running or an action succeeded. It
shows only the official transcription/reply events, not invented per-tool
progress. Tool effects still require observation of the bridge and target app.

The official protocol has no turn IDs. Completed turns can keep their
connection and context; interrupting replaces it to fence late audio/text.
Cloud context after replacement is not guaranteed. A failure never replays an
operation. Clearing a local display is not a cloud-memory reset.

## Sources and verification limits

- Settings and gates: `app_settings`, `labs`, `control/settings`.
- Input and timeline: `recognition`, `suppression`, `dictation/audio`.
- Protocol and playback: `voice_assistant`; pane: `rune_ui/voice_assistant`.
- Acceptance scenarios: [PRD VA-01–VA-14](../plans/voice-assistant-prd.md).
- Actual checks and installation evidence:
  [implementation record](../plans/voice-assistant-implementation.md).

Tests and fixture scenes are implementation checks. They do not establish
physical shortcut delivery, microphone capture, speaker output, official
service availability or actual MCP action completion. Those results must be
recorded separately after they are observed.

Observed on 2026-11-03: strict Clippy and 564 native selected tests passed;
official Xiaozhi probes returned STT, reply text and decodable Opus, and an
instruction was followed by an actual Calculator launch while the existing
bridge stayed running. The installed owner build `3.2.0-local.37c9524` showed
Connected and completed native Start recording/Cancel, retained settings and
permissions across restart, and passed the synthetic Qwen API regression.
See the implementation record for scope and evidence. Physical-key speech and
audible-speaker acceptance remain unobserved. Linux rendering/goldens were
explicitly waived by the owner for this delivery.

## Migration and recovery

Import only the existing activated **assistant** identity, not the old cloud
dictation identity. Do not commit identity files or put tokens in logs. Quit
the old `xiaozhi-voice` client before enabling Hex's connection with the same
identity; retain the independent MCP bridge. A missing identity or disconnected
session is visible in the pane; reconnect explicitly, then submit a new
instruction. Do not automatically repeat an action whose result is unknown.

Turning Labs off stops accepting assistant captures and local reply playback.
It retains the identity/settings for a later opt-in. Rollback restores the
verified previous application and saved configuration, then may restart the
old voice client; avoid two clients connecting with the same identity.
