# MagicRenamer ✨

MagicRenamer is a lightweight, cross-platform desktop application designed to triage and automatically rename poorly named archived files (e.g., `download (1).pdf`). It extracts text and metadata locally and queries a local LLM (via Ollama) to propose descriptive, standardized names (`YYYY-MM-DD_Topic.extension`).

## Features

- **Privacy-First:** All text extraction and LLM inference take place locally on your machine.
- **Local Document Ingestion:** Extracts text from PDFs (`pdf-extract`), Office files (`office_oxide`), and plain text formats.
- **Local AI Triage:** Generates suggested names via Ollama (`localhost:11434`).
- **Three-Pane Workflow:** Interactive queue, triage approval zone, and document preview.

---

## Prerequisites

Before building or running MagicRenamer, ensure you have the following installed on your system:

### 1. Node.js & npm
- Node.js (v18+ recommended)
- Verify with:
  ```bash
  node -v
  npm -v
  ```

### 2. Rust & Cargo
- Install Rust using `rustup`:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  source $HOME/.cargo/env
  ```
- Verify with:
  ```bash
  cargo --version
  rustc --version
  ```

### 3. System Dependencies (Linux)
Tauri on Linux requires webkit2gtk and development libraries:

- **Debian / Ubuntu / Pop!_OS:**
  ```bash
  sudo apt update
  sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```
- **Fedora / RHEL:**
  ```bash
  sudo dnf install -y webkit2gtk4.1-devel openssl-devel curl wget libappindicator-gtk3-devel librsvg2-devel
  ```
- **Arch Linux:**
  ```bash
  sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget openssl appmenu-gtk-module libappindicator-gtk3 librsvg
  ```

### 4. Ollama (Local AI Engine)
MagicRenamer connects to Ollama running locally at `http://localhost:11434`.
1. Install [Ollama](https://ollama.com/).
2. Pull the model used for suggestions (e.g., `qwen2.5-coder:7b` or `llama3:8b`):
   ```bash
   ollama pull qwen2.5-coder:7b
   ```
3. Ensure the Ollama service is running:
   ```bash
   ollama serve
   ```

---

## Configuration & Prompt Tuning

Prompt settings are managed through [`config.json`](file:///home/isojanis/development/magicRenamer/config.json) in the project root:

```json
{
  "system_prompt": "You are a specialized file-naming assistant. Analyze the document snippet and return ONLY a single standardized filename in format: YYYY-MM-DD_Topic.ext (or Topic.ext if no date found). Do not include quotes, explanation, or markdown. Use alphanumeric characters, hyphens, and underscores only.",
  "prompt_template": "Target extension: .{extension}\nDocument content snippet:\n\"\"\"\n{text_snippet}\n\"\"\"\nSuggested filename:",
  "model": "qwen2.5-coder:7b",
  "temperature": 0.2
}
```

You can adjust prompts in two ways:
1. **Directly in `config.json`**: Edit with any text editor at any time. Changes apply on subsequent triage runs.
2. **From the UI**: Click the **"Prompt Settings ⚙️"** button in the header bar to open the in-app configuration editor.

---

## Getting Started

### 1. Install Frontend Dependencies
From the repository root:
```bash
npm install
```

### 2. Run in Development Mode
Starts the Vite dev server and launches the Tauri desktop application with hot reload:
```bash
npm run tauri dev
```

---

## Tests

Run the frontend tests with Node's built-in test runner:

```bash
npm test
```

Run the headless Rust tests, including fixture generation:

```bash
npm run test:rust
```

Run both suites with `npm run test:all`. The Rust suite does not require a running Ollama server; the optional live-model smoke test is ignored by default.

---

## Building & Packaging

To compile standalone desktop bundles for distribution:

```bash
npm run tauri build
```

The compiled binaries and installers will be placed under:
- **Linux:** `src-tauri/target/release/bundle/deb/` or `appimage/`
- **Windows:** `src-tauri/target/release/bundle/msi/` or `nsis/`

---

## Project Structure

```text
magicRenamer/
├── PLAN.md              # Architectural specification & design document
├── TEST_PLAN.md         # Feature coverage plan
├── README.md            # Setup, development, and packaging instructions
├── package.json         # Node scripts & Tauri CLI tooling
├── index.html           # Three-pane UI markup
├── style.css            # Application styling
├── main.js              # Frontend triage controller & keyboard handlers
├── tests/               # Frontend workflow tests
└── src-tauri/           # Rust Tauri application and backend tests
    ├── Cargo.toml       # Backend crate dependencies
    ├── build.rs         # Tauri build hook
    ├── tauri.conf.json  # Tauri app and window configuration
    └── src/
        ├── main.rs      # Tauri startup and command registration
        ├── config.rs    # Prompt settings and persistence
        ├── documents.rs # Folder scan and document excerpts
        ├── naming.rs    # Dates, folder context, and filename rules
        ├── ai.rs        # Suggestions and provider requests
        ├── rename.rs    # Safe rename, undo, and history manifests
        ├── test_support.rs # Shared Rust test fixtures and temp folders
        └── tests/       # Rust tests grouped by feature
```

---

## License

This project is licensed under the MIT License - see the [LICENSE](file:///home/isojanis/development/magicRenamer/LICENSE) file for details.

