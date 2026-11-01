# Wumpa

Wumpa is a fast, free, open-source PDF editor for macOS, Windows, and Linux.
Built with **Rust** (Tauri v2 + lopdf) and **PDF.js**. Works fully offline — your files never leave your machine.
Full English/Arabic UI with complete right-to-left support.

## Features

**Viewing & navigation**
- Open multiple PDFs in tabs, drag & drop files to open, double-click a PDF in Finder/Explorer
- Smooth scrolling, zoom, fit-to-width, page thumbnails, side-by-side page compare with visual diff
- Select & copy text, search inside the document, clickable in-page links and QR codes
- Bookmarks panel (read and edit the PDF outline)

**Editing**
- Merge PDFs, split into parts, extract pages, compress with quality/size control
- Reorder pages (drag thumbnails), delete, rotate, insert blank pages, duplicate pages
- Object editor: click any text block, image or shape to move, resize, or truly delete it; drag a rectangle to delete everything in an area
- Edit existing text in place: double-click a line in edit mode, rewrite it, done
- Add free text anywhere (any language — rendered by the OS, so Arabic shaping is correct), insert images and tables, create new blank PDFs
- Highlight text and add sticky-note comments, draw freehand ink/rectangles/ellipses/lines/arrows — all saved as real PDF annotations
- Hand-drawn signature and stamps, Fill & Sign style
- Generate and scan QR codes
- Export any page as PNG (right-click a thumbnail)
- Undo / redo for all page operations

**Image to PDF (on-device OCR)**
- Detects pages that are really just a scanned image and rebuilds them as real, searchable, selectable text — including tables with borders and embedded pictures — into a new copy of the file
- Recognition runs fully on-device (Apple Vision on macOS, Arabic + English and more); Windows/Linux support is planned

**Export to Word & Excel**
- Converts a PDF (text or scanned) into an editable `.docx` with the original layout, fonts, tables and pictures kept
- Converts tables into a real `.xlsx` workbook: typed numbers, alignment, merged cells, formatting — one sheet per page plus a clean sheet per table

**Security**
- Password-protect PDFs (AES-256) with separate open/permission passwords, remove existing protection
- Built-in guessing protection: password strength meter, escalating lockouts after wrong attempts, and an opt-in policy to wipe the local copy after repeated failed attempts

**Experience**
- Print via the system viewer, recent files on the welcome screen, remembers window size
- Light & dark themes, hover tooltips everywhere, keyboard shortcuts
- 100% local, no internet required, PDF-only by design

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Open / Save / Save As | ⌘O / ⌘S / ⇧⌘S |
| Find | ⌘F |
| Undo / Redo | ⌘Z / ⇧⌘Z |
| Zoom in / out / reset | ⌘+ / ⌘− / ⌘0 (or ⌘ + scroll) |
| Close tab | ⌘W |
| Delete selected pages | Delete |
| Note tool | N |
| Edit objects / Delete area | E / R |
| Add text / Signature | T / S |
| Draw | D |

(Use Ctrl instead of ⌘ on Windows/Linux.)

## Installing

Grab the latest build for your platform from the [Releases page](https://github.com/4iShmmri/wumpa/releases):
a `.dmg` for macOS, an `.exe`/`.msi` installer for Windows, or an `.AppImage`/`.deb` for Linux.

Builds are not code-signed or notarized, so your OS may warn before the first launch
(macOS: right-click the app → Open; Windows: "More info" → "Run anyway" on the SmartScreen prompt).

## Development

Prerequisites: [Rust](https://rustup.rs), Node.js 20+.

```sh
npm install
npm run tauri dev    # run the desktop app
npm run dev          # UI-only in a browser (uses a bundled sample.pdf)
```

On macOS, the on-device OCR helper (`tools/ocr/`) needs a one-time build before `Image to PDF` works in a dev build:

```sh
./tools/ocr/build.sh
```

## Building installers

```sh
npm run tauri build
```

Outputs a `.app`/`.dmg` on macOS, `.exe`/`.msi` on Windows, `.AppImage`/`.deb` on Linux.
Tagged pushes (`v*`) build all platforms automatically via GitHub Actions.

## Architecture

- `src-tauri/` — Rust backend: file IO and all PDF restructuring (merge, split, reorder, rotate, delete, extract, annotations, encryption) via [lopdf](https://github.com/J-F-Liu/lopdf); `scan/` (OCR page analysis), `compose/` (PDF page composition with embedded fonts and Arabic shaping), `export/` (Word/Excel writers)
- `src/` — React + TypeScript UI, rendering and text layer via [PDF.js](https://mozilla.github.io/pdf.js/)
- `tools/ocr/` — macOS on-device text-recognition helper (Swift + Vision), built as a sidecar binary

## Contributing

Issues and pull requests are welcome. Before opening a PR, please make sure `npm run lint`,
`npm run typecheck`, `npm test`, and `cargo test`/`cargo clippy` (in `src-tauri/`) all pass.

## License

MIT — see [LICENSE](LICENSE).
