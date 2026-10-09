# Development plan

[简体中文](plan.zh-CN.md) · [README](../README.md)

## Goal and scope

Create a single Windows `rsc.exe` in Rust. The internal library must be statically linked. Match Scoop behavior, configuration, package manifests and directory layout. The intended changes are built-in multithreaded downloads and clearer output.

Scoop source code is excluded from the repository, executable and published Git history. Reference checkouts and comparison artifacts belong outside the project. PowerShell may execute manifest-supplied hooks and user aliases, through independently written adapters; it is not the manager backend.

The behavioral baseline is [Scoop v0.6.0, commit e6aa3b3](https://github.com/ScoopInstaller/Scoop/tree/e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f). Manager self-update is excluded. Package updates and bucket synchronization are included.

## Milestones

| Stage | Deliverable | Acceptance |
| --- | --- | --- |
| Foundation | Cargo executable and static library, configuration, layout, output | Standalone release binary and dependency audit |
| Query | Manifests, bucket search, installed state, architecture selection, SQLite | Queries against real and isolated roots |
| Download | Concurrency, segmentation, retries, resume, hash validation, compatible cache | HTTP fault tests and real package downloads |
| Lifecycle | Dependencies, install, persist, shims, uninstall, recovery | Native integration and bidirectional Scoop tests |
| Windows | Junctions, registry, shortcuts, modules, GUI/script launchers, process checks | Reparse-point safety and Windows execution tests |
| Management | Git buckets, history, update, hold, reset, cleanup, Scoopfiles | Historical versions, failure recovery and mutual import/export |
| Auxiliary commands | Alias, shim, checkup, create, VirusTotal | Command workflows and external-service validation |
| Release | GPLv3, English primary documentation and Chinese translations | Clean source history, public repository, accurate test records |

## Compatibility contracts

- Read `SCOOP` / `SCOOP_GLOBAL` / `SCOOP_CACHE` before configured paths.
- Respect XDG, standard and detected portable configuration roots.
- Preserve unknown configuration fields, key casing semantics and installation metadata fields.
- Use Scoop's apps/version, current, persist, shims, buckets, modules and cache structure.
- Write modern installation metadata; read modern and legacy filenames.
- Honor architecture fields, dependencies, binary aliases, extraction directories, installer entries, hooks, notes and suggestions.
- Preserve persistent user data during update, reset, cleanup and ordinary uninstall.
- Verify replacement downloads before unlinking the previous version.
- Return failure for incomplete installs, hash mismatch, script errors and failed batches.
- Preserve ordinary executable arguments, streams and exit status.

## Validation work

Native Windows tests cover core lifecycle and common portable packages. Additional verification remains for authenticated proxy deployments, FTP servers, FossHub, private GitHub releases, MSI/Inno/WiX packages, unusual autoupdate templates, uncommon manifest helper calls, external-service rate limits, ARM64/MSVC builds and clean Windows systems.

An implementation path is not equivalent to verified behavior. [Compatibility](compatibility.md) identifies actual limits, and [Testing](testing.md) records completed checks.

## Documentation

[Architecture](architecture.md) · [Commands](cli.md) · [Development](development.md) · [Tests](testing.md)

## Output and query optimization

Implemented semantic body colors, built-in JSON highlighting, bounded parallel manifest reads and shared status metadata. Result-equivalence checks and timings are recorded in [Performance](performance.md). Current regression results are in [Testing](testing.md).
