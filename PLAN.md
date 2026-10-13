# Project Handover: MagicRenamer

## 1. Project Overview

**MagicRenamer** is a lightweight, cross-platform desktop application designed to triage and automatically rename poorly named archived files (e.g., `download (1).pdf`). It achieves this by reading the file's metadata and content locally, and optionally passing a text snippet to a local LLM to generate a highly descriptive, standardized filename (e.g., `YYYY-MM-DD_Topic.extension`).

**Key Constraints:**

- **Privacy-First:** All processing, parsing, and AI summarization must happen entirely on the local machine. No external API calls.
- **Portability:** The application must compile into a standalone executable for both Linux and Windows.

## 2. Technical Stack

- **Framework:** Tauri (v2)
- **Backend:** Rust
- **Frontend:** HTML, CSS, JavaScript (Vanilla) + Vite
- **AI Integration:** Local Ollama REST API (`localhost:11434`)

## 3. Backend Architecture (Rust/Tauri)

The Rust backend is responsible for file ingestion, text extraction, and coordinating with the local LLM. It acts as the bridge for the frontend UI.

**Core Dependencies (`Cargo.toml`):**

- `tauri` (v2)
- `tauri-plugin-dialog` (v2)
- `tauri-plugin-fs` (v2)
- `tauri-plugin-opener` (v2)
- `pdf-extract` (v0.7)
- `office_oxide` (v0.1)
- `reqwest` (v0.12 with `json` feature)
- `serde` & `serde_json`
- `walkdir` (v2.5)
- `tokio` (v1)

**Exposed Tauri Commands (`#[tauri::command]`):**

1. `scan_folder(folder_path: String) -> Result<Vec<DiscoveredFile>, String>`: Recursively scans a directory for supported document formats (`pdf`, `docx`, `xlsx`, `doc`, `xls`, `txt`, `md`, `csv`) with metadata.
2. `extract_document_snippet(file_path: String) -> Result<String, String>`: Determines the file extension, extracts text using the appropriate parser, and truncates it to 1000 characters.
3. `suggest_filename_with_ollama(text_snippet: String, extension: String, model: Option<String>) -> Result<String, String>`: Posts the text snippet and system prompt loaded from `config.json` to the local Ollama API to generate a standardized filename.
4. `rename_file(old_path: String, new_path: String) -> Result<String, String>`: Safely renames the file on the local filesystem with collision handling.
5. `get_prompt_config() -> Result<PromptConfig, String>`: Loads the current prompt template, system prompt, and model from `config.json`.
6. `save_prompt_config(config: PromptConfig) -> Result<(), String>`: Persists modified prompts and parameters to `config.json`.

## 4. Frontend Architecture (UI/UX)

The UI handles the visual queue and triage workflow.

**Layout: Three-Pane Design**

- **Left Column (Queue):** A scrollable list of files loaded from a dropped/selected folder, displaying names and visual status badges (Pending, Processing, Ready, Renamed, Failed).
- **Center Column (Triage Zone):** Displays the active file's details: original name, editable input for AI suggested new name, extracted text snippet, and approve/skip/regenerate buttons.
- **Right Column (Preview & Details):** Metadata card (file name, path, extension, size, status) and extracted preview box.

**Interaction Flow:**

- Native directory picker or drag-and-drop folder selection.
- Batch processing using `Promise.allSettled()` with status transitions.
- "Approve All Ready" master button.
- Keyboard shortcut support: `Enter` to approve suggestion and advance to the next ready file.
- Header model selector matching available local Ollama models.

## 5. Implementation Roadmap & Status

### Phase 1: Scaffolding & Initial Setup ✅
- [x] Tauri v2 project structure initialized with Vite + Vanilla JS.
- [x] Initial HTML/CSS three-pane responsive layout created.
- [x] Initial Tauri commands skeleton in `src-tauri/src/main.rs`.
- [x] README documentation created.

### Phase 2: Bug Fixes & Pre-Build Hardening ✅
- [x] Remove unused `[lib]` entry in `Cargo.toml`.
- [x] Fix `office_oxide` API usage to `Document::open(path)?.plain_text()` and version pin to `0.1`.
- [x] Clean up unused Rust imports (`PathBuf`).
- [x] Configure Tauri v2 capabilities (`src-tauri/capabilities/default.json`) for dialog and fs plugins.
- [x] Refactor frontend plugin imports (`@tauri-apps/plugin-dialog`) and isolate mock fallbacks.
- [x] Add standard `.gitignore` (node_modules, target, dist, icons).
- [x] Generate desktop application icons across required resolutions.

### Phase 3: Rust Toolchain & Local Verification ✅
- [x] Install Rust/Cargo toolchain on system via `rustup` (Rust 1.99.0).
- [x] Install native Linux GUI libraries (`webkit2gtk4.1-devel`, `gtk3-devel`, `libappindicator-gtk3-devel`, `librsvg2-devel`).
- [x] Verify `cargo check` and compile full debug binary (`cargo build` completed cleanly).
- [x] Test Ollama integration prompt against local `qwen2.5-coder:7b` (`2026-08-12_Invoice-Enterprise-GPU-Compute-Cluster-Usage.txt` returned).
- [x] Create test documents in `test_docs/` for interactive verification.

### Phase 4: Polish & Production Packaging 🔄 (Current Step)
- [ ] Name collision & overwrite protection (auto-append `_1`, `_2` if destination exists).
- [ ] Launch application in dev mode (`npm run tauri dev`) or test binary directly.
- [ ] Standalone release packaging (`npm run tauri build`).
