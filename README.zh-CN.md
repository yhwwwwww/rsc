# rsc

[English — 主版本](README.md)

用 Rust 编写的 Windows 包管理器，兼容 Scoop 的命令、配置、清单和安装目录。

**单个可执行文件，原生包管理，内置多线程下载。**

rsc 是独立实现，项目不包含 Scoop 源码，也不会展开或运行 Scoop 的管理脚本。内部 `rsc_core` 库静态链接到 `rsc.exe`。

## 功能

- 安装、卸载、更新、重置、锁定和清理软件。
- 读取已有 Scoop 安装，写入兼容的安装信息。
- 管理 Git bucket、固定版本、依赖和 Scoopfile。
- 多文件并发下载；可靠支持 HTTP Range 时分段下载，支持中断恢复和哈希校验。
- 通过 Rust 和 Windows API 创建 shim、目录链接、持久数据链接、开始菜单快捷方式、环境变量及 PowerShell 模块链接。
- 为名称、版本、正常状态、警告和错误提供语义颜色，内置 JSON 清单语法高亮。
- 使用与 Scoop 一致的名称/二进制搜索，展示匹配的二进制，并按来源区分已安装版本及更新状态。参见[命令](docs/cli.zh-CN.md)和[性能](docs/performance.zh-CN.md)。
- 使用更清晰的表格、下载进度、速度和错误信息。

项目仍处于早期阶段。已验证的行为与待验收项见[兼容状态](docs/compatibility.zh-CN.md)和[测试](docs/testing.zh-CN.md)。

## 构建

需要 Windows Rust 工具链。需要 Git 或解压辅助工具的软件包仍使用相应工具。

```powershell
.\scripts\build.ps1
.\dist\rsc.exe help
```

交付文件是 `dist\rsc.exe`，不需要 rsc DLL、预装 Scoop、展开的管理脚本或单独的运行资源目录。

Windows PowerShell 仅用于执行软件清单或用户别名提供的代码。rsc 自行编写的适配器提供脚本变量，将支持的辅助函数调用转发给 Rust，不包含 Scoop 的实现。`scripts/` 中的开发脚本只用于构建和测试。

## 使用

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

rsc 使用 Scoop 的配置和目录结构，能发现本机已有的 Scoop 安装。全局操作需要管理员终端。共用已有安装前，请阅读[兼容状态](docs/compatibility.zh-CN.md)。

有意改变的部分是内置多线程下载与输出排版。管理器自身升级不在当前范围内，软件包更新和 bucket 同步包含在内。

## 文档

- [开发计划](docs/plan.zh-CN.md)
- [架构](docs/architecture.zh-CN.md)
- [命令与配置](docs/cli.zh-CN.md)
- [命令选项核对](docs/cli-options.zh-CN.md)
- [兼容状态](docs/compatibility.zh-CN.md)
- [构建与开发](docs/development.zh-CN.md)
- [测试与结果](docs/testing.zh-CN.md)
- [查询性能](docs/performance.zh-CN.md)

英文文档为主版本，每份文档都有简体中文版本。机器可读的构建和测试记录共用一份文件。

## 许可证

[GNU GPL v3.0 only](LICENSE)。依赖保留各自的许可证。

行为参考：[ScoopInstaller/Scoop](https://github.com/ScoopInstaller/Scoop)。本项目为独立实现，并非 Scoop 官方产品。
