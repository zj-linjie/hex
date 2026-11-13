# Transfers

The macOS Labs inbox for text, images and files explicitly queued in Hex
Mobile. The `Transfers` Lab (`transfers`, `labs.transfers`, Everyone, History
group) is off by default. While enabled, Transfers appears immediately after
History. Linux and Windows have no pane or Lab. Turning it off keeps local
copies and stops import admission.

```ts
Share to Hex Mobile                         // transfers.receive
├── held → stays in the sync folder; Mac copies nothing
└── Sync to Mac → queued, complete schema 1 item
    ├── running Hex + CLI import → private local copy, pane refresh
    ├── same manifest UUID → already, no duplicate bytes
    ├── missing/versioned/partial attachment → refuse; no published half-copy
    └── Mac launch → scan default sync folder, skip receipts and placeholders

Open Transfers                              // transfers.inspect
├── list → kind or attachment count, title, UTC time
├── detail → note, selectable text, image preview when decodable
├── Copy / Copy image → clipboard changes only after this action
├── Show in Finder → reveal the local image or file
├── Send to Grok Bot → read enabled recipients, forward this local copy // transfers.grok
│   ├── one recipient → send immediately, show its name beside the button
│   ├── several recipients → one button per bot name; choose one
│   └── no receiving bot / handoff failure → error; no partial batch
└── Delete… → Confirm delete → remove this Mac's copy only
```

Local copies live under the existing app data root at
`transfers/<manifest UUID>/`, with directories 0700 and files 0600 on Unix.
They remain until deleted. Import validates all attachments before copying,
then publishes a completed staging directory atomically. It accepts at most
10 attachments, 100 MiB each and 200 MiB combined, rejects unsafe and duplicate
names and symlinks, and preserves the original attachment bytes and UTI. Text
is `body.txt`. No dictation History entry is created. Import never reads or
writes the clipboard and never logs payload text.

When an item is imported while Transfers is not the visible pane, Hex asks
macOS to deliver one local notification, with generic text. Notification
presentation remains subject to macOS notification settings. No APNs or
application network transport is involved. The default scan uses iCloud
Drive's `Hex Mobile/transfers` folder; a missing folder is an empty inbox.
Custom folders are imported only by the [CLI](../cli.md#mobile-transfers-macos-labs).
Hex does not write `receipt.json`; the external sync script owns the receipt
and the Mobile app's delivered summary and cloud cleanup.

The macOS-only Grok action reads Grok Bot Anywhere's
`~/Library/Application Support/Grok Bot Anywhere/ShareInbox/configuration.json`
only after a send request. Missing, unreadable, unsupported, or empty
configuration reports that no receiving bot is turned on and writes nothing.
Hex uses the chosen entry's account, bot, session, mode, and prefix; it ignores
avatar keys and never reads login credentials or launches a CLI/SDK.

The payload combines the note and UTF-8 `body.txt`, falling back to the title
when empty. Text is capped at 2,000,000 UTF-16 units and the title at 1,000;
`body.txt` is not duplicated as a file. Other attachments retain their safe
names and source copies; symlinks are refused. A private completed directory
is renamed into `ShareInbox/Ready/<fresh id>` for each send. Before publication,
failures clean staging and leave Ready unchanged. The version-1 manifest uses
the receiver's required `source: "wechat"` discriminator. Compose/direct behavior
belongs to the running receiving app; Hex's success means the batch was
published locally, not that the bot processed or delivered it. No network or
real account configuration is used by the automated tests.

Sources: `src/transfers/grok.rs`, `src/transfers.rs`, `src/transfers/macos.rs`,
`src/rune_ui/transfers.rs`, `src/control/cli/transfers.rs`, and
`src/macos_rune/control.rs`. English source strings and Simplified Chinese
entries follow `desktop::i18n`; other catalogs currently fall back to English
for the new strings.

## Verification and limits

`src/transfers/tests.rs` exercises held/no-copy, queued/copy-once, duplicate
import, schema 2, missing schema, missing attachment, size mismatch, unsafe
names, symlinks, owner-only modes, and deletion preserving the source copy.
Grok fixtures cover recipient selection, regular-file copying without source
deletion, fresh IDs, absent/invalid configuration with no Ready directory,
symlink refusal, body text without file duplication, and payload limits.
Executed results belong in the implementation handoff; the presence of these
tests alone is not a passing run.

Not device-verified: Notes/Photos/Files sharing, iCloud hydration and transfer
latency, launchd receipt round-trip, native selectable-text interaction,
image preview/copy for every supported UTI, Finder reveal, macOS notification
presentation, or dictation/keyboard/vocabulary regressions on the paired
phone and Mac, or Grok Bot Anywhere ingestion/composer activation. The preview uses Rune's decoder; an unsupported image displays
Preview unavailable and keeps Copy image and Finder available. The Transfers row and pane render only in macOS builds, so Linux and Windows goldens do not change.
