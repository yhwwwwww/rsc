# 开发计划

[English — 主版本](plan.md) · [README](../README.zh-CN.md)

## 目标与范围

用 Rust 建立 Windows 包管理器，交付单个 `rsc.exe`，内部库静态链接。行为、配置、软件清单和目录结构兼容 Scoop。有意改变的部分是内置多线程下载和更清晰的输出。

仓库、程序和发布的 Git 历史均不包含 Scoop 源码。参考仓库和对比资料放在项目外。PowerShell 可执行清单钩子和用户别名，通过自行编写的适配器提供上下文，不充当包管理后端。

行为基线为 [Scoop v0.6.0，提交 e6aa3b3](https://github.com/ScoopInstaller/Scoop/tree/e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f)。Scoop 通过 Git 更新管理器自身的流程不在范围内，软件包更新和 bucket 同步包含在内。rsc 自身通过 kits 作为普通软件包分发，使用 `rsc update rsc` 升级。受管理安装与便携版的升级步骤见 [README](../README.zh-CN.md#升级-rsc)。

## 里程碑

| 阶段 | 交付 | 验收 |
| --- | --- | --- |
| 基础 | Cargo 程序及静态库、配置、目录、输出 | 独立发布文件及依赖审计 |
| 查询 | 清单、bucket 搜索、安装状态、架构、SQLite | 真实目录和隔离目录查询 |
| 下载 | 并发、分段、重试、恢复、哈希、兼容缓存 | HTTP 故障测试和真实下载 |
| 生命周期 | 依赖、安装、persist、shim、卸载、恢复 | 原生集成与 Scoop 双向操作 |
| Windows | 目录链接、注册表、快捷方式、模块、GUI/脚本启动、进程检查 | 链接删除安全和执行测试 |
| 管理 | Git bucket、历史、更新、hold、reset、cleanup、Scoopfile | 历史版本、失败恢复和互导 |
| 辅助命令 | alias、shim、checkup、create、VirusTotal | 命令流程和外部服务验证 |
| 发布 | GPLv3、英文主文档和中文翻译 | 干净源码历史、公开仓库、准确测试记录 |

## 兼容契约

- 环境变量 `SCOOP`、`SCOOP_GLOBAL`、`SCOOP_CACHE` 优先于配置路径。
- 支持 XDG、标准及可靠识别的便携配置目录。
- 保留未知配置及安装信息，兼容键名大小写。
- 使用 Scoop 的 apps/version、current、persist、shims、buckets、modules 和 cache 结构。
- 写入新安装信息文件，兼容读取新旧文件名。
- 支持架构字段、依赖、bin 别名、解压目录、安装器、钩子、notes 和 suggest。
- 更新、重置、清理和普通卸载保留用户持久数据。
- 替换旧版本入口前先完成下载和校验。
- 安装未完成、哈希错误、脚本错误和批量失败返回非零。
- 保留可执行文件参数、标准流及退出码。

## 验收工作

原生 Windows 测试覆盖核心生命周期和常见便携软件。认证代理、FTP、FossHub、私有 GitHub release、MSI/Inno/WiX、特殊 autoupdate 模板、不常见清单辅助函数、外部服务限流、ARM64/MSVC 和干净 Windows 仍需更多验证。

有实现路径不等于已经验证。具体限制见[兼容状态](compatibility.zh-CN.md)，已完成检查见[测试](testing.zh-CN.md)。

## 文档

[架构](architecture.zh-CN.md) · [命令](cli.zh-CN.md) · [开发](development.zh-CN.md) · [测试](testing.zh-CN.md)

## 输出与查询优化

已实现正文语义颜色、内置 JSON 高亮、限制并发数量的清单读取和状态元数据复用。查询结果一致性核对及耗时见[性能](performance.zh-CN.md)，最新回归结果见[测试](testing.zh-CN.md)。
