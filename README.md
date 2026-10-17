# MagicRenamer ✨

MagicRenamer is a lightweight, cross-platform desktop application designed to triage and automatically rename poorly named archived files (e.g., `download (1).pdf`). It extracts text and metadata locally, then queries Ollama or a selected cloud AI provider to propose descriptive, standardized names (`YYYY-MM-DD_Topic.extension`).

## Features

- **Provider choice:** Text extraction stays local. Ollama keeps AI inference local; cloud providers receive the document excerpt and naming context used for a suggestion.
- **Local Document Ingestion:** Extracts text from PDFs (`pdf-extract`), Office files (`office_oxide`), and plain text formats.
- **AI Triage:** Generates suggested names via Ollama (`localhost:11434`), OpenAI, Google Gemini, Anthropic Claude, or OpenRouter-compatible APIs.
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

### 4. AI provider
Ollama is the default and runs locally at `http://localhost:11434`. It is optional when you choose a cloud provider.
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

Open **Settings ⚙️** to choose Ollama, OpenAI, Google Gemini, Anthropic Claude, or OpenRouter / Compatible. The model field offers provider-specific suggestions and also accepts a model ID you enter. The header menu selects the model used for the next suggestion. Use **Test Connection** to send a small test request before processing documents; this may incur provider charges.

Set the provider API key in the masked field or through `OPENAI_API_KEY`, `GEMINI_API_KEY`, `ANTHROPIC_API_KEY`, or `OPENROUTER_API_KEY`. Leaving the field blank keeps the saved key. The checkbox removes the saved key for the selected provider. A custom endpoint accepts HTTPS, or HTTP on localhost, for Ollama and OpenAI-compatible providers. Enter the full request URL, such as `http://localhost:11434/api/generate` or `https://example.com/v1/chat/completions`.

Prompt and model settings live in `config.json`, and API keys live in `config.local.json`, under the per-user `MagicRenamer` settings directory: `$XDG_CONFIG_HOME/MagicRenamer` (or `~/.config/MagicRenamer`) on Linux, `~/Library/Application Support/MagicRenamer` on macOS, and `%APPDATA%\MagicRenamer` on Windows. The app no longer reads settings from its working directory. To bring forward settings from an earlier version, open **Settings → Import Previous Settings** and select the old `config.json`. The app imports its adjacent `config.local.json` when present. Review the imported settings, then remove the old API key file. Import only files you trust. Older inline keys are moved to the separate key file when the app loads. The application directory and files use owner-only permissions on Unix. For cloud providers, document excerpts, the original filename, and optional folder naming context are sent to the configured endpoint. A custom Ollama endpoint can also be remote; the app labels it accordingly. The app does not send raw document files.

The settings modal lets you edit the system prompt, user template, temperature, and folder context. Some newer models do not accept a custom temperature; the app omits that parameter for those models.

During a queue run, **Process Queue** changes to **Stop Processing**. Stopping prevents later files from starting, cancels active AI requests, and returns unfinished files to pending so they can be processed later. A request already delivered to a cloud provider cannot be recalled.


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

The build regenerates `THIRD_PARTY_NOTICES.md` for the Rust host target and bundles it with `LICENSE`. For a cross-target build, set `MAGIC_RENAMER_TARGET` to the destination Rust target triple before running the build. Commit the regenerated notices and `src-tauri/Cargo.lock` with each release. The notice file links to exact source archives for MPL-licensed crates. If you modify any of those crates, provide the modified source to recipients under MPL 2.0.

Before distributing an AppImage or another installer that contains native libraries, inspect its contents and include the applicable native library licenses and source information. System-provided libraries in other packages have their own distribution terms.

Tauri writes build outputs under `src-tauri/target/release/bundle/`. Copy each validated release package into its matching folder under [`releases/v0.1.2/`](releases/v0.1.2/) for GitHub publication. The planned targets are Windows x64, Windows ARM64, Linux DEB, Linux RPM, macOS Apple Silicon, and macOS Intel. See [`releases/README.md`](releases/README.md) for the folder layout.

The [Linux packages workflow](.github/workflows/linux-packages.yml) runs manually from GitHub Actions or when a matching `v<version>` tag is pushed. It uses Ubuntu 22.04 to build x86-64 DEB and RPM packages, runs both test suites, verifies package version, architecture, and bundled license files, and uploads the packages with a `SHA256SUMS` file as a workflow artifact. Download that artifact and review the packages on the target distributions before copying them into `releases/v<version>/linux-deb/` and `linux-rpm/`. The workflow does not publish a GitHub Release or commit binaries automatically.

The [Windows packages workflow](.github/workflows/windows-packages.yml) runs on Windows x64 and ARM64 runners. It runs the test suites, builds native NSIS installers, checks the app executable architecture and installer version, and uploads each installer with the project license, generated third-party notices, and `SHA256SUMS`. Download the artifacts, install and launch them on each architecture, then copy validated files into `releases/v<version>/windows-x64/` and `windows-arm64/`. The workflow does not publish or commit binaries automatically.

The [macOS packages workflow](.github/workflows/macos-packages.yml) builds separate Apple Silicon and Intel DMGs on native macOS runners. It checks the app bundle version, architecture, bundled license files, and DMG integrity, then uploads each DMG with notices and checksums. Install and launch each DMG on its target Mac before copying it into the matching `releases/v<version>/` folder. These CI builds have no Developer ID signing or Apple notarization configured; configure and verify those before publishing downloadable macOS packages.

For a private first release, commit and push these workflows, then push the matching `v<version>` tag. After the Linux, Windows, and macOS runs succeed, manually run [Prepare private release](.github/workflows/prepare-private-release.yml) and supply their three run IDs. It verifies all runs used the tagged commit, checks downloaded package hashes, creates a review branch containing `releases/v<version>/`, and creates a draft GitHub Release with the six installers. Review and merge the branch to retain packages in Git. If GitHub blocks pull-request creation by Actions, open a pull request from the pushed branch shown in the workflow summary. The draft release stays private while the repository is private and remains unpublished until reviewed. Regular Git rejects files of 100 MiB or more; the promotion step stops with a clear error if a package crosses that limit.

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

This project is licensed under the MIT License; see [LICENSE](LICENSE). Dependency licenses and source links are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

