# 发布与安装

[English — 主版本](releasing.md) · [README](../README.zh-CN.md)

rsc 以单个 `rsc.exe` 分发。目前验证的构建为 Windows x64，用户无需安装 Rust 或单独的 rsc 库。需要 Git 或解压辅助工具的操作仍使用相应工具。

## 安装本地构建

在仓库目录执行：

```powershell
.\scripts\build.ps1
.\dist\rsc.exe help
```

便携使用时，将 `dist\rsc.exe` 复制到固定目录，把该目录加入 **用户 PATH**，打开新终端执行 `rsc help`。本机已有 Scoop 时，rsc 会发现它的配置和目录。

如果希望由 Scoop 管理本地可执行文件，可以生成本地清单。版本须与 `Cargo.toml` 和构建的程序一致：

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

这是首次安装命令；`scoop install` 不会替换已有的 rsc 安装。本地清单在后续重新安装时仍需要能够访问原构建文件。

## 准备 GitHub Release

1. 在 `Cargo.toml` 中确定版本，提交需要发布的源码，再执行一次 `.\scripts\build.ps1`。
2. 按[测试说明](testing.zh-CN.md)验收该可执行文件。
3. 推送需要发布的源码提交，用 **Release 下载地址** 生成清单。版本须与程序一致：

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

4. 发布 Release。以下命令上传附件；标签尚不存在时，在指定的提交上创建标签：

```powershell
$releaseCommit = (git rev-parse HEAD).Trim()
gh release create $releaseTag `
    .\dist\rsc.exe .\dist\rsc.json .\dist\SHA256SUMS .\dist\LICENSE `
    --repo yhwwwwww/rsc --target $releaseCommit `
    --title "rsc $releaseVersion" --generate-notes
```

对应提交必须已经推送。当前开发分支为 `feat/native-rust`，默认分支为 `main`，因此需要指定 `--target`。标签提供对应的源码快照。已发布的版本附件应保持稳定，后续改动使用新版本。参见 [GitHub CLI 发布文档](https://cli.github.com/manual/gh_release_create)。

## 安装已发布版本

发布 `v0.1.0` 及其附件之后：

```powershell
scoop install https://github.com/yhwwwwww/rsc/releases/download/v0.1.0/rsc.json
rsc help
```

Scoop 下载可执行文件、校验哈希并创建命令 shim。清单格式参照 [Scoop 清单文档](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)。

便携安装时，从 [Releases 页面](https://github.com/yhwwwwww/rsc/releases)下载 `rsc.exe` 和 `SHA256SUMS`，将 `Get-FileHash .\rsc.exe -Algorithm SHA256` 的结果与公布的校验值比较，再把程序所在目录加入用户 PATH。

上述 Release 清单地址绑定具体版本。希望正常使用 Scoop 升级时，需要在已发布的 Scoop bucket 中维护最新的 `rsc.json`；用户添加该 bucket 后，可执行 `scoop update rsc`。仅发布 Release 附件清单并不会提供管理器自动升级。参见 [Scoop bucket 文档](https://github.com/ScoopInstaller/Scoop/wiki/Buckets)。

rsc 自身的管理器升级尚未实现。便携版升级时，在程序未运行时替换 exe；已安装软件的数据仍保留在原 Scoop 目录。
