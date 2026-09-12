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

Autosave runs one second after input. Failed writes retain unsaved text in memory.
External changes detected before replacement stop saving. Existing pages use an
atomic swap that retains the displaced file as a hidden `.the-page-*.md` sibling;
a change during replacement reports that file's location. These predecessors are
not automatically deleted, so storage grows with saves. New pages never replace
an existing file. A filesystem that cannot swap files safely reports a save error.
Keep the window open if saving fails; automatic recovery is not implemented, and
force quit or power loss can lose unsaved writing.

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
Remote images and symbolic-link paths are rejected. Failed imports retain pending
bytes in memory for retry; keep the window open until the import succeeds.

This version implements writing, resume, session folds, journal-day transitions,
continuous history, templates, labels, and photographs. Card exports and durable
recovery are not implemented yet.
The app uses bundled fonts and makes no network requests. Folder syncing is external.

Alegreya and IBM Plex Mono ship under the SIL Open Font License; the notices are in
`public/licenses/` and the app bundle's `Contents/Resources/licenses/` directory.
