# Wumpa

Wumpa is a fast, free, open-source PDF editor for macOS, Windows, and Linux.
Built with **Rust** (Tauri v2 + lopdf) and **PDF.js**. Works fully offline — your files never leave your machine.

## Features

- Open multiple PDFs in tabs, drag & drop files to open
- Smooth scrolling, zoom, fit-to-width, page thumbnails
- Select & copy text, search inside the document
- Merge PDFs, split into parts, extract pages
- Reorder pages (drag thumbnails), delete, rotate
- Highlight text and add sticky-note comments (saved into the PDF as real annotations)
- Shows highlights and comments already present in a PDF
- Object editor: click any text block, image or shape on a page to move, resize, or truly delete it; drag a rectangle to delete everything in an area
- Add free text anywhere (any language — rendered by the OS, so Arabic shaping is correct) and a hand-drawn signature, Fill & Sign style
- Edit existing text in place: double-click a line in edit mode, rewrite it, done
- Draw on pages: freehand ink, rectangles, ellipses, lines and arrows in six colors — saved as real PDF annotations
- Insert blank pages, duplicate pages, export any page as PNG (right-click a thumbnail)
- Print via the system viewer (⌘P), recent files on the welcome screen, remembers window size
- Opens password-protected PDFs for viewing
- Double-click any PDF in Finder/Explorer to open it (after installing)
- Undo / redo for all page operations
- Light & dark themes, keyboard shortcuts
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

## Development

Prerequisites: [Rust](https://rustup.rs), Node.js 20+.

```sh
npm install
npm run tauri dev    # run the desktop app
npm run dev          # UI-only in a browser (uses a bundled sample.pdf)
```

## Building installers

```sh
npm run tauri build
```

Outputs a `.app`/`.dmg` on macOS, `.exe`/`.msi` on Windows, `.AppImage`/`.deb` on Linux.
Tagged pushes (`v*`) build all platforms automatically via GitHub Actions.

## Architecture

- `src-tauri/` — Rust backend: file IO and all PDF restructuring (merge, split, reorder, rotate, delete, extract, highlight annotations) via [lopdf](https://github.com/J-F-Liu/lopdf)
- `src/` — React + TypeScript UI, rendering and text layer via [PDF.js](https://mozilla.github.io/pdf.js/)

## License

MIT
