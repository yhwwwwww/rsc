# 兼容状态

[English — 主版本](compatibility.md) · [README](../README.zh-CN.md)

rsc 以 [Scoop v0.6.0](https://github.com/ScoopInstaller/Scoop/tree/e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f) 为行为参考，是独立 Rust 实现，不包含 Scoop 代码。

## 已实现并验收

| 范围 | 证据 |
| --- | --- |
| 配置及路径共用 | 隔离目录、本机配置未改变、no_junction、环境恢复 |
| HTTP 并发、哈希、缓存 | Range、忽略及错误区间、重试、断线、恢复、私有请求头和 cookie |
| 软件生命周期 | 安装、依赖排序及循环、钩子、脚本安装器、失败修复和卸载 |
| persist | 目录链接、文件硬链接、改名、重装、更新和 purge |
| Windows 集成 | 原生环境变量、快捷方式、模块、exe/脚本/GUI shim |
| 版本管理 | Git 同步、SQLite 历史、固定版本、hold、更新、reset、强制重装和 cleanup |
| Scoopfile | 双向导入和导出 |
| 共用安装 | rsc 安装 → Scoop 查询/卸载；Scoop 安装 → rsc 查询/重置/卸载 |
| 真实软件 | jq 下载及运行；ripgrep ZIP 安装及运行 |

独立 native 测试阶段移除 Scoop 管理器资源后执行。源码审计和干净的发布历史保证不保留原版代码。具体结果见[测试](testing.zh-CN.md)。

## 有意差异

- 默认使用 Rust 并发和分段下载。保留 aria2 配置供 Scoop 使用，rsc 不启动 aria2。
- 输出使用 rsc 的表格、进度和诊断。
- Scoop 通过 Git 更新管理器自身的流程不在范围内，软件更新和 bucket 同步仍受支持。rsc 自身使用 kits 的普通软件包更新流程；安装方式及可执行文件被占用时的处理步骤见[升级 rsc](../README.zh-CN.md#升级-rsc)。

## 当前限制

已测试的核心流程可用，这一版尚未证明兼容所有 Scoop 清单及 Windows 部署。

- 正则使用 Rust fancy-regex，支持常用忽略大小写及前后查找；.NET 专有的平衡组等语法不等价。
- 历史清单支持 SQLite 和 Git。autoupdate 支持版本替换、URL/正则哈希查询，尚未覆盖任意 PowerShell 哈希生成表达式及所有模板变体。
- 清单适配器提供常用变量与辅助函数转发，并未重建 Scoop 导出的全部辅助函数。使用其他辅助函数的脚本需要补充实现和测试。
- checkup 展示已实现的原生检查，尚未复现全部专项诊断。
- create 通过自己的交互生成可用清单，提示顺序及所有 URL 推断规则不完全等价。
- VirusTotal 已有原生查询、提交及错误码路径，真实 API 限流和详细输出变体未验收。
- FTP、认证代理、FossHub/私有 release、MSI/Inno/WiX 有原生实现路径，仍需部署场景验证。
- ARM64/MSVC、声明的最低编译器版本和干净 Windows 尚未验收。

这些限制取代原先基于内嵌 Scoop 函数的兼容表述。旧混合实现的测试结果不能证明原生实现兼容。

## 数据处理

普通卸载保留持久数据，只有 `--purge` 删除。清理旧版本时不跟随 persist 链接，Scoop 创建的只读链接由 Windows API 处理。

更新先下载并校验，再移除活动入口，保留旧版本，失败时尝试恢复。未完成安装会标记状态，再次安装前修复或清理。

参见[计划](plan.zh-CN.md)、[架构](architecture.zh-CN.md)和[命令](cli.zh-CN.md)。
