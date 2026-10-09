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

`search [query]` follows Scoop's default name and binary search. Without SQLite, it uses a case-insensitive regular expression, searches names and top-level `bin` entries, and displays matched executable filenames or aliases in `Binaries`. A name match leaves that column blank. Architecture-specific `bin` entries and descriptions do not add matches by default. Scoop's raw-content prefilter applies before default binary matching. With SQLite enabled, Scoop's name/binary/shortcut LIKE behavior remains in use.

`Installed` shows the version and scope as `2.48.1 (user)` or `2.48.1 (global)` for the same bucket. The version itself is blue when current, yellow when outdated or unknown, magenta when newer than the manifest, and red when broken. There is no separate state column. Each scope is colored independently; hold and missing-source annotations remain next to the version. Redirected output and `NO_COLOR` use short annotations for non-current states. An installation from a different bucket does not mark a same-name result as installed. Unversioned nightly manifests remain unknown.

Search options have specific effects:

| Option | Behavior | Example |
| --- | --- | --- |
| `-e` / `--explicit` | Case-insensitive literal substring matching; regex characters lose their special meaning | `rsc search -e 'c++'` |
| `-N` / `--name-only` | Search package names only and skip reading unmatched manifests; excludes binary/alias matches | `rsc search -N git` |
| `-D` / `--with-description` | Include decoded description text and show a `Description` column | `rsc search -D editor` |

`-e` combines with either field option; `-N` and `-D` conflict. These optional modes search local buckets; the default mode keeps Scoop's known-bucket fallback when no local result exists. SQLite retains LIKE matching, while `-e` also escapes LIKE wildcards. Search rejects `-g` and `--arch`: installed markers cover both scopes, and Scoop's default binary search uses top-level entries. See `rsc search --help` for examples.

Versions are compared with local bucket manifests; search does not fetch updates. Bucket priority and duplicate names are retained. No persistent search cache is added.

## Options

Options follow the command they affect. The [command option audit](cli-options.md) lists every command and the implementation behind each retained option.

- `-g` / `--global` is available only for install, uninstall, update, cleanup, hold, unhold, list, prefix, which and shim. Global writes require administrator rights.
- `-a` / `--arch 64bit|32bit|arm64` is available only for install, download and depends. Update preserves the installed architecture.
- Install/update: `-i` / `--independent`, `-k` / `--no-cache`, `-s` / `--skip-hash-check`.
- Download: `-f` / `--force`, `-s` / `--skip-hash-check`.
- Update: `-f` / `--force`, `-a` / `--all`. Package options require package names or `--all`.
- Reset/cleanup: `-a` / `--all`. Uninstall: `-p` / `--purge`. Cleanup: `-k` / `--cache`.
- `alias list -v` / `--verbose` includes descriptions; other alias actions have no flags.
- `cache rm -a` / `--all` removes all cached downloads; `cache show` has no flags.
- VirusTotal: `-a` / `--all`, `-s` / `--scan` and `-n` / `--no-depends`.
- Ignored `--no-update-scoop`, update `--quiet` and VirusTotal `--passthru` were removed. Unknown options now fail before work begins. Arguments forwarded to custom aliases or shim targets remain user-controlled.
- Explicit package targets cannot be combined with `--all`.

Run `rsc help command`, or `rsc command --help`, for the applicable syntax and option descriptions.

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

Tables respect terminal width and Unicode display widths. Horizontal table headers, vertical detail labels and help headings use light blue exclusively for titles. The first table column uses bold bright magenta for the subject; bucket values use green. Installed versions and scopes are combined as `2.48.1 (user)` in search, list, info, depends and operation summaries. Status retains separate version and scope columns for comparison. Local installed versions use ordinary blue, distinct from the light blue headings. Search uses warning colors for non-current states. Colors stay on the version itself; scope suffixes are dim.

Download progress uses a full-width block bar with a spinner, percentage, transfer rate and precise ETA, adapting to narrow terminals. Unknown sizes use elapsed time instead of a fabricated percentage or ETA. Downloads from manifest creation use the same display. Completed transfers show a concise saved/cached summary.

`cat` highlights JSON keys, strings, numbers, literals and punctuation internally; setting `cat_style` retains the optional bat viewer. Redirected output and `NO_COLOR` disable colors; redirected progress uses plain event lines. Diagnostics and download progress go to stderr; paths, manifests and Scoopfiles remain suitable for stdout redirection.

`status` keeps its report on stdout: expected bucket updates are summarized before the package table, and check warnings are grouped afterward under `Checks needing attention`. Fatal errors still use stderr. This preserves report order even when a shell merges streams.

Failed commands return nonzero. VirusTotal reserves 2 for unsafe reports, 4 for request errors, 8 for unresolved manifests and 16 for a missing API key; report failures may combine these bits.
