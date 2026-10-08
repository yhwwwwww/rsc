# Tests and results

[简体中文](testing.zh-CN.md) · [README](../README.md)

## Native implementation baseline

The old version that embedded Scoop scripts has been discarded. Its test results are archived outside this project and do not count toward the native rewrite.

The independent Rust version is tested on Windows with a GNU x64 toolchain. The release binary, compiler, checksum and imported DLLs are recorded in [build-info.json](build-info.json). The latest completed suite is recorded in [test-results.json](test-results.json).

## Latest result

The native release passed **19 Rust tests and 32 Windows integration groups**, with no failures. The locally installed program was replaced with this release and its checksum matches the distribution artifact.

## Run

```powershell
.\scripts\test.ps1
.\scripts\test.ps1 -SkipBuild -Phase native
```

`test.ps1` runs Rust tests, builds independently written fixture executables and launches the Python integration harness. It uses a system temporary directory with spaces and Chinese characters. Live Scoop configuration and the relevant user environment registry entries are preserved.

## Coverage

| Group | Checks |
| --- | --- |
| Rust | CLI dispatch, architecture fallback/rejection, filenames, version order, regex validation, UTF-16 metadata, context expansion, read-only junction removal, Metalink parsing |
| Network | Concurrent ranges, cache/force, ignored or incorrect ranges, HTTP retry, disconnect, hash mismatch, headers/cookies, killed-process resume, cache removal |
| Lifecycle | Git buckets, dependencies/cycles, hooks, installer scripts, directory persist, environment, shortcuts, modules, shims and exit status |
| Version management | Hold/unhold, bucket changes, update, force/no-cache, reset, cleanup, pinned SQLite/Git history |
| Recovery | Failed hooks, incomplete metadata and empty Scoop installation directories |
| Interoperability | Installation/reset/uninstall in both directions, legacy metadata, no_junction and Scoopfile import/export |
| Standalone native | No Scoop resources, multiple archives, extraction directories, file persist, helper forwarding, script/GUI shims, Metalink payload verification, local file URLs |
| Real packages | jq and ripgrep, actual downloads and executable launch |

The comparison tests invoke Scoop from its separately installed location. Copied supporting resources are kept only in temporary test directories, outside the rsc repository. The standalone native phase removes these resources.

## Interpretation

Passing these checks proves the exercised behaviors. It does not prove all manifests, remote services, archive types, architectures or deployment environments.

Authenticated proxies, FTP, FossHub/private GitHub releases, MSI/Inno/WiX, uncommon script helpers and autoupdate templates need additional fixtures or real deployment tests. See [Compatibility](compatibility.md).

The published records contain the current native results, not the previous wrapper implementation's results. Machine-readable records are shared by both document languages.

[Development](development.md) · [Architecture](architecture.md)
