# Publishing and installation

[简体中文](releasing.zh-CN.md) · [README](../README.md)

rsc is distributed as one `rsc.exe`. The current validated build is Windows x64; users do not need Rust or a separate rsc library. Git and package-specific extraction tools remain necessary for operations that use them.

## Install from a local build

From the repository directory:

```powershell
.\scripts\build.ps1
.\dist\rsc.exe help
```

For portable use, copy `dist\rsc.exe` into a permanent folder and add that folder to your **user PATH**. Open a new terminal and run `rsc help`. With Scoop installed, rsc discovers its existing configuration and directories.

To let Scoop manage the local executable, generate a local manifest. The version must match `Cargo.toml` and the built executable:

```powershell
$releaseVersion = '0.1.0'
$binary = (Resolve-Path .\dist\rsc.exe).Path
$binaryHash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
$manifest = [ordered]@{
    version = $releaseVersion
    description = 'A Windows package manager compatible with Scoop'
    homepage = 'https://github.com/yhwwwwww/rsc'
    license = 'GPL-3.0-only'
    architecture = @{
        '64bit' = @{ url = ([Uri]$binary).AbsoluteUri; hash = $binaryHash }
    }
    bin = 'rsc.exe'
}
[IO.File]::WriteAllText(
    (Join-Path (Get-Location) 'dist\rsc.json'),
    ($manifest | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
scoop install .\dist\rsc.json
rsc help
```

This is a first-install command; `scoop install` does not replace an existing rsc installation. The local manifest needs the build file to remain accessible for future reinstalls.

## Prepare a GitHub Release

1. Choose the version in `Cargo.toml`, commit the intended source, and build once with `.\scripts\build.ps1`.
2. Validate that executable using the checks in [Testing](testing.md).
3. Push the source commit you intend to release. Generate a manifest using the **release URL**, not a local file URL. Set the version to match the executable:

```powershell
$releaseVersion = '0.1.0'
$releaseTag = "v$releaseVersion"
$releaseUrl = "https://github.com/yhwwwwww/rsc/releases/download/$releaseTag"
$binaryHash = (Get-FileHash .\dist\rsc.exe -Algorithm SHA256).Hash.ToLowerInvariant()
$manifest = [ordered]@{
    version = $releaseVersion
    description = 'A Windows package manager compatible with Scoop'
    homepage = 'https://github.com/yhwwwwww/rsc'
    license = 'GPL-3.0-only'
    architecture = @{
        '64bit' = @{ url = "$releaseUrl/rsc.exe"; hash = $binaryHash }
    }
    bin = 'rsc.exe'
}
[IO.File]::WriteAllText(
    (Join-Path (Get-Location) 'dist\rsc.json'),
    ($manifest | ConvertTo-Json -Depth 5),
    [Text.UTF8Encoding]::new($false)
)
"$binaryHash  rsc.exe" | Set-Content .\dist\SHA256SUMS -Encoding ascii
Copy-Item .\LICENSE .\dist\LICENSE
```

4. Publish the release. This command uploads the assets and creates the tag at the specified commit if needed:

```powershell
$releaseCommit = (git rev-parse HEAD).Trim()
gh release create $releaseTag `
    .\dist\rsc.exe .\dist\rsc.json .\dist\SHA256SUMS .\dist\LICENSE `
    --repo yhwwwwww/rsc --target $releaseCommit `
    --title "rsc $releaseVersion" --generate-notes
```

The commit must already be pushed. Supplying `--target` matters when releasing from `feat/native-rust` rather than the default `main` branch. The tag provides the corresponding source snapshot. Keep published versioned assets stable and use a new version for later changes. See the [GitHub CLI release reference](https://cli.github.com/manual/gh_release_create).

## Install a published release

After the `v0.1.0` release and its assets have been published:

```powershell
scoop install https://github.com/yhwwwwww/rsc/releases/download/v0.1.0/rsc.json
rsc help
```

Scoop downloads the executable, checks its hash and creates the command shim. The manifest format follows the [Scoop manifest reference](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests).

For portable installation, download `rsc.exe` and `SHA256SUMS` from the [Releases page](https://github.com/yhwwwwww/rsc/releases), compare `Get-FileHash .\rsc.exe -Algorithm SHA256` with the published checksum, and add its folder to user PATH.

The release URL above is version-specific. For normal Scoop-driven upgrades, maintain an up-to-date `rsc.json` in a published Scoop bucket; users can then add the bucket and run `scoop update rsc`. A release-attached manifest alone does not provide automatic manager upgrades. See the [Scoop bucket reference](https://github.com/ScoopInstaller/Scoop/wiki/Buckets).

rsc's own manager self-update is outside the current implementation. Portable upgrades replace the executable while it is not running; package installation data stays in the existing Scoop directories.
