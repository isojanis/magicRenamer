# Release packages

Store packaged builds under `v<app version>/<target>/`. The version should match `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.

```text
releases/
└── v0.1.0/
    ├── windows-x64/
    ├── windows-arm64/
    ├── linux-deb/
    ├── linux-rpm/
    ├── macos-apple-silicon/
    └── macos-intel/
```

These are planned release targets; an empty folder does not mean a package has been built or tested. The `.gitkeep` files retain empty folders in Git. Remove each placeholder when adding a package. Verify the package on its target platform and include the project license and generated third-party notices before publishing it.
