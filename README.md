# rsc

[简体中文](README.zh-CN.md)

A Windows package manager written in Rust, with Scoop-compatible commands, configuration, manifests, and installation directories.

**One executable. Native package management. Built-in parallel downloads.**

rsc is an independent implementation. It contains no Scoop source code and does not unpack or run Scoop's management scripts. The internal `rsc_core` library is statically linked into `rsc.exe`.

## What it does

- Install, uninstall, update, reset, hold, and clean up packages.
- Read existing Scoop installations and write compatible metadata.
- Manage Git buckets, pinned versions, dependencies, and Scoopfiles.
- Download multiple files concurrently, split reliable HTTP ranges, resume interrupted segments, and verify hashes.
- Create executable shims, directory junctions, persistent data links, Start Menu shortcuts, environment entries, and PowerShell module links through Rust and Windows APIs.
- Show readable tables, progress, speed, and useful failure details.

This is an early release. Tested behavior and remaining validation work are described in [Compatibility](docs/compatibility.md) and [Testing](docs/testing.md).

## Build

Requires a Windows Rust toolchain. Git and archive helpers are needed by packages that use them.

```powershell
.\scripts\build.ps1
.\dist\rsc.exe help
```

The distributed program is `dist\rsc.exe`. No rsc DLL, installed Scoop, extracted manager scripts, or separate runtime resource directory is required.

Windows PowerShell is invoked only for code supplied in package manifests or user aliases. rsc's independently written adapter provides script variables and forwards supported helper calls to Rust. It does not contain Scoop implementations. Development scripts under `scripts/` build and test the project; they are not the package manager backend.

## Use

```powershell
rsc bucket add main
rsc search ripgrep
rsc install ripgrep jq
rsc list
rsc update ripgrep
rsc reset ripgrep
rsc uninstall jq
rsc export > scoopfile.json
rsc import scoopfile.json
```

rsc uses the same configuration and directory layout as Scoop. With Scoop already installed, rsc discovers that installation. Global changes require an elevated terminal. Read [Compatibility](docs/compatibility.md) before sharing an existing installation.

The intended differences are the built-in multithreaded downloader and presentation. Manager self-update is outside the current scope; package updates and bucket synchronization are included.

## Documentation

- [Development plan](docs/plan.md)
- [Architecture](docs/architecture.md)
- [Commands and configuration](docs/cli.md)
- [Compatibility](docs/compatibility.md)
- [Building and development](docs/development.md)
- [Tests and results](docs/testing.md)

English documents are primary; each has a Simplified Chinese counterpart. Machine-readable build and test records use shared files.

## License

[GNU General Public License v3.0 only](LICENSE). Dependencies retain their respective licenses.

Behavioral reference: [ScoopInstaller/Scoop](https://github.com/ScoopInstaller/Scoop). This project is independently implemented and is not an official Scoop product.
