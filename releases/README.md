# Release packages

Store packaged builds under `v<app version>/<target>/`. The version should match `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.

```text
releases/
└── v0.1.1/
    ├── windows-x64/
    ├── windows-arm64/
    ├── linux-deb/
    ├── linux-rpm/
    ├── macos-apple-silicon/
    └── macos-intel/
```

These are planned release targets; an empty folder does not mean a package has been built or tested. The `.gitkeep` files retain empty folders in Git. Remove each placeholder when adding a package. Verify the package on its target platform and include the project license and generated third-party notices before publishing it.

The [Linux packages workflow](../.github/workflows/linux-packages.yml) uploads verified DEB and RPM files plus `SHA256SUMS` as a GitHub Actions artifact. After installation checks on Debian and RPM-based systems, copy the packages to the corresponding folders here.

The [Windows packages workflow](../.github/workflows/windows-packages.yml) uploads x64 and ARM64 NSIS installers as separate artifacts. Each platform folder contains the installer, license, third-party notices, and `SHA256SUMS`. Install and launch each package on its target architecture before placing it here.

The [macOS packages workflow](../.github/workflows/macos-packages.yml) uploads Apple Silicon and Intel DMGs as separate artifacts, each with license files and `SHA256SUMS`. These builds require Developer ID signing, notarization, and target-Mac installation checks before public distribution.

To keep the first builds here while the repository is private, run [Prepare private release](../.github/workflows/prepare-private-release.yml) after all three platform workflows succeed for the same tag. It checks the artifacts, pushes a review branch containing these folders, and creates a draft GitHub Release. Merge the review branch after installation checks. The draft Release contains direct installer downloads; these folders retain the files in Git.
