# MagicRenamer test plan

This plan covers the implemented desktop workflow in `main.js` and the `src-tauri/src/` Rust modules. It is separate from the product roadmap in `PLAN.md`. The aim is confidence that a user can scan, inspect, name, rename, and restore local documents without losing data or sending document text anywhere beyond the selected AI provider.

## Current baseline

- Rust now has 44 passing headless tests, including deterministic Ollama HTTP stub cases, config persistence, no-overwrite rename and undo, concurrent manifest updates, SHA-256 verification on manifest undo, rollback, and symlink/path safety. The real local Ollama smoke test remains ignored by default.
- `npm run test:rust` generates the ignored fixture corpus before running Rust tests, so a clean checkout does not need manually prepared documents.
- `tests/frontend.test.js` has fourteen dependency-free workflow checks, a small DOM fake, and mocked Tauri calls. All fourteen pass with the bundled Node runner. The same frontend and headless Rust suites are exposed through `npm run test:all`.
- Desktop packaging and Linux/Windows smoke checks remain manual. Coverage does not yet include every P1 UI interaction or every document format named in the roadmap.

## Test approach

1. **Rust behavior tests:** Keep pure date, context, excerpt, and filename rules in fast table-driven unit tests. Use per-test temporary directories for commands that read or change files. Make fixture setup explicit so a clean checkout can run the documented command.
2. **Rust integration tests:** Exercise the scan, extraction, config, rename, manifest, and undo commands through their Rust functions. Give config tests an isolated config path and give Ollama tests an injectable local endpoint or HTTP client. Use a deterministic local HTTP stub for request, response, retry, and error cases; keep the real Ollama check optional.
3. **Frontend tests:** The first suite uses Node's built-in test runner and a small DOM fake, with mocked Tauri invoke and folder-dialog calls. Test observable queue state, rendered details, button state, and calls to the backend. Move to a real DOM runner if the fake stops representing browser behavior reliably.
4. **Desktop smoke checks:** On Linux and Windows, run a short manual or automated packaged-app checklist using disposable copies of fixtures. Include an optional local Ollama run. Never rename the source fixture corpus during smoke checks.

## Feature coverage matrix

| Feature | Cases to cover | Level | Priority |
| --- | --- | --- | --- |
| Folder selection and scanning | Valid folder, empty folder, missing path, nested files, supported extensions with mixed case, ignored unsupported files, metadata, and symlink behavior. Verify a 522-file scan and the warning threshold at 500 versus 501 files. | Rust + frontend | P0 |
| Extraction and preview | TXT, Markdown, CSV, JSON, PDF, DOCX and the Office formats claimed by the app (`xlsx`, `doc`, `xls`); representative excerpt content, length limit, late document date, empty/unreadable/invalid files, and per-file failure shown in the preview. Add real fixtures for each claimed Office format before considering it covered. | Rust + frontend | P0 |
| Document date and folder context | ISO, day-first numeric, English and Finnish month dates, leap/invalid dates, issue date versus due date, date absent, filename fallback, unrelated siblings, ties, dominant invoice label, eight-sibling limit, hidden/history files excluded, and context disabled for a run. | Rust | P0 |
| Ollama suggestion | Correct selected model, system/template substitutions, confirmed date, folder context and matching siblings, JSON schema, temperature bounds, and loopback-only endpoint. Test valid JSON, plain-text fallback if supported, malformed/empty body, HTTP error, connection failure, generic first response followed by valid retry, and two unusable responses. Assert exactly one retry for invalid names and no retry on transport/API errors. | Stubbed Rust integration | P0 |
| Filename validation | Date prefix and original extension, separators, accents, wrong extension, empty or path-like names, generic labels, unverified model dates, specific reference-only names, and category/reason preservation. Use table cases for accepted and rejected suggestions. | Rust | P0 |
| Single rename and batch approval | Edited suggestion reaches the backend, actual collision-suffixed path updates the UI, next ready item is selected, batch processes only ready items, and one failed rename does not falsely mark the whole batch successful. | Rust + frontend | P0 |
| Filesystem and history safety | Existing destinations, including dangling symlinks, get `_1`, `_2` without changing existing bytes; simultaneous renames keep unique names and every manifest entry; missing, symlink, and nonregular sources fail; file bytes/hash survive rename and undo; malformed manifests remain intact and failed updates roll back. Test atomic manifest replacement failure on each release platform. | Rust integration | P0 |
| Undo | xattr recovery where supported, manifest fallback when xattr is absent, SHA-256 mismatch or invalid hash, no history, missing file, repeated undo, and original-name collision. Verify that another file or dangling symlink is never overwritten and history remains usable after an undo/redo cycle. | Rust integration + frontend | P0 |
| Queue and controls | Pending → processing → ready/failed, retry failed item, regenerate, skip, approve, approve all, undo, button enablement, empty queue, selection changes during an async operation, and a batch of more than three files. Check the three-at-a-time limit and that one extraction failure does not block other items. | Frontend | P1 |
| Settings and model choice | Default/missing/malformed config, load/save round trip, save failure, folder-context toggle, model selection, temperature clamp, reset, cancel, reopen, and persistence after restart. | Rust + frontend | P1 |
| Desktop interaction and rendering | Native folder picker cancel/error, drag/drop behavior, Enter shortcut only in the intended context, modal focus, pane resize bounds and restored widths, status/preview text, and narrow-window layout. Render filenames, paths, snippets, and model text containing HTML special characters as text. | Frontend + desktop smoke | P1 |
| Privacy and packaging | Ollama mode uses only a local endpoint; cloud mode sends only the prepared excerpt and context to the selected endpoint. API keys stay out of public config responses and tracked config.json; Linux and Windows packaged app opens, can select a folder, read fixtures, and create/undo a rename in a disposable folder. | Integration + smoke | P1 |
| Cloud provider requests | Local HTTP stubs assert OpenAI/OpenRouter Bearer, Gemini x-goog-api-key, Anthropic x-api-key/version, model and JSON request shapes, response parsing, and no key in URL or error text. | Stubbed Rust integration | P0 |
| Cloud settings | Switching providers updates suggested models, masked key status, endpoint visibility, active model, privacy badge, and Test Connection feedback. Saving a provider preserves other keys; clear-key action and legacy key migration are verified. | Frontend + Rust unit | P0 |
| Queue cancellation | The active Process Queue button becomes Stop Processing. Stop during run setup, extraction, and in-flight AI work; no later batch starts, late results are discarded, unfinished files remain pending, and a new run can resume them. | Frontend + stubbed Rust integration | P0 |

The safety cases are expected behavior. If a test exposes current code that overwrites an original file, accepts a path outside the selected folder, or inserts document text as HTML, fix that behavior rather than weakening the assertion.

## Implementation order

1. **Make tests reproducible:** `npm test`, `npm run test:rust`, and `npm run test:all` are now defined. The Rust script generates fixtures first; local Ollama tests remain opt-in. Keep temporary test directories unique and clean them on success and failure.
2. **Protect file operations first:** Add collision, manifest, undo, hash preservation, and path validation cases. Address any destructive behavior these tests reveal.
3. **Cover naming decisions:** Expand date, excerpt, sibling selection, model parsing, and filename validation tables. Add a local HTTP stub for the full Ollama request/retry path.
4. **Cover the UI workflow:** Add DOM tests for folder loading, processing, approval, undo, settings, and error states. Then test keyboard, drag/drop, resizing, and untrusted text rendering.
5. **Run desktop smoke checks:** Use copied fixtures and record Linux/Windows results, including any platform-specific xattr/ADS differences. Keep optional real-model output checks broad enough to tolerate model wording while enforcing filename safety.

## Completion criteria

- Every P0 matrix row has an automated success case and at least one meaningful failure or boundary case; every P1 row has automated coverage where practical and a documented desktop check for platform-dependent behavior.
- The default test command passes from a clean checkout without a running Ollama service, cloud API credentials, or an existing generated fixture corpus.
- The optional local Ollama test and desktop smoke checks have separate commands and prerequisites; they do not make normal CI flaky.
- CI runs frontend tests and headless Rust tests on each change, plus an application build. Run the packaged desktop smoke checklist on Linux and Windows before a release.
- Track branch coverage for the rules and workflow code after the harness exists. Use it to find missed paths; do not substitute a percentage for the feature matrix above.
