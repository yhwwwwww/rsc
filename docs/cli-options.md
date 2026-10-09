# Command option audit

[简体中文](cli-options.zh-CN.md) · [Commands](cli.md) · [README](../README.md)

Each retained option is connected to an operation, filter, selected field or visible output change. `-h` / `--help` is available for built-in commands; the root `-V` / `--version` prints the program version.

| Command | Retained options | Actual effect |
| --- | --- | --- |
| `install` | `-g`, `-a/--arch`, `-i`, `-k`, `-s` | Installation root, selected manifest architecture, dependency plan, temporary download cache, hash checks |
| `uninstall` | `-g`, `-p` | Installation root; remove persistent data |
| `update` | `-g`, `-a/--all`, `-f`, `-i`, `-k`, `-s` | Package selection, reinstall, dependencies, cache, hashes; plain update only synchronizes buckets |
| `status` | `-l/--local` | Skip remote bucket fetch; package checks still run |
| `reset` | `-a/--all` | Reset all installed user/global packages; metadata determines each scope |
| `cleanup` | `-g`, `-a/--all`, `-k/--cache` | Select scope/packages; remove cache for deleted versions |
| `hold` | `-g` | Set hold in the selected installation |
| `unhold` | `-g` | Remove hold in the selected installation |
| `search` | `-e`, `-N`, `-D` | Literal matching, names only, include/show descriptions |
| `list` | `-g` | Filter to global installations; default includes both scopes |
| `info` | `-v` | Include architecture, binaries, dependencies and notes |
| `cat` | None | Show the selected manifest |
| `prefix` | `-g` | Choose the global installation directory |
| `which` | `-g` | Search only global shims instead of PATH plus Scoop shims |
| `config` | None | Positional read/set/rm operations |
| `bucket add/rm/list/known` | None | Positional bucket operations |
| `depends` | `-a/--arch` | Resolve dependencies for the selected architecture |
| `download` | `-a/--arch`, `-f`, `-s` | Select files, bypass cached files, skip hash checks |
| `cache` / `cache show` | None | List all or named package cache files |
| `cache rm` | `-a/--all` | Remove all files instead of named package files |
| `export` | `-c/--config` | Include Scoop settings in the Scoopfile |
| `import` | None | Scope and architecture come from the Scoopfile |
| `home` | None | Open the selected manifest homepage |
| `alias add/rm` | None | Positional alias operations |
| `alias list` | `-v` | Include descriptions in the displayed table |
| `shim add/rm/list/info/alter` | `-g` | Select global shims; target arguments follow `--` |
| `checkup` | None | Diagnose both roots, tools, metadata and user PATH |
| `create` | None | Positional URL/output and interactive manifest fields |
| `virustotal` | `-a`, `-s`, `-n` | Select all installed apps, submit missing URL reports, skip dependencies |
| `help` | None | Show help for a built-in command or action |

## Removed ignored options

- Shared `--arch` was removed from every command except install/download/depends; those three now share one `-a/--arch` argument.
- Shared `-g/--global` was removed from commands that do not use a selectable scope. Search/info/status inspect both scopes, reset uses installation metadata, import uses Scoopfile metadata, export includes both scopes, and bucket/cache/config actions do not select an installation scope.
- Install/download `-u/--no-update-scoop` was removed because rsc has no Scoop self-update step.
- Update `-q/--quiet` was removed because it did not control output.
- VirusTotal `-u/--no-update-scoop` and `-p/--passthru` were removed because neither changed CLI behavior.
- Unrestricted argument lists for alias/checkup/create/shim/virustotal were replaced with action-specific parsing. Unknown manager options and excess positional arguments are rejected. Custom alias and shim target arguments are forwarded intentionally.
- Package-only update flags require explicit package selection. `--all` cannot silently override supplied package names.
- `alias list -v` now changes the output by including descriptions, instead of being ignored.
- `cache --all` and `cache show --all` were removed; the useful form is `cache rm --all`.

Human version/scope formatting and the new palette are described in [Commands](cli.md). Default search matching remains aligned with Scoop.
