# The Page

The Page is a local Markdown journal for macOS on Apple Silicon.

## Develop

Install Node.js 24, Rust through rustup, and the Xcode command-line tools. Then run:

```sh
npm ci
npm run tauri dev
```

Choose a folder, type `**A quiet afternoon.**`, and press Command-S. Quit and reopen:
the formatted text and its original timestamp remain. The day starts at 04:00 in
local time. A blank page creates no file until you write.

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
session marker precede the original Markdown. Unknown metadata is preserved.
The folder preference is stored in the app's macOS Application Support directory.

Autosave runs one second after input. Failed writes retain unsaved text in memory.
External changes detected before replacement stop saving. Existing pages use an
atomic swap that retains the displaced file as a hidden `.the-page-*.md` sibling;
a change during replacement reports that file's location. These predecessors are
not automatically deleted, so storage grows with saves. New pages never replace
an existing file. A filesystem that cannot swap files safely reports a save error.
Keep the window open if saving fails; automatic recovery is not implemented, and
force quit or power loss can lose unsaved writing.

This version implements writing and resume only. Session folds, history navigation,
templates, labels, photographs, and card exports are not implemented yet.
The app uses bundled fonts and makes no network requests. Folder syncing is external.

Alegreya and IBM Plex Mono ship under the SIL Open Font License; the notices are in
`public/licenses/` and the app bundle's `Contents/Resources/licenses/` directory.
