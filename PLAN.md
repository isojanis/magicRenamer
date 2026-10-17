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
- [ ] Standalone release packaging (`npm run tauri build`).

### Phase 5: Multi-Provider Cloud AI API Support (OpenAI, Gemini, Anthropic, OpenRouter) 🔄 (In Progress)
- [x] Provider abstraction in Rust backend (`query_ai_provider` dispatcher):
  - [x] Local Ollama (`http://localhost:11434/api/generate`)
  - [x] OpenAI (`api.openai.com/v1/chat/completions` — e.g. `gpt-4o-mini`, `gpt-4o`)
  - [x] Google Gemini (`generativelanguage.googleapis.com/v1beta/models/...:generateContent` — e.g. `gemini-1.5-flash`, `gemini-2.0-flash`)
  - [x] Anthropic Claude (`api.anthropic.com/v1/messages` — e.g. `claude-3-5-haiku`, `claude-3-5-sonnet`)
  - [x] OpenRouter / Generic OpenAI-compatible endpoints (Groq, Together, DeepSeek, LocalAI)
- [x] Secure API key management in backend:
  - [x] Stored locally in `config.json` with zero external analytics/tracking.
  - [x] Environment variable fallbacks supported (`OPENAI_API_KEY`, `GEMINI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`).
- [ ] UI Provider & Model Selector in Settings modal:
  - [ ] Dropdown for active Provider (`Ollama (Local)`, `OpenAI`, `Google Gemini`, `Anthropic`, `OpenRouter / Custom`).
  - [ ] Dynamic model list per selected provider with fast/cost-effective defaults (e.g. `gpt-4o-mini`, `gemini-1.5-flash`, `claude-3-5-haiku`).
  - [ ] API Key input with password-masking and "Test Connection" status indicator.
  - [ ] Custom endpoint URL field for local/self-hosted or proxy deployments.
- [ ] Fallback & Offline Graceful Handling:
  - [ ] If Ollama is not installed or running, detect it and guide user to either start Ollama or configure a cloud API key.


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

### 1. Supported Providers

| Provider | Endpoint Format | Default Fast/Cheap Model | Key Capabilities |
| :--- | :--- | :--- | :--- |
| **Ollama (Local)** | `http://localhost:11434/api/generate` | `qwen2.5-coder:7b` | Privacy-first, zero cost, 100% offline |
| **OpenAI** | `https://api.openai.com/v1/chat/completions` | `gpt-4o-mini` | High quality, structured JSON schema output |
| **Google Gemini** | `https://generativelanguage.googleapis.com/v1beta/...` | `gemini-1.5-flash` / `gemini-2.0-flash` | Extremely fast, cost-effective, long context |
| **Anthropic** | `https://api.anthropic.com/v1/messages` | `claude-3-5-haiku` | Exceptional reasoning & structured output |
| **OpenRouter / Custom** | `https://openrouter.ai/api/v1/chat/completions` | `deepseek/deepseek-chat` | Universal gateway to hundreds of open/commercial models |

### 2. Configuration Schema (`config.json`)

The configuration file is expanded to support provider selection and per-provider settings without exposing keys in plain sight:

```json
{
  "provider": "ollama",
  "model": "qwen2.5-coder:7b",
  "temperature": 0.2,
  "folder_context_enabled": true,
  "system_prompt": "You are a specialized file-naming assistant...",
  "prompt_template": "...",
  "providers": {
    "ollama": {
      "base_url": "http://localhost:11434"
    },
    "openai": {
      "api_key": "",
      "base_url": "https://api.openai.com/v1"
    },
    "gemini": {
      "api_key": ""
    },
    "anthropic": {
      "api_key": ""
    },
    "openrouter": {
      "api_key": "",
      "base_url": "https://openrouter.ai/api/v1"
    }
  }
}
```

### 3. API Key Security & Privacy Safeguards
- **Zero Remote Storage:** API keys are stored only on the user's local disk inside `config.json` (or encrypted in OS Keyring / environment variables). Keys are never transmitted anywhere except directly to the selected provider's official endpoint via HTTPS.
- **Privacy Notice:** When switching from `Ollama (Local)` to a cloud provider, the UI clearly displays a small privacy badge indicating that snippets will be sent over HTTPS to the selected cloud service.
- **Unified Output Handling:** All providers feed through the exact same response sanitizer and validation pipeline in Rust (`finalize_suggestion`), ensuring consistent date formatting, separator normalization, collision protection, and generic name rejection regardless of the underlying LLM engine.

