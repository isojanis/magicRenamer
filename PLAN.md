# Project Handover: MagicRenamer

## 1. Project Overview

**MagicRenamer** is a lightweight, cross-platform desktop application designed to triage and automatically rename poorly named archived files (e.g., `download (1).pdf`). It achieves this by reading the file's metadata and content locally, and optionally passing a text snippet to a local LLM to generate a highly descriptive, standardized filename (e.g., `YYYY-MM-DD_Topic.extension`).

**Key Constraints:**

- **Privacy choice:** Parsing stays local. Ollama inference stays local; cloud inference sends the prepared document excerpt and naming context to the selected provider.
- **Portability:** Provide validated release packages for Windows x64, Windows ARM64, Linux DEB, Linux RPM, macOS Apple Silicon, and macOS Intel.

## 2. Technical Stack

- **Framework:** Tauri (v2)
- **Backend:** Rust
- **Frontend:** HTML, CSS, JavaScript (Vanilla) + Vite
- **AI Integration:** Hybrid — Local Ollama REST API (`localhost:11434`) + Cloud AI APIs (OpenAI, Anthropic, Google Gemini, OpenRouter / OpenAI-compatible)


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
3. `suggest_filename_with_ollama(...) -> Result<FilenameSuggestion, String>`: Uses the selected provider for a naming suggestion. The command name remains for compatibility.
4. `rename_file(old_path: String, new_path: String) -> Result<String, String>`: Safely renames the file on the local filesystem with collision handling.
5. `get_prompt_config() -> Result<PublicPromptConfig, String>`: Loads nonsecret settings and key-present flags.
6. `save_prompt_config(config: PromptConfigUpdate) -> Result<PublicPromptConfig, String>`: Persists settings to `config.json` and saved keys to ignored `config.local.json`.

## 4. Frontend Architecture (UI/UX)

The UI handles the visual queue and triage workflow.

**Layout: Three-Pane Design**

- **Left Column (Queue):** A scrollable list of files loaded from a dropped/selected folder, displaying names and visual status badges (Pending, Processing, Ready, Renamed, Failed).
- **Center Column (Triage Zone):** Displays the active file's details: original name, editable input for AI suggested new name, extracted text snippet, and approve/skip/regenerate buttons.
- **Right Column (Preview & Details):** Metadata card (file name, path, extension, size, status) and extracted preview box.

**Interaction Flow:**

- Native directory picker or drag-and-drop folder selection.
- Batch processing using `Promise.allSettled()` with status transitions. The Process Queue button stops a run, cancels active AI requests, and leaves unfinished files pending.
- "Approve All Ready" master button.
- Keyboard shortcut support: `Enter` to approve suggestion and advance to the next ready file.
- Header model selector matching the active provider.

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
- [x] Name collision & overwrite protection (auto-append `_1`, `_2` if destination exists).
- [x] Prompt configuration file (`config.json`) & in-app settings modal with temperature slider.
- [x] Resizable UI columns with persistent widths and dark-mode styling fixes.
- [x] MIT License added and verified.
- [x] Folder-Context Awareness Intelligence (Enabled by default):
  - [x] Folder summary reports local file counts and date-prefix observations without treating the whole folder as one document category.
  - [x] For each document, use up to eight sibling names with the same document kind and meaningful subject overlap.
  - [x] Use matching siblings for consistent wording, including a shared invoice label when one label is clearly dominant.
  - [x] Extract issue dates from source content and enforce a leading full date in the final filename.
  - [x] Reject generic suggestions, retry once, and show a failed state when no usable name is produced.
  - [x] Send the configured temperature and structured JSON format to Ollama.
  - [x] Add `folder_context_enabled: true` toggle in `config.json` and UI Settings modal.
  - [x] Smart excerpts per file format, with a confirmed document date added when it appears beyond the excerpt.
  - [x] Large folder pre-flight warning (>500-1,000 files): Alert user if folder context is enabled, warning that local LLMs may struggle with excessive breadth/variability, and offer one-click toggle to disable context for the run.
  - [x] Pass 2 Context-aware Ollama prompt with structured JSON output (`new_name`, `category`, `reason`).
- [x] Original filename preservation:
  - [x] Implement extended attributes (`xattrs` on Linux/macOS, ADS on Windows) for zero-mutation local metadata.
  - [x] Implement visible timestamped `rename_history_YYYYMMDD_HHMM.json` manifest generation on rename batch.
  - [x] Add "Undo Rename" capability reading from `rename_history_*.json` / `xattr`.
- [ ] Launch application in dev mode (`npm run tauri dev`) or test binary directly.
- [x] Add a Linux GitHub Actions pipeline for DEB and RPM builds, tests, package metadata and license checks, checksums, and downloadable workflow artifacts.
- [ ] Build and test release packages for each target listed below. Store finished packages in the matching `releases/v<version>/` subfolder.
  - [ ] Windows x64 (`windows-x64/`)
  - [ ] Windows ARM64 (`windows-arm64/`)
  - [ ] Linux DEB (`linux-deb/`)
  - [ ] Linux RPM (`linux-rpm/`)
  - [ ] macOS Apple Silicon (`macos-apple-silicon/`)
  - [ ] macOS Intel (`macos-intel/`)

### Phase 5: Multi-Provider Cloud AI API Support (OpenAI, Gemini, Anthropic, OpenRouter) 🔄 (In Progress)
- [x] Provider abstraction in Rust backend (`query_ai_provider` dispatcher):
  - [x] Local Ollama (`http://localhost:11434/api/generate`)
  - [x] OpenAI (`api.openai.com/v1/chat/completions` — e.g. `gpt-5.4-mini`, `gpt-5.6-terra`)
  - [x] Google Gemini (`generativelanguage.googleapis.com/v1beta/models/...:generateContent` — e.g. `gemini-2.5-flash`, `gemini-2.5-pro`)
  - [x] Anthropic Claude (`api.anthropic.com/v1/messages` — e.g. `claude-haiku-5-5`, `claude-sonnet-5-5`)
  - [x] OpenRouter / Generic OpenAI-compatible endpoints (Groq, Together, DeepSeek, LocalAI)
- [x] Secure API key management in backend:
  - [x] Stored locally in Git-ignored `config.local.json` with owner-only permissions on Unix; no external analytics/tracking.
  - [x] Environment variable fallbacks supported (`OPENAI_API_KEY`, `GEMINI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`).
- [x] UI Provider & Model Selector in Settings modal:
  - [x] Dropdown for active Provider (`Ollama (Local)`, `OpenAI`, `Google Gemini`, `Anthropic`, `OpenRouter / Custom`).
  - [x] Dynamic model suggestions per selected provider, with an editable model ID.
  - [x] API Key input with password-masking and "Test Connection" status indicator.
  - [x] Custom endpoint URL field for local/self-hosted or proxy deployments.
- [x] Fallback & Offline Graceful Handling:
  - [x] If Ollama is not installed or running, guide the user to start Ollama or configure a cloud provider.
- [ ] Live provider and packaged-desktop smoke checks with user-provided credentials and platform dependencies.


---

## 6. Original Filename Preservation & History Tracking

To preserve the provenance and original filename of renamed documents without risking document corruption or modifying raw file bytes, MagicRenamer uses a dual-layer strategy:

### Layer 1: Filesystem Extended Attributes (`xattr` / ADS) — Zero-Mutation Local Storage
- **Mechanism:** When a file is renamed, MagicRenamer attaches an extended attribute to the target file:
  - **Linux / macOS (`xattr`):** `user.magicrenamer.original_name = "original_filename.ext"`
  - **Windows (NTFS):** Alternate Data Stream (ADS)
- **Integrity Guarantee:** Does not alter a single byte of the file itself. Cryptographic hashes, digital signatures (e.g. signed PDFs), and binary structures remain 100% intact.
- **Limitation:** Extended attributes do not survive standard cloud storage uploads (Google Drive, Dropbox, OneDrive) or file transfers across non-compatible filesystems (FAT32/exFAT).

### Layer 2: Visible Timestamped Manifest File (`rename_history_YYYYMMDD_HHMM.json`) — Universal & Select-All Friendly
- **Naming Pattern:** `rename_history_YYYYMMDD_HHMM.json` (e.g., `rename_history_20261014_2028.json`).
- **No Leading Dot:** By deliberately avoiding a leading dot (not a hidden file), it is visibly present in file managers (Nautilus, Dolphin, Windows Explorer, macOS Finder).
- **Batch Drag & Select-All Compatibility:** When you press `Ctrl+A` / `Cmd+A` to drag, upload, or sync files to Google Drive or Dropbox, the manifest is selected along with the documents rather than being skipped by hidden-file filters.
- **Timestamped per Session/Batch:** If multiple renaming batches occur in the same folder over time, each session produces a distinct, non-destructive historical record without clobbering previous batches.

```json
{
  "version": 1,
  "session_timestamp": "2026-10-14T20:28:00Z",
  "mappings": [
    {
      "current_name": "2026_Budget_Plan.json",
      "original_name": "temp_export_1012.json",
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "renamed_at": "2026-10-14T20:28:00Z"
    }
  ]
}
```

- **Cloud Survivability & Restore:** When re-downloaded elsewhere, MagicRenamer can inspect the folder, detect `rename_history_*.json`, and map every single file back to its history or perform an undo operation.
- **Key Advantages:**
  - **Universal & Visible:** Easily included when selecting all files to upload or move.
  - **Zero File Mutation:** Documents remain 100% clean and unaltered.
  - **Immutable History:** Timestamped filenames provide an audit trail across multiple triage sessions.

---

## 7. Folder context and naming validation

Folder context is enabled by default and can be disabled in Settings or for a large run. A selected folder can contain unrelated document kinds. Its summary reports only basic observations, such as file count and existing date prefixes.

For each target document, the app extracts a compact text excerpt and looks for a full document date. It distinguishes invoice issue dates from due dates when labels are present. Dates in YYYY-MM-DD, DD.MM.YYYY, English month-name, and Finnish month-name forms are supported. The excerpt includes a confirmed date even when it occurs later in the document.

When context is enabled, sibling filenames are considered only if they have the same recognizable document kind and at least two meaningful subject tokens in common with the target. Up to eight matching names are sent to Ollama. An unrelated receipt, report, or contract does not set an invoice's terminology. If matching invoices clearly favor one of the equivalent labels Lasku or Invoice, that wording is used for the target.

Ollama receives the configured temperature and a JSON response schema. The app then validates and normalizes each suggestion. A confirmed date is placed at the start in YYYY-MM-DD form; field separators become underscores; the source extension is retained. Generic names such as download, document, and a date plus Invoice alone are rejected. The app asks Ollama once more for a specific name, then marks the item failed with an explanation if it still cannot produce one.

The large-folder warning remains available when more than 500 files are discovered. The user can disable context for that run without changing the saved setting.

---

## 8. Multi-Provider Cloud AI Architecture

To support users who do not run local LLMs or lack dedicated GPU hardware, MagicRenamer provides a unified AI client layer supporting both local models and popular cloud AI APIs.

### 1. Supported providers

| Provider | Default request endpoint | Suggested model |
| :--- | :--- | :--- |
| Ollama (local) | `http://localhost:11434/api/generate` | `qwen2.5-coder:7b` |
| OpenAI | `https://api.openai.com/v1/chat/completions` | `gpt-5.4-mini` |
| Google Gemini | `https://generativelanguage.googleapis.com/v1beta/models/...:generateContent` | `gemini-2.5-flash` |
| Anthropic Claude | `https://api.anthropic.com/v1/messages` | `claude-haiku-5-5` |
| OpenRouter / Compatible | `https://openrouter.ai/api/v1/chat/completions` | `openrouter/auto` |

The model field accepts a custom ID. OpenAI and OpenRouter use a Chat Completions request; Gemini uses `generateContent`; Anthropic uses Messages; Ollama uses `/api/generate`. The prepared excerpt and naming context follow the selected provider's request format. Responses all pass through `finalize_suggestion` for name validation.

### 2. Configuration files

`config.json` holds the active provider, model, temperature, prompts, folder-context toggle, and optional full custom request URL. It remains compatible with older flat configuration files. Saved API keys are in ignored `config.local.json` beside it. Older inline keys are migrated into this companion file. Environment variables provide a fallback for each provider. The frontend receives key-present flags, never key values.

### 3. Privacy and endpoint safeguards

- The app shows a cloud badge when a cloud provider is active. Ollama inference stays local. Cloud inference sends the prepared excerpt, original filename, and optional naming context to the chosen provider.
- Remote custom endpoints require HTTPS; HTTP is allowed only for localhost. URLs with credentials, query strings, or fragments are rejected. Redirects are disabled so request credentials and document text stay on the selected endpoint.
- Gemini uses an `x-goog-api-key` header. API keys and provider error bodies are excluded from user-visible errors. API key files use owner-only permissions on Unix.
- Test Connection sends a small synthetic prompt to the selected provider. A live cloud request may incur a small charge. Desktop smoke checks and live provider checks remain to be done outside the headless test suite.
