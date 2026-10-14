# Test Documents

This fixture set contains small, synthetic local documents for manual scanning and rename-flow checks. The document corpus is intentionally ignored by Git; recreate it with `python3 test_docs/generate_fixtures.py`.

- The categorized subfolders cover finance, engineering, legal, research, operations, and data exports, with TXT, Markdown, CSV, and JSON examples.
- `document-formats/` contains small PDF and DOCX examples for parser coverage.
- `large-batch/` contains 522 supported files across its root and six nested folders. Select this folder to exercise recursive discovery and the large-folder context warning.

All names, organizations, and amounts are fictional. No fixture contains private data.

After generating the local corpus, run the fixture-backed Rust tests from the project root with:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features fixture_tests
```

The `--no-default-features` mode omits the Tauri desktop runtime so these local parser and folder-scanning tests can run without WebKitGTK development packages.
