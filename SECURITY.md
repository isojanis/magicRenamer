# Release security review

The scheduled and pull request security workflow runs
`npm audit --audit-level=moderate` and
`cargo audit --file src-tauri/Cargo.lock`. Package workflows use the locked dependency files. Recheck
advisory exceptions when updating `office_oxide`, `zip`, or their call sites.

## Current RustSec triage

- `rustls` was updated to 0.23.46 for RUSTSEC-2026-0346 through
  RUSTSEC-2026-0349. RUSTSEC-2026-0285 was already fixed in 0.23.45.
- RUSTSEC-2026-0367, RUSTSEC-2026-0368, and RUSTSEC-2026-0369 affect ZIP
  APIs that extract archive entries to the filesystem. The pinned
  `office_oxide` 0.1.13 opens and reads individual entries; it does not call
  `ZipArchive::extract` or `ZipStreamReader::extract`. MagicRenamer's own
  Office archive preflight reads entry metadata and likewise does not extract
  entries to the filesystem. The CI audit ignores only these three IDs.
  This exception must be removed or renewed with a call-site review if the
  dependency or parser changes.
- RUSTSEC-2024-0429 is an informational unsoundness advisory for
  `glib::VariantStrIter` 0.18.5 in the Linux Tauri/GTK stack. MagicRenamer
  does not use that iterator directly. The upstream GTK binding has not
  moved to glib 0.20 in the current dependency tree. Keep this under review
  when upgrading Tauri.
- RustSec also reports unmaintained `proc-macro-error` and `ttf-parser`
  dependencies. They are transitive; neither is an active vulnerability
  finding. Review on dependency upgrades.

## Document parser limits

Extraction rejects inputs above 25 MiB. ZIP-based Office documents are
preflighted against 10,000 entries, 25 MiB per expanded entry, and 100 MiB
total declared expanded size. At most two parser jobs run at once. Calls
return a timeout after 20 seconds. The parser runs in a blocking thread, so
a timed-out job may continue using resources until it finishes. Process
isolation with a hard memory and CPU limit remains open before public release.

## Saved provider keys

Keys currently use the private per-user settings file, with owner-only Unix
permissions. A keychain migration must handle Windows Credential Manager,
macOS Keychain, and Linux Secret Service availability and unlock behavior.
Keep the existing file until each migrated key is read back successfully;
then remove only that key from the file. Missing or locked keychains must
produce a clear UI state and must never silently erase the saved key. The
migration needs per-platform restart, locked-store, and rollback tests before
enabling it for existing users.

## License notice status

The notice index lists dependencies included in the desktop runtime: Rust crates from
Cargo tree's resolved normal dependency graph for the selected target, plus npm modules rendered
into the production frontend bundle. Full license and source notice texts are in
`THIRD_PARTY_LICENSE_TEXTS.md`; both files ship with release packages. The strict
`npm run check:licenses` gate still fails on five Rust crates without collected
license files: `adobe-cmap-parser`, `alloc-stdlib`, `dlopen2`,
`pdf-extract`, and `type1-encoding-parser`. Their copyright and permission notices need source
review before public release.

## Dependency scope review

Reviewed against the current lockfile for `x86_64-unknown-linux-gnu`. All 15
normal direct Rust dependencies have production call sites; `tauri-build` is used
by the build script. The three direct npm dependencies provide the IPC API,
Tauri CLI, and Vite build tooling. Only the IPC API contributes frontend bundle
modules.

The resolved normal Rust graph contains 249 distinct package versions, excluding
build and procedural macro dependencies. The earlier count of 302 came from
traversing Cargo metadata, which includes features from other dependency contexts.
The generator now selects packages with `cargo tree --target <target> --edges
normal,no-proc-macro` and uses metadata to locate their source and license files.
These are dependency graph counts, not measurements of code retained by the linker.
See [Cargo tree's feature-unification notes](https://doc.rust-lang.org/cargo/commands/cargo-tree.html#feature-unification).

| Dependency group | Packages in its runtime subtree | Application use |
| --- | ---: | --- |
| Tauri with dialog plugin | 157 | Native window, webview, IPC, folder selection, settings import |
| Reqwest | 88 | Ollama and cloud provider HTTP/HTTPS requests |
| PDF extraction | 58 | PDF text extraction, including font encodings |
| Office extraction | 31 | DOC, DOCX, XLS, XLSX text extraction |

Subtrees include their root package and overlap; their counts must not be added.
The filesystem plugin remains an indirect dependency of the dialog plugin.

Tokio now requests `rt-multi-thread`, `macros`, `sync`, `time`, `net`, and
`io-util` instead of `full`. The resolved Linux runtime graph dropped from
251 to 249 crates, removing `errno` and `signal-hook-registry`. The app has
no Tokio process, signal, or standard-input/output call sites.

Further feature experiments in a temporary manifest produced these Linux graph
changes. They have not been applied or validated with desktop builds:

- Replace Chrono's `clock` feature with `now`, retaining `std`: removes
  `iana-time-zone`. The app uses UTC timestamps. The graph would drop from 249 to 248 packages.
- Disable Reqwest charset decoding: removes no packages because its dependencies
  are shared. Disabling HTTP/2 as well removes three packages, but changes network
  protocol support and needs provider compatibility checks. Retain TLS and system
  proxy behavior unless a separate decision changes those requirements.
- Disable Tauri asset compression: removes four Brotli-related packages, but
  trades them for uncompressed bundled assets. This is not automatically a benefit.
- Narrow the app's ZIP feature to a single Deflate backend: removes no packages.
  `office_oxide` also enables ZIP's broader `deflate` feature, retaining the Zopfli
  compressor. Removing that compressor requires an upstream feature change or a
  maintained patch; the app itself only reads Office archives.

Prioritize the UTC-clock feature reduction, then evaluate upstream ZIP
feature control. Replacing the desktop framework or document parsers would require
larger implementation and compatibility work; removing those dependencies outright
would remove existing application features.
