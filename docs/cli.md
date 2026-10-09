# Commands and configuration

[简体中文](cli.zh-CN.md) · [README](../README.md)

Commands use Scoop's names and direct style.

| Command | Behavior |
| --- | --- |
| `install app...` | Resolve dependencies and install; accepts local/URL manifests and pinned versions |
| `uninstall app...` | Remove packages; `--purge` also removes persistent data |
| `update` | Synchronize buckets |
| `update app...` / `update *` | Update packages; honor hold, accept forced reinstall |
| `status [--local]` | Report available versions, incomplete state and missing dependencies |
| `reset app[@version]` / `reset *` | Activate an installed version and recreate integration |
| `cleanup app...` / `cleanup *` | Remove older versions; `--cache` also removes their cache |
| `hold app...` / `unhold app...` | Disable/enable updates |
| `bucket add name [repo]` | Add a known or explicit Git bucket |
| `bucket rm name` / `bucket list` / `bucket known` | Remove/list buckets or show known URLs |
| `depends app` | Dependency and extraction-helper installation order |
| `search [query]` / `list [query]` | Find manifests or installed packages |
| `info app [-v]` / `cat app` | Package details or manifest JSON |
| `prefix app` / `which command` | Print active directory or resolved command target |
| `download app...` | Fetch and verify package files without installing |
| `cache show [app...]` / `cache rm app...` | Inspect/remove matching cache entries |
| `config [name [value]]` / `config rm name` | Read, set or delete a setting |
| `export [--config]` / `import file-or-url` | Write/read a Scoopfile |
| `home app` | Open package homepage |
| `alias add/rm/list` | Manage user-supplied command aliases |
| `shim add/rm/list/info/alter` | Manage native launchers and alternative targets |
| `checkup` | Inspect roots, tools, environment and incomplete installations |
| `create url [output]` | Download, hash and interactively create a manifest |
| `virustotal app...` | Query VirusTotal; requires `virustotal_api_key` |
| `help [command]` | Show command help |

## Search results

`search [query]` follows Scoop's default name and binary search. Without SQLite, it uses a case-insensitive regular expression, searches names and top-level `bin` entries, and displays the matched executable filenames or aliases in `Binaries`. A name match leaves that column blank. Architecture-specific `bin` entries and descriptions do not add matches. Scoop's raw-content prefilter also applies before binary matching. With SQLite enabled, Scoop's name/binary/shortcut LIKE behavior remains in use.

`Installed` shows the installed version and user/global scope for the same bucket. `State` distinguishes `current` (green), `outdated` (yellow), `newer` and `broken` (red); held packages retain their hold indication. An installation from a different bucket does not mark a same-name result as installed. Missing source metadata is indicated explicitly. Unversioned nightly manifests show `unknown (nightly)` rather than claiming a latest version.

Versions are compared with the local bucket manifests; search does not fetch updates. Bucket priority and duplicate names are retained. No persistent search cache is added.

## Options

- `-g` / `--global`: global scope; writes require administrator rights.
- `--arch 64bit|32bit|arm64`: architecture; install/download/depends also accept `-a`.
- Install/update: `-i` / `--independent`, `-k` / `--no-cache`, `-s` / `--skip-hash-check`.
- Download: `-f` / `--force`, `-s` / `--skip-hash-check`.
- Update: `-f` / `--force`, `-a` / `--all`; reset/cleanup also accept `--all`.
- Uninstall: `-p` / `--purge`. Cleanup: `-k` / `--cache`.
- `--no-update-scoop` is accepted for compatibility. Manager self-update is excluded.
- VirusTotal: `--all`, `--scan`, `--no-depends`, `--no-update-scoop` and `--passthru` are accepted.

Run `rsc help command` for the complete argument syntax.

## Configuration

Scoop settings stay in the detected Scoop `config.json`. Downloader tuning stays in `rsc/config.json` under the configuration home.

| Downloader setting | Default |
| --- | --- |
| `download.threads` | 4 |
| `download.concurrent` | 4 |
| `download.per_host` | 8 |
| `download.split_size` | 4,194,304 bytes |
| `download.retries` | 3 |
| `download.timeout` | 30 seconds |

`show_manifest` requests confirmation before installation; `cat_style` invokes bat to render manifests. `use_sqlite_cache` enables Scoop-compatible SQLite indexing. `use_isolated_path` migrates package paths to the configured environment variable.

Plain queries use case-insensitive regular expressions; SQLite search uses LIKE. See [Compatibility](compatibility.md) for regex and autoupdate limits.

## Output and exit status

Tables use terminal width, Unicode-aware display widths and semantic color. Names are cyan, versions magenta, healthy states green, attention states yellow, failures red, and secondary paths/sources dim. Mixed states keep their own colors. `cat` highlights JSON keys, strings, numbers, literals and punctuation internally; setting `cat_style` retains the optional bat viewer. Redirected output and `NO_COLOR` disable dynamic/color formatting. Diagnostics and download progress go to stderr; paths, manifests and Scoopfiles are suitable for stdout redirection.

`status` keeps its report on stdout: expected bucket updates are summarized before the package table, and check warnings are grouped afterward under `Checks needing attention`. Fatal errors still use stderr. This preserves report order even when a shell merges streams.

Failed commands return nonzero. VirusTotal reserves 2 for unsafe reports, 4 for request errors, 8 for unresolved manifests and 16 for a missing API key; report failures may combine these bits.
