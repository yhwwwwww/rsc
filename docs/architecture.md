# Architecture

[简体中文](architecture.zh-CN.md) · [README](../README.md)

## Implementation boundary

All package management behavior is implemented in Rust. No Scoop source tree, PowerShell manager, embedded upstream resource table, or script expansion backend is present.

`rsc_core` is an `rlib` linked into `rsc.exe`. The executable also acts as the shim launcher. SQLite and the TLS implementation are linked into the program; Windows system libraries provide operating system integration.

## Modules

| Module | Responsibility |
| --- | --- |
| `main`, `commands`, `output` | Direct commands, error states, tables and progress |
| `config`, `layout` | Scoop settings, portable roots, user/global scope |
| `bucket`, `database` | Git bucket index, compatible SQLite cache and history |
| `manifest`, `package` | Architecture fields, manifest validation, installation state |
| `download`, `ftp` | Concurrent HTTP transfer, ranges, resume, cache, hashes, FTP |
| `manager` | Dependency graph, locks, download-before-update, commit and recovery |
| `native/query` | Resolution, history, versions, regex search, metadata, special URLs |
| `native/lifecycle` | Extraction, installer orchestration, persist and package integration |
| `native/windows`, `native/attributes` | Registry, junctions, process checks, shortcuts, WinHTTP |
| `native/commands` | Alias, shim, diagnostics, manifest creation, VirusTotal |
| `native/scripts` | Boundary for manifest-supplied or user-supplied scripts |
| `native/metalink` | Native XML redirect parsing |
| `shim` | Launch target with fixed and caller arguments, inherited streams and exit code |

The native action dispatcher is a Rust function boundary. It does not invoke another manager.

## Package transaction

1. Resolve manifests and dependencies; detect cycles.
2. Select architecture and acquire the package lock.
3. Download and verify every file.
4. Create a version directory and incomplete-installation marker.
5. Extract archives; execute manifest pre-install and installer entries.
6. Create current, shims, shortcuts, modules and environment entries.
7. Connect persist data; execute the manifest post-install entry.
8. Atomically write installation metadata and remove the marker.

Updates validate downloads before replacing the active version. Older versions remain available. Failure attempts to restore the previous entries. Uninstall preserves persist unless purge was requested.

Deletion uses reparse-point-aware traversal. A junction is removed as a link; its target is not traversed. Read-only attributes on Scoop-created links are cleared through a handle opened on the link itself.

## Script boundary

Manifest script fields and aliases are external programs provided by the package author or user. They may require Windows PowerShell. A small independently written context adapter supplies variables such as `dir`, `original_dir`, `persist_dir`, `manifest`, `architecture` and `global`.

Supported helper adapters send requests back to Rust. Management decisions, filesystem operations, downloads and Windows integration remain in Rust. The adapter contains no copied Scoop functions. Arbitrary scripts may use additional Scoop-specific helpers; their compatibility needs separate validation.

## Downloads

File concurrency, host limits and HTTP segmentation are independent. Range responses are checked for exact bounds and resource validators. Unsupported ranges fall back to one stream. Resume state records the URL, expected hash, length and validators. The final file is verified before atomic publication.

Windows-authenticated proxies use WinHTTP; FTP uses WinINet. ZIP extraction and Metalink parsing are internal. Other formats use package helper tools when required.

See [Compatibility](compatibility.md) and [Development](development.md).
