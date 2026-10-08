# 构建与开发

[English — 主版本](development.md) · [README](../README.zh-CN.md)

## 环境

需要 Windows 和 Rust 工具链。bucket 使用 Git，软件包所需解压工具从软件目录或 PATH 查找。集成测试使用 Python 3。对照测试需要已安装 Scoop，rsc 本身不依赖 Scoop。

工程声明 Rust 1.85 为源码语言最低版本，实际验证的编译器及目标见 [build-info.json](build-info.json)。最低工具链和其他目标需分别验证。

## 构建

```powershell
.\scripts\build.ps1
.\scripts\build.ps1 -Profile debug
```

构建脚本支持普通 Rust 安装，也可使用 `.tools` 下的项目本地 GNU 工具链。准备后者：

```powershell
.\scripts\setup-dev.ps1
```

准备脚本下载 Rust 和编译工具，仅用于开发，不是运行组件。

发布文件为 `dist\rsc.exe`。`rsc_core` 使用 `crate-type = ["rlib"]`。SQLite 内置，Windows CRT 配置为静态链接。发布程序的导入表应只有 Windows 系统 DLL。

## 测试

```powershell
.\scripts\test.ps1
.\scripts\test.ps1 -SkipBuild -Phase native
```

可选阶段：`network`、`lifecycle`、`native`、`real`、`all`。

native 阶段移除参考管理器资源后测试 rsc。对照阶段调用本机单独安装的 Scoop。参考 supporting 文件和隔离软件目录存放在系统临时目录中，位于源码仓库之外。

测试恢复相关用户环境注册表值，并检查本机 Scoop 配置未改变。日志记录隔离目录。本地测试程序和最新结果指针位于被忽略的 `.test-lab`。

## 源码要求

- 用 Rust 独立实现行为。
- Scoop 参考仓库、脚本和复制的管理器资源放在项目外。
- 不引入 PowerShell 后端、内嵌原版源码或运行时展开脚本。
- 清单及用户脚本属于允许的输入；自行编写的上下文及辅助适配器位于 `native/scripts`。
- 新公共文档提供中英两版，英文为主，链接指向同语言文档。
- 通过测试夹具及原版对照验收，不能把编译成功当作行为通过。
- 发布的干净历史排除已弃用的原版脚本实现。

[架构](architecture.zh-CN.md) · [兼容状态](compatibility.zh-CN.md) · [测试](testing.zh-CN.md)
