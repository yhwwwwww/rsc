# 测试与结果

[English — 主版本](testing.md) · [README](../README.zh-CN.md)

## 原生实现基线

已弃用内嵌 Scoop 脚本的旧版本。旧测试结果归档在项目外，不计入原生重写的验收。

独立 Rust 版本在 Windows GNU x64 工具链上测试。发布文件、编译器、校验值和导入 DLL 见 [build-info.json](build-info.json)，最新完整结果见 [test-results.json](test-results.json)。

## 最新结果

原生发布版通过 **19 项 Rust 测试和 32 组 Windows 集成测试**，没有失败。本机旧程序已替换为该发布版，安装文件与交付文件校验值一致。

## 运行

```powershell
.\scripts\test.ps1
.\scripts\test.ps1 -SkipBuild -Phase native
```

`test.ps1` 执行 Rust 测试，编译自行编写的测试程序，再运行 Python 集成套件。隔离目录使用系统临时目录，包含空格和中文。测试保留本机 Scoop 配置及相关用户环境注册表值。

## 覆盖范围

| 分组 | 检查 |
| --- | --- |
| Rust | 命令分发、架构回退/拒绝、文件名、版本顺序、正则校验、UTF-16 安装信息、变量展开、只读链接删除、Metalink 解析 |
| 网络 | 并发分段、缓存/强制下载、忽略或错误 Range、重试、断线、哈希错误、请求头/cookie、强制结束后恢复、缓存清理 |
| 生命周期 | Git bucket、依赖/循环、钩子、脚本安装器、目录 persist、环境、快捷方式、模块、shim 和退出码 |
| 版本 | hold/unhold、bucket 更新、更新、强制/不使用缓存、reset、cleanup、SQLite/Git 固定版本 |
| 恢复 | 钩子失败、不完整安装信息、Scoop 空安装目录 |
| 互用 | 双向安装/重置/卸载、旧信息、no_junction、Scoopfile 导入导出 |
| 独立原生 | 无 Scoop 资源、多文件解压、解压目录、文件 persist、辅助函数转发、脚本/GUI shim、Metalink 文件校验、本地文件 URL |
| 真实软件 | jq 和 ripgrep，真实下载及执行 |

对照测试调用本机单独安装的 Scoop。复制的 supporting 资源仅放在项目外的临时测试目录，独立 native 阶段会移除这些资源。

## 结果含义

通过测试证明对应场景可以运行，不代表全部软件清单、远程服务、解压类型、架构和部署环境都已验证。

认证代理、FTP、FossHub/私有 GitHub release、MSI/Inno/WiX、不常见辅助函数和 autoupdate 模板仍需更多夹具或真实部署测试。参见[兼容状态](compatibility.zh-CN.md)。

发布记录使用当前原生实现结果，不使用旧包装实现结果。机器可读的记录由中英文文档共用。

[开发](development.zh-CN.md) · [架构](architecture.zh-CN.md)
