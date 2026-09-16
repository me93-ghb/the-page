# The Page

A daily journal for macOS on Apple Silicon, for people who want a page to write on.
Open it to today's writing. Scroll up to read earlier days. Your writing lives in
Markdown files in a folder you choose.

Write `**A quiet afternoon.**` and the formatting appears as you type. Come back
after a break and a timestamped fold marks where you began again.

## Writing

Each day starts blank, with a date and time. You can also begin with a Morning,
Evening, or Letter template. On an empty page, press Tab to reach the templates,
then use the arrow keys and Return to choose one.

A pause of at least 30 minutes adds a fold when you next write at the end of the
current page. Editing earlier writing does not start a session or reset that
clock. Older pages remain editable, with their original dates and session times.

The journal day starts at 04:00 in your Mac's local time zone. Uninterrupted
writing continues on the same page until a qualifying pause. Reopening or
reactivating the app returns to the current journal day.

Use the sun and moon buttons to choose light paper or charcoal paper. Both have
a subtle grain across the page and title bar. The app remembers your choice.

Paste or drop a PNG or JPEG into a page, then write a caption beneath it.
Backspace removes a photograph as one edit; undo restores it. Removing it from
the page does not delete the image file.

Select a passage and choose **File > Press Line…** to make a card with its page
date, initial time, and label. Save or copy it as a 1200 × 2000 PNG. Selections
longer than 160 characters are shortened at a word boundary, with a notice.
Escape returns to your selection. Exporting does not change your journal.
Card exports use a flat background.

## Getting around

Earlier pages load as you scroll upward, eight at a time. Navigation stops when
you scroll, type, or use the pointer. Today returns to the current journal day,
even if travel has left pages with later dates in your folder.

| Action | Shortcut |
| --- | --- |
| Save now | ⌘S |
| Today | ⌘T |
| Previous / next page | ⌘⌥↑ / ⌘⌥↓ |
| Edit the page label | ⌘⇧L |
| Press a card | ⌘⇧P |
| Undo / redo | ⌘Z / ⌘⇧Z |

**File > Reveal in Finder** opens the journal location.
**File > Save a Copy…** exports Markdown with its photographs. Cancelling leaves
writing and recovery data intact.

## Your files

Pages are stored as `YYYY/YYYY-MM-DD.md`. Photographs retain their original bytes
in a dated folder beside the page. A blank page creates no file until you write
or add a label.

Front matter stores the page ID, creation and update times, and the last input
time used for folds. Date headings and session markers preserve the writing's
Markdown, local times, and UTC offsets. Unknown metadata is preserved. The journal
folder preference lives in the app's macOS Application Support directory.

The app runs locally without accounts or network requests. Fonts and paper texture
are bundled. There is no built-in sync; you can sync the journal folder yourself.

### Saving and recovery

Autosave runs one second after input, when the app loses focus or is hidden, on
quit, and when macOS supplies a sleep notification. Failed saves retry on input,
⌘S, and every 30 seconds.

Before saving, the app checkpoints pending writing and photographs in
`~/Library/Application Support/The Page/recovery/`. The save notice tells you
whether that recovery copy succeeded. If writing exists only in memory, keep
the window open. Ordinary quit requires a saved journal, a recovery copy, or an
explicit discard. Force quit or power loss before the next checkpoint can lose
new input.

Pages without unsaved changes reload external edits when the app becomes active.
Overlapping edits preserve the external version in a unique `.conflict.md` copy
before replacing the daily page. A change during replacement also retains the
displaced file and reports its location. Conflict copies do not appear in the
journal flow.

<details>
<summary>Storage details and limits</summary>

Each journal has one atomic JSON recovery checkpoint, keyed by page ID. It holds
pending Markdown, metadata, image bytes, and the exact base version used to detect
conflicts. A day rollover updates both pages in that checkpoint. Recovery uses
that recorded base rather than file timestamps. Local image caches support
recovery while the journal folder is unavailable. If recovery also fails, pending
imports remain in memory for retry.

Checkpoint writes include all pending pages, so large unsaved journals require
more disk work. Older per-page recovery records remain after migration; the
journal checkpoint determines which writing is still pending.

Atomic saves retain hidden `.the-page-*.md` predecessors because another editor
may still have them open. These files and image caches are not automatically
removed, so storage grows with use.

Photographs are limited to 32 MB, 16,384 pixels per side, and 256 MB of decoder
allocation. Remote images and symbolic-link image paths are rejected.

</details>

## Build from source

The app uses Svelte, TypeScript, and CodeMirror, with Tauri and Rust for desktop
integration and file storage. macOS on Apple Silicon is the current target;
other platforms have not been tested.

Install Node.js 24, Rust through rustup, and the Xcode command-line tools. From
this repository, run:

```sh
npm ci
npm run tauri dev
```

To build a local debug app:

```sh
npm run tauri build -- --debug --bundles app
```

The bundle is written to `src-tauri/target/debug/bundle/macos/The Page.app`.

Run the checks with:

```sh
npm run verify
```

This checks Svelte and TypeScript, runs editor and Rust tests, and builds the
frontend. Native keyboard input, clipboard access, folder selection, and window
appearance still need an app pass at 560 × 400 and 1200 × 800 in both themes.

## Bundled assets

Alegreya and IBM Plex Mono use the SIL Open Font License. Paper 001 by ambientCG
is CC0. Notices are in [`public/licenses/`](public/licenses/) and the app bundle's
`Contents/Resources/licenses/` directory.
