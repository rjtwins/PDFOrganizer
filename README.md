# PDFOrganizer

PDFOrganizer is a desktop GUI for sorting PDFs by previewing their pages and moving each file into a named destination folder.

The app is written in Rust with [Iced](https://github.com/iced-rs/iced) for the interface, `pdfium-render` for PDF previews, and `rfd` for native folder pickers.

## Agentic coding

This project was completed fully with **agentic coding**.

The implementation, preview integration, tests, and documentation updates were all carried out through an agent-driven development workflow rather than manual step-by-step editing alone.

## What the app does

PDFOrganizer is built around a simple workflow:

1. Add one or more destination folders and give each one a short nickname.
2. Pick a folder that contains PDFs.
3. Review the rendered page previews for each PDF in that folder.
4. Move a PDF into one of the saved destinations.

This is useful for triaging downloads, scans, invoices, forms, or any other batch of PDFs that need manual sorting.

## How the app works

### 1. Destination management

The top section of the window is the **Destinations** panel.

- **Add Destination** opens a native folder picker.
- After you choose a folder, the app creates a temporary pending destination.
- You must enter a nickname before saving it.
- Nicknames must be unique.
- **Remove** deletes a saved destination from the current session.

Important behavior:

- Destinations persist across restarts.
- They are stored as JSON in the user's local app data folder under `PDFOrganizer\destinations.json`.
- The app resolves that folder with `dirs::data_local_dir()`.
- On Windows, that typically resolves to a path under `%LOCALAPPDATA%\PDFOrganizer\destinations.json`.
- Saved destinations are loaded automatically when the app starts.
- Adding or removing a destination immediately rewrites the JSON file.
- If the storage path cannot be resolved, or the JSON file cannot be read or parsed, the app starts with an empty destination list and shows the error in the destinations panel.
- The destinations list can be collapsed with **Hide Destinations** / **Show Destinations**.

### 2. Folder selection

Press **Select Folder** to choose the folder you want to organize.

When a folder is selected, the app:

- stores the selected path,
- scans that folder,
- keeps files whose extension is exactly lowercase `.pdf`,
- ignores non-PDF files.

Current limitations:

- The scan is **not recursive**; only the selected folder is checked.
- Files named with uppercase `.PDF` are currently not included.

### 3. PDF preview rendering

For every discovered PDF, the app creates a section in the main scrolling area.

Each section shows:

- the file name,
- a **Move To Destination** action,
- a scrollable preview area containing rendered pages.

While rendering is still in progress, the section shows **Rendering preview...**.

If rendering fails, the section shows:

- **Failed to render this PDF.**
- the underlying error message returned by PDFium.

Rendering details taken from the code:

- Every page of the PDF is rendered, not just the first page.
- The preview width is based on the current window width.
- Preview rerenders are debounced by 1 second after a resize, so the app does not immediately rerender on every resize event.
- Rendered pages are capped to a maximum height of 1080 pixels.
- Landscape pages are rotated to improve preview readability.
- Rendering is generation-based, so stale render results from an older resize are ignored.

### 4. Moving a PDF

Each PDF section has a **Move To Destination** button.

When you click it:

- if no destinations exist yet, the app tells you to add one first;
- otherwise, it expands a list of destination buttons in the format `nickname -> path`.

When a destination is chosen, the app tries to move the file into that destination folder.

Move behavior:

- The destination file name stays the same.
- If a file with the same name already exists in the destination, the move is rejected.
- The app first tries a filesystem rename.
- If rename fails, it falls back to copy-then-delete.
- If the move succeeds, the PDF is removed from the on-screen list immediately.
- If the move fails, the error is shown in the destinations panel.

## UI layout

The main window starts at **900 x 800** and is resizable.

The interface is organized into:

1. the destinations panel,
2. the folder selection controls,
3. the selected folder display,
4. the scrolling list of PDF preview sections.

## Project structure

```text
Cargo.toml          Dependencies and package metadata
build.rs            Downloads and stages the PDFium runtime library
src\main.rs         Iced application state, update loop, and UI
src\destination.rs  Destination management, JSON persistence, and file-moving logic
src\pdf_proc.rs     Folder scanning and PDF rendering
src\main_tests.rs   App-level tests
```

## Destination storage format

Destinations are stored in a JSON object with a `destinations` map keyed by nickname.

Example:

```json
{
  "destinations": {
    "Invoices": "C:\\Users\\You\\Documents\\Invoices",
    "Archive": "D:\\PDF Archive"
  }
}
```

This file is managed by the app. Editing it manually is possible, but invalid JSON will prevent the saved destinations from loading.

## Build requirements

You need:

- a recent Rust toolchain with Cargo,
- an internet connection on the first build so `build.rs` can download PDFium,
- a supported target platform for the bundled PDFium binaries:
  - Windows x64 / x86 / ARM64
  - Linux x64 / x86 / ARM / ARM64
  - macOS x64 / ARM64

On Windows, installing Rust through [rustup](https://rustup.rs/) is the easiest setup.

## How the build works

This project depends on the native PDFium library for rendering PDFs.

The application code also uses:

- `dirs` to locate the user's local app data directory,
- `serde` and `serde_json` to serialize and deserialize destination data,
- `rfd` for native folder pickers,
- `iced` for the GUI,
- `pdfium-render` for PDF preview generation.

During `cargo build`:

1. `build.rs` detects the current target OS and architecture.
2. It chooses the matching prebuilt PDFium archive.
3. It downloads that archive from the `bblanchon/pdfium-binaries` GitHub release.
4. It extracts the platform library:
   - `pdfium.dll` on Windows
   - `libpdfium.so` on Linux
   - `libpdfium.dylib` on macOS
5. It copies that library into the Cargo profile output directory so the built app can load it at runtime.

The downloaded library is cached under the target profile directory, so later builds can reuse it.

At runtime, the app first tries to load PDFium from the executable's directory. If it is not there, it falls back to a system-installed PDFium library.

## Running the app in development

```powershell
cargo run
```

This builds the project and launches the GUI.

## Building a release binary

```powershell
cargo build --release
```

On Windows, the main executable will be:

```text
target\release\PDFOrganizer.exe
```

The matching PDFium runtime library must stay next to the executable when you distribute or move the app.

## Running tests

```powershell
cargo test
```

The repository includes tests for:

- destination creation and validation,
- destination JSON persistence and reload behavior,
- move behavior,
- folder scanning,
- resize and rerender behavior,
- PDF rendering on a minimal generated PDF,
- UI construction for major states.

## Notes and current limitations

- Only files ending in lowercase `.pdf` are detected.
- The folder scan is not recursive.
- The app is designed for manual review and sorting, not automatic classification.
- Large or complex PDFs may take longer to render because every page is previewed.
- Destinations are persisted, but the currently selected source folder is not.

## Typical usage example

1. Launch the app.
2. Add destinations like `Invoices`, `Contracts`, and `Archive`.
3. Select a folder full of downloaded PDFs.
4. Inspect each preview.
5. Move each file into the correct destination.

That leaves the source folder progressively cleaned up while keeping the review process visual and manual.
