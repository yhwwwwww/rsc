# rsc

[简体中文](README.zh-CN.md)

A Windows package manager written in Rust, with Scoop-compatible commands, configuration, manifests, and installation directories.

**One executable. Built-in parallel downloads. Clear, colorful output.**

rsc works with an existing Scoop installation or starts from a standalone `rsc.exe`. It is an independent implementation with no Scoop source code; its internal library is statically linked into the executable.

## First install

The current release is for **Windows x64**. Git must be available in PATH to add or synchronize buckets. Running rsc does not require Rust.

### If you already use Scoop

Install rsc through the [kits bucket](https://github.com/yhwwwwww/kits):

```powershell
scoop bucket add kits https://github.com/yhwwwwww/kits
scoop install kits/rsc
```

Skip the bucket-add command if kits is already present. Open a new terminal, then start using rsc:

```powershell
rsc --version
rsc bucket list
```

rsc discovers your Scoop configuration, buckets, and installed packages. There is no package migration step. If main is missing from `rsc bucket list`, add it with `rsc bucket add main`. Then find and install packages:

```powershell
rsc search ripgrep
rsc install ripgrep jq
```

### On a new machine without Scoop

1. Install [Git for Windows](https://gitforwindows.org/) and ensure `git --version` works in a new terminal.
2. Download `rsc.exe` from [Releases](https://github.com/yhwwwwww/rsc/releases/latest) into a folder of your choice. Open PowerShell in that folder.
3. Add the buckets for general command-line tools and rsc:

   ```powershell
   .\rsc.exe bucket add main
   .\rsc.exe bucket add kits https://github.com/yhwwwwww/kits
   ```

4. Install rsc as a managed package so it can be upgraded with its own package commands:

   ```powershell
   .\rsc.exe install kits/rsc
   ```

5. Open a new terminal from the Start menu and use the installed command:

   ```powershell
   rsc --version
   rsc install ripgrep jq
   rsc list
   ```

Scoop does not need to be installed. rsc creates the required directories and adds the package shim directory to your user PATH. The downloaded executable can be kept outside the managed installation as a recovery copy; it does not need to be added to PATH.

### If you want to keep rsc portable

Follow steps 1–3 above and continue running the downloaded executable directly:

```powershell
.\rsc.exe search ripgrep
.\rsc.exe install ripgrep jq
.\rsc.exe list
```

You can add its folder to user PATH to use `rsc` without the `.\` prefix. Portable mode still installs packages into the configured Scoop-compatible directories. Adding kits provides the rsc manifest; it does not register the downloaded executable as an installed package. To enable package-managed upgrades, run `.\rsc.exe install kits/rsc`, remove the portable folder from PATH if you added it, and use the installed command in a new terminal.

### Add more buckets

main provides common command-line tools; kits carries rsc and other tools as they are added. You can add more buckets as needed:

```powershell
rsc bucket known
rsc bucket add extras
rsc bucket list
```

extras provides additional applications. For a custom bucket, use `rsc bucket add name repository-url`. Check the bucket list first and skip buckets that are already present.

## Everyday use

| Task | Command |
| --- | --- |
| Find packages | `rsc search git` |
| Show package details | `rsc info ripgrep` |
| Read a highlighted manifest | `rsc cat ripgrep` |
| Install packages | `rsc install ripgrep jq` |
| List installed packages | `rsc list` |
| Check available updates | `rsc status` |
| Check using local manifests only | `rsc status --local` |
| Synchronize buckets | `rsc update` |
| Upgrade one package | `rsc update ripgrep` |
| Upgrade all user packages | `rsc update '*'` |
| Hold or unhold a package | `rsc hold ripgrep` / `rsc unhold ripgrep` |
| Remove old versions | `rsc cleanup '*'` |
| Uninstall a package | `rsc uninstall jq` |
| Check the installation environment | `rsc checkup` |
| Get command help | `rsc help` / `rsc help install` |

`rsc update` synchronizes buckets. Supplying package names also upgrades those packages after synchronization. `rsc status` checks remotes for bucket updates and compares packages with local manifests; `--local` skips the remote checks. Run `rsc update` first to synchronize manifests before checking the latest available package versions.

Search results mark installed versions and their scope, such as `2.48.1 (user)`. Version colors indicate whether the installation is current, outdated, newer, or broken. See [Commands and configuration](docs/cli.md) for details.

Export or restore your package selection with a Scoopfile:

```powershell
rsc export > scoopfile.json
rsc import scoopfile.json
```

Global package changes require an elevated terminal and `-g`, for example `rsc install -g jq`. Ordinary user installations do not require administrator rights.

## Update rsc

### Managed installation

For rsc installed from kits, including an installation originally made with Scoop, prefer rsc's own update command:

```powershell
rsc update rsc
```

This synchronizes buckets and upgrades the installed rsc package. A separate `scoop update` command is unnecessary.

If Windows reports that rsc is running or its executable is in use, open a terminal in a folder containing a separate portable copy from [Releases](https://github.com/yhwwwwww/rsc/releases/latest), outside the managed installation, and run:

```powershell
.\rsc.exe update rsc
```

Use the installed `rsc` command again after the update finishes. For a global rsc installation, use an elevated terminal and add `-g`.

### Portable installation

An unregistered portable executable is upgraded by downloading the new release and replacing the old `rsc.exe` while it is not running. For future upgrades through `rsc update rsc`, first install it as a managed package using the first-install instructions above.

## Configuration and directories

rsc reads Scoop's configuration and uses its package layout. With default settings:

- User packages, buckets, shims, and cache live under `%USERPROFILE%\scoop`.
- Shared Scoop settings live in `%USERPROFILE%\.config\scoop\config.json`.
- rsc download settings live in `%USERPROFILE%\.config\rsc\config.json`.

Existing configuration, custom roots, and Scoop environment variables are honored. Use `rsc config` to inspect the active settings. See [Commands and configuration](docs/cli.md#configuration) and [Compatibility](docs/compatibility.md) before sharing an existing installation.

## Features and compatibility

- Native package installation, removal, updates, version activation, holds, dependencies, and cleanup.
- Built-in concurrent downloads, HTTP range splitting where supported, interrupted-download recovery, caching, and hash verification.
- Scoop-compatible manifests, installation metadata, persistent data, shims, shortcuts, environment entries, and Scoopfiles.
- Readable tables, semantic colors, JSON highlighting, download progress, transfer rates, and error details.

This is an early release. Tested behavior and remaining validation work are described in [Compatibility](docs/compatibility.md) and [Testing](docs/testing.md).

## Documentation

- [Commands and configuration](docs/cli.md)
- [Compatibility](docs/compatibility.md)
- [Query performance](docs/performance.md)
- [Tests and results](docs/testing.md)
- [Development plan](docs/plan.md)
- [Architecture](docs/architecture.md)
- [Command option audit](docs/cli-options.md)
- [Building and development](docs/development.md)

English documents are primary; each has a Simplified Chinese counterpart. Machine-readable build and test records use shared files.

## Build from source

Requires a Windows Rust toolchain:

```powershell
.\scripts\build.ps1
.\dist\rsc.exe help
```

The output is `dist\rsc.exe`, with no rsc DLL or separate runtime resource directory. See [Building and development](docs/development.md) for setup.

## License

[GNU General Public License v3.0 only](LICENSE). Dependencies retain their respective licenses.

Behavioral reference: [ScoopInstaller/Scoop](https://github.com/ScoopInstaller/Scoop). This project is independently implemented and is not an official Scoop product.
