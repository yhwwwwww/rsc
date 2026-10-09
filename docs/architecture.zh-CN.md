# 架构

[English — 主版本](architecture.md) · [README](../README.zh-CN.md)

## 实现边界

包管理行为全部由 Rust 实现。项目中没有 Scoop 源码目录、PowerShell 管理器、原版资源嵌入表或展开脚本的后端。

`rsc_core` 是静态链接进 `rsc.exe` 的 `rlib`。同一个程序也充当 shim 启动器。SQLite 和 TLS 实现链接在程序内，操作系统集成使用 Windows 系统库。

## 模块

| 模块 | 职责 |
| --- | --- |
| `main`、`commands`、`output` | 直接命令、错误状态、表格和进度 |
| `config`、`layout` | Scoop 配置、便携目录、用户及全局范围 |
| `bucket`、`database` | Git bucket 索引、兼容 SQLite 缓存和历史 |
| `manifest`、`package` | 架构字段、清单校验、安装状态 |
| `download`、`ftp` | HTTP 并发、分段、恢复、缓存、哈希、FTP |
| `manager` | 依赖图、锁、更新前下载、提交和恢复 |
| `native/query` | 解析、历史版本、正则查询、信息和特殊 URL |
| `native/lifecycle` | 解压、安装器调度、persist 和系统集成 |
| `native/windows`、`native/attributes` | 注册表、目录链接、进程检查、快捷方式、WinHTTP |
| `native/commands` | alias、shim、诊断、创建清单、VirusTotal |
| `native/scripts` | 软件清单及用户脚本的执行边界 |
| `native/metalink` | 原生 XML 下载重定向解析 |
| `shim` | 固定参数、调用参数、标准流和退出码转发 |

原生动作分发只是 Rust 函数边界，不会调用另一个包管理器。

## 安装事务

1. 解析清单及依赖，检查循环。
2. 选择架构，取得软件包锁。
3. 下载并校验全部文件。
4. 创建版本目录和安装未完成标记。
5. 解压，执行清单中的 pre-install 和 installer。
6. 创建 current、shim、快捷方式、模块及环境变量。
7. 连接持久数据，执行清单 post-install。
8. 原子写入安装信息，删除未完成标记。

更新在替换当前版本前先校验下载，保留旧版本。失败时尝试恢复旧入口。卸载默认保留 persist，显式 purge 才删除。

删除时识别 reparse point，只删除目录链接本身，不遍历目标。Scoop 创建的只读链接通过打开链接自身的句柄解除只读。

## 脚本边界

清单脚本和别名由软件作者或用户提供，执行这些代码可能需要 Windows PowerShell。自行编写的小型适配器提供 `dir`、`original_dir`、`persist_dir`、`manifest`、`architecture`、`global` 等变量。

已支持的辅助函数会把请求转回 Rust。管理决策、文件操作、下载和系统集成仍由 Rust 负责，适配器没有复制 Scoop 函数。任意脚本可能使用更多 Scoop 专用辅助函数，需要单独验证兼容性。

## 下载

文件并发、主机限制和 HTTP 分段分别控制。分段响应检查精确区间与资源标识，不支持可靠 Range 时回退单流。恢复信息记录 URL、预期哈希、长度和资源标识，校验后才发布到缓存。

Windows 认证代理使用 WinHTTP，FTP 使用 WinINet。ZIP 解压和 Metalink 解析内置，其他格式按包要求调用解压辅助工具。

参见[兼容状态](compatibility.zh-CN.md)和[开发说明](development.zh-CN.md)。

## 查询与输出

查询共用保序且限制并发数量的读取器，以及仅在单次命令内使用的文件名索引。状态检查重复使用安装信息和依赖名称。`presentation` 模块提供语义颜色与内置 JSON 分词器，根据完整内容判断重要程度，再按可识别 ANSI 的显示宽度裁剪，窄终端不会改变状态颜色。重定向及 `NO_COLOR` 保持纯文本。见[性能](performance.zh-CN.md)。
