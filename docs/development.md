# Building and development

[简体中文](development.zh-CN.md) · [README](../README.md)

## Requirements

Windows, a Rust toolchain, Git with the repository's complete tag history, and Python 3.11 or later are required for the build scripts. Git is also used for buckets; package-specific extraction tools are discovered in package directories or PATH. Python runs version synchronization, release packaging, and the integration suite. The comparison phase requires an installed Scoop, but rsc itself does not.

The package declares Rust 1.85 as its source-language minimum. The validated compiler version and target are recorded in [build-info.json](build-info.json); minimum-toolchain and other-target builds need separate validation.

## Build

```powershell
.\scripts\build.ps1
.\scripts\build.ps1 -Profile debug
```

The script uses a regular Rust installation, or an optional project-local GNU toolchain under `.tools`. To provision the latter:

```powershell
.\scripts\setup-dev.ps1
```

The setup script downloads Rust and a compiler toolchain. It is a development utility, not a runtime component.

Release output is `dist\rsc.exe`. `rsc_core` uses `crate-type = ["rlib"]`. SQLite is bundled and the Windows CRT is configured for static linking. Only Windows system DLLs should appear in the release import table.

## Git-derived versions

The version source is `git describe --tags`. Both `rsc --version` and the download user agent embed its complete result, such as `v0.1.0-3-gabcdef0`. Git is needed at build time, not for printing the version at runtime.

`scripts/build.ps1` runs `scripts/version.py --sync` before Cargo. It automatically synchronizes the root package version in `Cargo.toml` and `Cargo.lock`; only a leading `v` is removed to satisfy Cargo's SemVer format. Do not bump either file manually. Use the build script so Cargo metadata is synchronized before Cargo reads it. The test script performs the same synchronization.

`build.rs` embeds the Git description and watches Git references so rebuilding after commits or tags updates the runtime version. The source checkout must have a reachable version tag; fetch full history and tags when working from a shallow clone. Tags should use SemVer names, normally `vMAJOR.MINOR.PATCH`.

For example, a build after three commits beyond `v0.1.0` reports `v0.1.0-3-gabcdef0`, while Cargo uses `0.1.0-3-gabcdef0`. For a named stable version, create and push its Git tag on the intended main commit; the next build uses that tag automatically.

## Tests

```powershell
.\scripts\test.ps1
.\scripts\test.ps1 -SkipBuild -Phase native
```

Available phases: `network`, `lifecycle`, `native`, `real`, `all`.

The native phase removes the reference manager's resources before exercising rsc. The comparison phases invoke the separately installed Scoop. Reference supporting files and isolated package data are stored in a system temporary directory, outside the source repository.

Tests restore the relevant user environment registry values and verify that the live Scoop configuration is unchanged. Test logs identify the isolated directory. Local fixture binaries and the latest-results pointer are ignored under `.test-lab`.

## Source policy

- Implement behavior independently in Rust.
- Keep Scoop checkouts, scripts and copied manager resources outside this project.
- Do not introduce a PowerShell backend, embedded upstream source, or runtime script extraction.
- Manifest/user scripts are permitted inputs; independently written context and helper adapters belong in `native/scripts`.
- Keep new public documentation bilingual: English primary, Chinese counterpart, matching local-language links.
- Verify behavior with fixtures and upstream comparison; never mark a feature verified merely because it compiles.
- Publish only clean history that excludes the discarded upstream-script implementation.

[Architecture](architecture.md) · [Compatibility](compatibility.md) · [Testing](testing.md)

## Query benchmarks

Run `python scripts/benchmark_queries.py after hok` after building the release executable. The optional previous executable can be passed with `--before`. See [Performance](performance.md) for methodology and raw records.

For the installed search comparison, run `.\scripts\benchmark_search.ps1`. It uses five warmups and twenty runs of rsc, Hok's default and binary search, and Scoop. The read-only Scoop result comparison is `python tests/search_scoop.py`.

## GitHub Actions release

Open [Actions → Release](https://github.com/yhwwwwww/rsc/actions/workflows/release.yml), click **Run workflow**, select **main**, and run it. The workflow checks out complete history and tags, then reads `git describe --tags`. No manual Cargo version change is required. A new commit produces a Git description that can be published; a new SemVer tag selects a named version.

Three jobs build Windows x64 with MSVC and a static CRT, publish a GitHub Release, and update [the separate Scoop bucket](https://github.com/yhwwwwww/kits). Published assets are `rsc.exe`, `rsc.json`, `SHA256SUMS`, `LICENSE`, and `build-info.json`. One frozen Git description supplies the executable version, release tag, build metadata, bucket manifest version, and download URL. Cargo receives the same version without its leading `v`. The manifest's checkver reads the full GitHub release tag and its autoupdate URL uses `$version` directly, preserving commit-distance/hash suffixes and avoiding an extra `v`. Packaging verifies the executable version, synchronized Cargo metadata, Windows system DLL imports, and manifest checksum. This workflow builds and packages; the separately documented test suite records behavioral validation.

Only the publishing job has `contents: write` for this repository. The bucket job uses `SCOOP_BUCKET_DEPLOY_KEY`, an Actions secret holding a dedicated SSH private key. Its public key is configured as a write-enabled deploy key on `yhwwwwww/kits`. The key cannot write to other repositories. Official actions are pinned to commits.

The release is initially a draft; assets are uploaded before publication. Bucket updates verify the downloaded release binary against the checksum. Published versions are not overwritten. Source ancestry prevents an older or unrelated source commit from replacing the bucket; an existing version cannot receive a different binary. If only the bucket job fails, rerun that failed job from the Actions run; do not start a new full release for the same version. A draft can be retried from the same source commit.

The workflow must remain on the default `main` branch for GitHub's manual-run button. The two local `docs/releasing*.md` notes are intentionally excluded from Git.
