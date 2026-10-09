# Compatibility

[简体中文](compatibility.zh-CN.md) · [README](../README.md)

rsc targets behavioral compatibility with [Scoop v0.6.0](https://github.com/ScoopInstaller/Scoop/tree/e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f). It is an independent Rust implementation and includes no Scoop code.

## Implemented and exercised

| Area | Evidence |
| --- | --- |
| Shared configuration and paths | Isolated roots, unchanged live config, no_junction and environment restoration |
| HTTP concurrency, hashes, cache | Range, ignored ranges, incorrect ranges, retries, disconnect, resume, private headers and cookies |
| Package lifecycle | Install, dependency sorting/cycles, hooks, script installers, failed-install repair and uninstall |
| Persist | Directory junctions, file hardlinks, renamed persist entries, reinstall, update and purge |
| Windows integration | Native environment, shortcuts, modules, executable/script/GUI shims |
| Package version management | Git bucket synchronization, SQLite history, pinned versions, hold, update, reset, forced reinstall and cleanup |
| Scoopfiles | Import/export in both directions |
| Shared installations | rsc install → Scoop list/uninstall; Scoop install → rsc list/reset/uninstall |
| Real packages | jq download and execution; ripgrep ZIP installation and execution |

The standalone native phase removes Scoop manager resources before running. Source audits and clean publication history enforce the zero-upstream-code requirement. Exact test results are in [Testing](testing.md).

## Intentional differences

- Built-in Rust concurrent/segmented downloads replace the downloader backend. Existing aria2 settings are preserved for Scoop, but rsc does not launch aria2.
- Output uses rsc's tables, progress and diagnostics.
- Scoop's Git-based manager self-update is excluded. Software updates and bucket synchronization remain supported. rsc itself uses the ordinary kits package update path; see [Update rsc](../README.md#update-rsc) for installation modes and handling an executable that is in use.

## Current limits

The core tested workflows work, but this release is not certified for every Scoop manifest or every Windows deployment.

- Regular expressions use Rust's fancy-regex implementation. Common case-insensitive expressions and lookaround are supported; .NET-only constructs such as balancing groups are not equivalent.
- Historical manifests are found through SQLite or Git. Autoupdate supports version substitutions and URL/regex hash lookup; arbitrary PowerShell hash-generation expressions and every upstream template variant are not covered.
- The manifest script adapter supports common context variables and helper forwarding. It does not recreate every helper exported by Scoop. Scripts using additional helpers need implementation and tests.
- checkup reports implemented native checks; it does not yet reproduce every specialized Scoop diagnosis.
- create produces a usable manifest through its own interactive prompts; exact prompt order and all URL guessing rules are not equivalent.
- VirusTotal has native query/submission paths and error codes. Live API rate-limit behavior and detailed output variants have not been validated.
- FTP, authenticated proxies, FossHub/private-release lookup and MSI/Inno/WiX extraction have native code paths but need deployment-specific verification.
- ARM64/MSVC, the declared minimum compiler and a clean Windows system have not been validated.

These limits replace previous claims based on embedded Scoop functions. Earlier hybrid-version tests are not proof of native compatibility.

## Data handling

Uninstall keeps persistent data unless `--purge` is requested. Cleanup removes old version directories without following their persist links. Read-only Scoop junctions are handled through native Windows APIs.

Updates download and verify before removing active entries, preserve old versions and attempt recovery after failure. An unfinished installation remains visibly marked and is repaired or removed before retrying.

See [Plan](plan.md), [Architecture](architecture.md) and [Commands](cli.md).
