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
- [x] Name collision & overwrite protection (auto-append `_1`, `_2` if destination exists).
- [x] Prompt configuration file (`config.json`) & in-app settings modal with temperature slider.
- [x] Resizable UI columns with persistent widths and dark-mode styling fixes.
- [x] MIT License added and verified.
- [x] Folder-Context Awareness Intelligence (Enabled by default):
  - [x] Pass 1 Rust Blueprint extractor: Scan sibling file names, detect date & naming patterns (`YYYY-MM-DD`, `snake_case`, `kebab-case`), read existing `rename_history_*.json`, and sample directory theme.
  - [x] Add `folder_context_enabled: true` toggle in `config.json` and UI Settings modal.
  - [x] Smart excerpting per file format (headers/schema/headings) keeping prompt within 200-500 tokens.
  - [x] Large folder pre-flight warning (>500-1,000 files): Alert user if folder context is enabled, warning that local LLMs may struggle with excessive breadth/variability, and offer one-click toggle to disable context for the run.
  - [x] Pass 2 Context-aware Ollama prompt with structured JSON output (`new_name`, `category`, `reason`).
- [ ] Original filename preservation:
  - [ ] Implement extended attributes (`xattrs` on Linux/macOS, ADS on Windows) for zero-mutation local metadata.
  - [ ] Implement visible timestamped `rename_history_YYYYMMDD_HHMM.json` manifest generation on rename batch.
  - [ ] Add "Undo Rename" capability reading from `rename_history_*.json` / `xattr`.
- [ ] Launch application in dev mode (`npm run tauri dev`) or test binary directly.
- [ ] Standalone release packaging (`npm run tauri build`).

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

## 7. Folder-Context Awareness Intelligence (Enabled by Default)

To enable small local LLMs (3B-8B parameters with 2k-4k token limits) to generate naming suggestions that harmoniously fit into existing project directories, MagicRenamer implements a **structured, two-pass pipeline**:

```
 ┌──────────────────────────────────────────────────────────────┐
 │ PASS 1: Directory Pre-Scan & Context Extraction (Fast Rust)   │
 │ - Group files by extension, topic, or date cluster.          │
 │ - Extract structural metadata & sample key terms.            │
 │ - Read existing rename_history_*.json manifest for priors.   │
 │ - Build a compact "Directory Blueprint" (150-300 tokens).    │
 └──────────────────────────────┬───────────────────────────────┘
                                │
                                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │ PASS 2: Context-Aware File Renaming Prompt (Local LLM)       │
 │ System Prompt: Directory Blueprint + Consistency Rules       │
 │ User Prompt: Target File Details + High-Signal Excerpt       │
 │ Output: Clean JSON: { new_name, category, reason }           │
 └──────────────────────────────────────────────────────────────┘
```

### 1. Default Configuration
- **Default Status:** **Enabled by default** (`folder_context_enabled: true` in `config.json`).
- **UI Toggle:** Checkbox in the Settings modal allowing users to disable context awareness if they want isolated file-only naming.

### 2. Pass 1: The Compact "Directory Blueprint"
Constructed instantly in Rust (<5ms) during directory scan or queue loading:
1. **Parent Directory Theme:** Parent directory name (e.g. `Project_Apollo_Invoices` or `Taxes_2026`).
2. **Detected Sibling Conventions:** Sample of existing sibling filenames to detect case styles (`kebab-case`, `snake_case`, `TitleCase`) and date formats (`YYYY-MM-DD`).
3. **Prior Renames:** Top rules/categories extracted from existing `rename_history_*.json` in the folder.
4. **Sequence Tracking:** Numerical indexing awareness (e.g. recognizing `invoice_003.pdf` so the next file is indexed consistently).

```yaml
Directory Context:
  Parent Folder: "Project_Apollo_Q3_Invoices"
  Detected Pattern: "YYYY-MM-DD_<Vendor>_<InvoiceNo>.<ext>"
  Sibling Files:
    - "2026-08-01_Acme_INV-101.pdf"
    - "2026-08-15_Acme_INV-102.pdf"
    - "2026-09-01_CloudHost_8921.txt"
  Known Categories: [Invoices, Receipts, Contracts]
```

### 3. Smart Excerpting (Token Budget Optimization)
Instead of dumping full contents, excerpts are tuned by file type to keep the per-file token budget around 50–150 tokens:
- **PDF / DOCX:** Title metadata, document header, first paragraph.
- **TXT / Markdown:** First 3 lines + headings + final line.
- **JSON:** Root schema keys & array lengths (omitting huge array payloads).
- **CSV:** Column headers + first 2 data rows.

### 4. Structured Output Format
When context awareness is enabled, the model returns JSON:
```json
{
  "new_name": "2026-10-02_Hetzner_Cloud_Server.txt",
  "category": "Receipt",
  "reason": "Matched existing date format (YYYY-MM-DD), detected vendor 'Hetzner', and aligned with expense naming pattern."
}
```
The UI uses `new_name` for the suggestion input and displays `category` and `reason` in the Preview & Details card as helpful AI rationale.

### 5. Large Folder Guardrail & User Warning (>500-1,000 files)
When scanning a directory with thousands of files:
- **Challenge:** A folder containing thousands of heterogenous documents risks creating an excessively noisy blueprint or confusing local small parameter LLMs (which excel at focused, local consistency but degrade with massive multi-topic diversity).
- **Guardrail Behavior:**
  - If the discovered file count exceeds a threshold (e.g., 500+ files) and `folder_context_enabled` is active, MagicRenamer displays an interactive warning banner/modal before processing starts:
    > ⚠️ **Large Folder Detected (X files)**: Folder-Context Awareness is currently enabled. Inferring conventions across very large, diverse directories may exceed the optimal capacity of local LLMs.
  - **User Choices:**
    1. **"Proceed with Context (Sampled)"**: Limits sibling sampling to the immediate subfolder or top 20 representative files.
    2. **"Disable Context for this Run (Recommended)"**: Seamlessly falls back to independent, file-only triage without mutating permanent user settings.




