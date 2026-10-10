# rsc

[English — 主版本](README.md)

用 Rust 编写的 Windows 包管理器，兼容 Scoop 的命令、配置、清单和安装目录。

`rsc` 是 **Rust Scoop** 的缩写，使用缩写是为了在命令行中少打几个字。

项目的目标是完整兼容 Scoop 的行为。目前仅测试了作者个人常用的功能，尚未覆盖 Scoop 的全部功能和所有软件清单。

**单个可执行文件，内置多线程下载，清晰且丰富的彩色输出。**

rsc 可以共用已有的 Scoop 安装，也可以从独立的 `rsc.exe` 开始使用。它是独立实现，不包含 Scoop 源码；内部库静态链接到可执行文件中。

## 首次安装

当前发行版适用于 **Windows x64**。添加和同步 bucket 需要 PATH 中有可用的 Git。运行 rsc 无需安装 Rust。

### 已经在使用 Scoop

通过 [kits bucket](https://github.com/yhwwwwww/kits) 安装：

```powershell
scoop bucket add kits https://github.com/yhwwwwww/kits
scoop install kits/rsc
```

如果已有 kits，请跳过添加 bucket 的命令。打开新终端，即可开始使用 rsc：

```powershell
rsc --version
rsc bucket list
```

rsc 会发现已有的 Scoop 配置、bucket 和软件，无需迁移软件。如果 `rsc bucket list` 中没有 main，请先执行 `rsc bucket add main`。然后搜索并安装软件：

```powershell
rsc search ripgrep
rsc install ripgrep jq
```

### 没有安装 Scoop 的新环境

1. 安装 [Git for Windows](https://gitforwindows.org/)，确认在新终端中能执行 `git --version`。
2. 从 [Releases](https://github.com/yhwwwwww/rsc/releases/latest) 下载 `rsc.exe`，放入自己选择的文件夹，并在该文件夹中打开 PowerShell。
3. 添加提供常用命令行工具的 main，以及提供 rsc 的 kits：

   ```powershell
   .\rsc.exe bucket add main
   .\rsc.exe bucket add kits https://github.com/yhwwwwww/kits
   ```

4. 将 rsc 安装为受管理的软件包，方便以后用它自己的软件更新命令升级：

   ```powershell
   .\rsc.exe install kits/rsc
   ```

5. 从开始菜单打开新终端，使用已安装的命令：

   ```powershell
   rsc --version
   rsc install ripgrep jq
   rsc list
   ```

无需预先安装 Scoop。rsc 会创建必要的目录，并将软件的 shim 目录加入用户 PATH。下载的可执行文件可以留在受管理的安装目录之外，作为恢复用的副本；无需将它的目录加入 PATH。

### 希望一直使用便携版

完成上面的第 1–3 步后，直接使用下载的可执行文件即可：

```powershell
.\rsc.exe search ripgrep
.\rsc.exe install ripgrep jq
.\rsc.exe list
```

可以将它所在的文件夹加入用户 PATH，以后使用 `rsc`，无需写 `.\` 前缀。便携模式仍将软件安装到配置指定的 Scoop 兼容目录中。添加 kits 会提供 rsc 清单，但不会把下载的可执行文件登记成已安装的软件包。要启用软件包管理方式的升级，请执行 `.\rsc.exe install kits/rsc`；如果曾将便携版文件夹加入 PATH，请移除该条目，然后在新终端中使用已安装的命令。

### 添加其他 bucket

main 提供常用命令行工具；kits 当前提供 rsc，以后也可以收录其他工具。按需添加其他 bucket：

```powershell
rsc bucket known
rsc bucket add extras
rsc bucket list
```

extras 提供更多应用。自定义 bucket 使用 `rsc bucket add 名称 仓库地址`。请先查看 bucket 列表，跳过已经添加的 bucket。

## 日常使用

| 操作 | 命令 |
| --- | --- |
| 搜索软件 | `rsc search git` |
| 查看软件信息 | `rsc info ripgrep` |
| 查看带高亮的清单 | `rsc cat ripgrep` |
| 安装软件 | `rsc install ripgrep jq` |
| 列出已安装软件 | `rsc list` |
| 检查可用更新 | `rsc status` |
| 仅根据本地清单检查 | `rsc status --local` |
| 同步 bucket | `rsc update` |
| 升级一个软件 | `rsc update ripgrep` |
| 升级所有用户软件 | `rsc update '*'` |
| 锁定或解锁软件版本 | `rsc hold ripgrep` / `rsc unhold ripgrep` |
| 清理旧版本 | `rsc cleanup '*'` |
| 卸载软件 | `rsc uninstall jq` |
| 检查安装环境 | `rsc checkup` |
| 查看命令帮助 | `rsc help` / `rsc help install` |

`rsc update` 同步 bucket；带上软件名称时，还会在同步后升级相应软件。`rsc status` 检查远程 bucket 是否有更新，并使用本地清单对比软件版本；`--local` 跳过远程检查。要检查最新的软件版本，请先用 `rsc update` 同步清单。

搜索结果会标出已安装版本和安装范围，例如 `2.48.1 (user)`。版本号的颜色表示最新、过期、比清单更新或安装异常等状态。详细说明见[命令与配置](docs/cli.zh-CN.md)。

通过 Scoopfile 导出或恢复软件清单：

```powershell
rsc export > scoopfile.json
rsc import scoopfile.json
```

全局软件操作需要管理员终端和 `-g`，例如 `rsc install -g jq`。普通用户安装无需管理员权限。

## 升级 rsc

### 受管理的安装

从 kits 安装的 rsc，包括最初使用 Scoop 安装的版本，优先使用 rsc 自己的更新命令：

```powershell
rsc update rsc
```

这个命令会同步 bucket 并升级已安装的 rsc 软件包，无需另外执行 `scoop update`。如果旧发行版显示的版本号只有 `0.1.0`，首次改用 Git 版本格式时，请执行一次 `rsc update -f rsc`。

如果 Windows 提示 rsc 正在运行或可执行文件被占用，请从 [Releases](https://github.com/yhwwwwww/rsc/releases/latest) 下载另一份便携版，放在受管理的安装目录之外。在它所在的文件夹中打开终端，执行：

```powershell
.\rsc.exe update rsc
```

更新完成后继续使用已安装的 `rsc` 命令。全局安装的 rsc 需要在管理员终端中升级，并加上 `-g`。

### 便携安装

未登记为软件包的便携版，需要下载新发行版，在程序未运行时替换旧的 `rsc.exe`。如果希望今后通过 `rsc update rsc` 升级，请先按照上面的首次安装步骤，将它安装为受管理的软件包。

## 配置和目录

rsc 读取 Scoop 的配置，并使用相同的软件目录结构。默认设置下：

- 用户软件、bucket、shim 和缓存位于 `%USERPROFILE%\scoop`。
- 共用的 Scoop 配置位于 `%USERPROFILE%\.config\scoop\config.json`。
- rsc 下载配置位于 `%USERPROFILE%\.config\rsc\config.json`。

已有配置、自定义安装根目录和 Scoop 环境变量都会生效。使用 `rsc config` 查看当前设置。共用已有安装前，请阅读[命令与配置](docs/cli.zh-CN.md#配置)和[兼容状态](docs/compatibility.zh-CN.md)。

## 功能与兼容性

- 原生实现软件安装、卸载、更新、版本切换、锁定、依赖处理和清理。
- 内置并发下载、服务器支持时的 HTTP 分段、中断恢复、缓存及哈希校验。
- 兼容 Scoop 的清单、安装信息、持久数据、shim、快捷方式、环境变量和 Scoopfile。
- 清晰的表格、语义颜色、JSON 高亮、下载进度、传输速度和错误详情。

项目仍处于早期阶段。已验证的行为与待验收项见[兼容状态](docs/compatibility.zh-CN.md)和[测试](docs/testing.zh-CN.md)。

## 文档

- [命令与配置](docs/cli.zh-CN.md)
- [兼容状态](docs/compatibility.zh-CN.md)
- [查询性能](docs/performance.zh-CN.md)
- [测试与结果](docs/testing.zh-CN.md)
- [开发计划](docs/plan.zh-CN.md)
- [架构](docs/architecture.zh-CN.md)
- [命令选项核对](docs/cli-options.zh-CN.md)
- [构建与开发](docs/development.zh-CN.md)

英文文档为主版本，每份文档都有简体中文版本。机器可读的构建和测试记录共用一份文件。

## 从源码构建

需要 Windows Rust 工具链、带有仓库标签的 Git，以及 Python 3.11 或更新版本：

```powershell
.\scripts\build.ps1
.\dist\rsc.exe help
```

生成文件为 `dist\rsc.exe`，不需要 rsc DLL 或单独的运行资源目录。环境准备见[构建与开发](docs/development.zh-CN.md)。

## 许可证

[GNU GPL v3.0 only](LICENSE)。依赖保留各自的许可证。

行为参考：[ScoopInstaller/Scoop](https://github.com/ScoopInstaller/Scoop)。本项目为独立实现，并非 Scoop 官方产品。
