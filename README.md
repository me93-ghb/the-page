# The Page

The Page is a local Markdown journal for macOS on Apple Silicon.

## Develop

Install Node.js 24, Rust through rustup, and the Xcode command-line tools. Then run:

```sh
npm ci
npm run tauri dev
```

Choose a folder, type `**A quiet afternoon.**`, and press Command-S. Quit and reopen:
the formatted text and its original timestamp remain. A pause of at least 30 minutes
adds a timestamped fold when you next write at the end. Corrections do not reset
that clock. The journal day starts at 04:00; uninterrupted writing stays on its page
until a pause. Reopening or reactivating the app selects the current journal day.
A blank page creates no file until you write or add a label.

## Verify

```sh
npm run verify
```

This checks Svelte/TypeScript, editor behavior, Rust storage, and the production
frontend build. Native keyboard, clipboard, folder selection, and window appearance
also need testing in the app at 560×400 and 1200×800, in light and dark modes.

To make a local debug app bundle:

```sh
npm run tauri build -- --debug --bundles app
```

## Files and limits

The chosen folder contains `YYYY/YYYY-MM-DD.md`. Front matter records the page ID,
creation/update times, and end-input clock. The generated date heading and initial
session markers preserve the original Markdown, local times, and UTC offsets.
Unknown metadata is preserved.
The folder preference is stored in the app's macOS Application Support directory.

Autosave runs one second after input, on blur/hide, on quit, and when macOS
supplies a sleep notification. Saves first checkpoint writing, its exact base
version, and image data in `~/Library/Application Support/The Page/recovery/`.
Each journal has one atomic JSON checkpoint keyed by page ID, containing pending
Markdown, metadata, and image bytes. A rollover updates both pages in that same
checkpoint. Local image caches support recovery while the journal is unavailable.
Older individual page records remain on disk after migration; the journal checkpoint
then controls which writing is pending. Checkpoint writes include all pending pages,
so large unsaved journals require more disk work.

Failed saves retry on input, Command-S, and every 30 seconds. The notice says
whether recovery succeeded. Keep the window open when writing exists only in
memory; ordinary quit requires durable journal/recovery data or explicit discard.
Force quit or power loss before the next checkpoint can lose new input.
Relaunch restores recovered writing using its recorded base, not file timestamps.

Clean pages reload external edits on activation. Overlapping edits preserve the
external bytes in unique `.conflict.md` copies before replacing the daily page.
A change during replacement retains the displaced file and reports its location.
Atomic swaps retain hidden `.the-page-*.md` predecessors because other editors may
still hold those files open. These files and image caches are not automatically
removed, so storage grows with use. Conflict copies stay outside the daily flow.
File > Save a Copy exports Markdown with its photographs; cancelling keeps all
writing and recovery state intact.

Scroll upward to load earlier pages in groups of eight. Historical corrections stay
in their original files and keep their session times. The Go menu provides Today
(Command-T) and Previous/Next Page (Command-Option-Up/Down). Today always selects
the current journal day, including when saved pages have later dates after travel.
Navigation stops when you start scrolling, typing, or using the pointer.

On a blank page, press Tab to choose Blank, Morning, Evening, or Letter with the
arrow keys and Return. Command-Shift-L edits the page label. Paste or drop a PNG
or JPEG to insert a photograph; edit its caption beneath the image. Photographs
retain their original bytes in a dated sibling folder. Backspace removes a figure
as one edit, and undo restores it; removing a reference never deletes its file.
Images are limited to 32 MB, 16,384 pixels per side, and 256 MB of decoder allocation.
Remote images and symbolic-link paths are rejected. Pending imports are included
in recovery. If recovery also fails, their bytes remain in memory for retry.

This version implements writing, resume, session folds, journal-day transitions,
continuous history, templates, labels, photographs, recovery, and conflict copies.
Select writing and press Command-Shift-P (File > Press Line…) to compose a card.
Save Image… writes a 1200×2000 PNG; Copy puts the same image on the clipboard.
Cards include the page date, initial time, and label. Selections over 160 characters
are shortened at a word boundary, with a visible notice. Escape closes the card
and restores the selection; exporting leaves the journal unchanged.
The app uses bundled fonts and makes no network requests. Folder syncing is external.

Alegreya and IBM Plex Mono ship under the SIL Open Font License; the notices are in
`public/licenses/` and the app bundle's `Contents/Resources/licenses/` directory.
