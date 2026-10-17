# Test Documents

This fixture set contains small, synthetic local documents for manual scanning and rename-flow checks. The document corpus is intentionally ignored by Git; recreate it with `python3 test_docs/generate_fixtures.py`.

- The categorized subfolders cover finance, engineering, legal, research, operations, and data exports, with TXT, Markdown, CSV, and JSON examples.
- `document-formats/` contains small PDF and DOCX examples for parser coverage.
- `large-batch/` contains 522 supported files across its root and six nested folders. Select this folder to exercise recursive discovery and the large-folder context warning.

All names, organizations, and amounts are fictional. No fixture contains private data.

From the project root, run the headless Rust tests with:

```sh
npm run test:rust
```

This generates the ignored fixture corpus first, then runs Rust tests with `--no-default-features`, which omits the Tauri desktop runtime and WebKitGTK requirement. Run the frontend tests with `npm test`, or both suites with `npm run test:all`. The real Ollama smoke test remains ignored unless explicitly selected and requires the configured local model.
