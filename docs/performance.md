# Query performance

[简体中文](performance.zh-CN.md) · [README](../README.md)

## Measurement

Windows 11, GNU x64 release build, 16 logical processors. The same local Scoop installation contains 5 buckets and 5,189 manifests. SQLite search is disabled. Both programs read the same bucket directories; Hok search uses `-B` to include executable aliases.

Each result is the median of 7 runs after one warmup. Timings include process creation and captured stdout. These are measurements with a warm filesystem cache, not cold-disk or terminal redraw benchmarks. Executable hashes, versions and every sample are in [query-benchmark.json](query-benchmark.json).

| Command | Previous rsc | Current rsc | Hok |
| --- | ---: | ---: | ---: |
| `search jq` | 575.9 ms | 90.0 ms | 97.2 ms |
| `search ^git` | 631.1 ms | 109.0 ms | 115.4 ms |
| `status --local` | 510.6 ms | 44.0 ms | — |
| `list` | 34.5 ms | 30.0 ms | 36.2 ms |
| `cat jq` | 49.7 ms | 29.6 ms | 41.0 ms |

Hok 0.1.0-beta.7 has no `status` command. The local status comparison therefore uses the previous native rsc release as its baseline. The results establish comparable query speed on this dataset; they do not guarantee a particular latency on other machines.

## Implementation changes

- Enumerate bucket filenames once for a status operation and use a name lookup for every installed package.
- Read installed metadata once, and reuse a dependency name set.
- Compile the version comparison expression once per process.
- Search directly over parsed manifests, removing the full JSON serialization and second parsing pass.
- Read manifests concurrently with at most 16 workers, preserving bucket priority, row order and malformed-manifest warnings.
- Create the download worker runtime only for transfer operations; queries use a smaller runtime and shims dispatch before runtime creation.
- Fetch remote bucket status concurrently while retaining the existing checks and warning order.

No persistent search cache is introduced. Manifest changes and deletions are visible on the next command. Existing Scoop SQLite configuration is still respected.

`status` without `--local` still checks remote bucket updates, as Scoop does. Network latency is excluded from the table. Search with no local match still checks other known buckets, so that path also depends on the network.


A separate live network check of default `status` (median of 3 runs) measured 5.11 s before and 0.99 s after, with the same bucket update warnings. These timings vary with the network. Samples are recorded separately under `remote_status` in the raw record.

## Result checks

The benchmark compares previous and current rsc search/list output and manifest JSON. Local status output is compared after normalizing the newly explicit `current` state. Rust fixtures cover case-insensitive names, executable aliases, architecture fields, nested manifests, deterministic order, malformed manifests, edits/deletions, user/global scope, held/broken/removed packages, missing dependencies and nightly versions.

Direct executables are timed in the table above. Installed command entry points were also measured separately, including the shim and child process startup. `search jq`: rsc 102.3 ms, Hok 105.4 ms. `list`: rsc 44.6 ms, Hok 41.5 ms. The same warmup and 7-run median are used; raw samples are under `installed_launchers`.

## Reproduce

```powershell
python scripts/benchmark_queries.py after hok
python scripts/benchmark_queries.py before after hok --before .test-lab/rsc-before-query.exe --output .test-lab/query-benchmark.json
```

The previous executable is an optional local artifact and is not distributed with the repository. Build `dist/rsc.exe` first. The script performs local read-only queries and uses the current Scoop configuration.

[Testing](testing.md) · [Architecture](architecture.md)
