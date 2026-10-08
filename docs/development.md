# Building and development

[简体中文](development.zh-CN.md) · [README](../README.md)

## Requirements

Windows and a Rust toolchain are required. Git is used for buckets; package-specific extraction tools are discovered in package directories or PATH. Python 3 is used by the integration suite. The comparison phase requires an installed Scoop, but rsc itself does not.

The package declares Rust 1.85 as its source-language minimum. The validated compiler version and target are recorded in [build-info.json](build-info.json); minimum-toolchain and other-target builds need separate validation.

## Build

```powershell
.\scripts\build.ps1
.\scripts\build.ps1 -Profile debug
```

The script uses a regular Rust installation, or an optional project-local GNU toolchain under `.tools`. To provision the latter:

```powershell
.\scripts\setup-dev.ps1
```

The setup script downloads Rust and a compiler toolchain. It is a development utility, not a runtime component.

Release output is `dist\rsc.exe`. `rsc_core` uses `crate-type = ["rlib"]`. SQLite is bundled and the Windows CRT is configured for static linking. Only Windows system DLLs should appear in the release import table.

## Tests

```powershell
.\scripts\test.ps1
.\scripts\test.ps1 -SkipBuild -Phase native
```

Available phases: `network`, `lifecycle`, `native`, `real`, `all`.

The native phase removes the reference manager's resources before exercising rsc. The comparison phases invoke the separately installed Scoop. Reference supporting files and isolated package data are stored in a system temporary directory, outside the source repository.

Tests restore the relevant user environment registry values and verify that the live Scoop configuration is unchanged. Test logs identify the isolated directory. Local fixture binaries and the latest-results pointer are ignored under `.test-lab`.

## Source policy

- Implement behavior independently in Rust.
- Keep Scoop checkouts, scripts and copied manager resources outside this project.
- Do not introduce a PowerShell backend, embedded upstream source, or runtime script extraction.
- Manifest/user scripts are permitted inputs; independently written context and helper adapters belong in `native/scripts`.
- Keep new public documentation bilingual: English primary, Chinese counterpart, matching local-language links.
- Verify behavior with fixtures and upstream comparison; never mark a feature verified merely because it compiles.
- Publish only clean history that excludes the discarded upstream-script implementation.

[Architecture](architecture.md) · [Compatibility](compatibility.md) · [Testing](testing.md)
