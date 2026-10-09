# 命令选项核对

[English — 主版本](cli-options.md) · [命令](cli.zh-CN.md) · [README](../README.zh-CN.md)

每个保留选项都对应操作、筛选、所选字段或可见输出变化。内置命令均可使用 `-h` / `--help`，顶层 `-V` / `--version` 显示程序版本。

| 命令 | 保留选项 | 实际作用 |
| --- | --- | --- |
| `install` | `-g`、`-a/--arch`、`-i`、`-k`、`-s` | 安装范围、清单架构、依赖计划、临时下载缓存、哈希校验 |
| `uninstall` | `-g`、`-p` | 安装范围、删除持久数据 |
| `update` | `-g`、`-a/--all`、`-f`、`-i`、`-k`、`-s` | 软件选择、重装、依赖、缓存及哈希；不指定软件时只同步 bucket |
| `status` | `-l/--local` | 跳过远程 bucket 拉取，仍检查软件状态 |
| `reset` | `-a/--all` | 重置全部用户/全局安装，范围由已有安装信息决定 |
| `cleanup` | `-g`、`-a/--all`、`-k/--cache` | 范围及软件选择，删除旧版本对应缓存 |
| `hold` | `-g` | 设置指定安装的 hold |
| `unhold` | `-g` | 移除指定安装的 hold |
| `search` | `-e`、`-N`、`-D` | 字面量匹配、只搜名称、搜索并展示描述 |
| `list` | `-g` | 只列全局安装，默认包含两个范围 |
| `info` | `-v` | 包含架构、二进制、依赖和备注 |
| `cat` | 无 | 展示指定清单 |
| `prefix` | `-g` | 选择全局安装目录 |
| `which` | `-g` | 只搜索全局 shim，默认还搜索 PATH 及 Scoop shim |
| `config` | 无 | 通过位置参数查询、设置及删除配置 |
| `bucket add/rm/list/known` | 无 | 通过位置参数操作 bucket |
| `depends` | `-a/--arch` | 按所选架构解析依赖 |
| `download` | `-a/--arch`、`-f`、`-s` | 选择文件、跳过缓存、跳过哈希校验 |
| `cache` / `cache show` | 无 | 查看全部或指定软件缓存 |
| `cache rm` | `-a/--all` | 删除全部缓存，而非指定软件缓存 |
| `export` | `-c/--config` | 在 Scoopfile 中包含 Scoop 配置 |
| `import` | 无 | 范围与架构从 Scoopfile 读取 |
| `home` | 无 | 打开指定清单的软件主页 |
| `alias add/rm` | 无 | 通过位置参数操作别名 |
| `alias list` | `-v` | 在表格中展示描述 |
| `shim add/rm/list/info/alter` | `-g` | 选择全局 shim；目标参数放在 `--` 后 |
| `checkup` | 无 | 检查两个安装目录、工具、安装信息及用户 PATH |
| `create` | 无 | URL/输出位置参数及交互式清单字段 |
| `virustotal` | `-a`、`-s`、`-n` | 全部安装、提交不存在的 URL 报告、不检查依赖 |
| `help` | 无 | 展示内置命令或操作的帮助 |

## 移除的无用选项

- 除 install/download/depends 外，移除其他命令继承的 `--arch`；这三个命令现在使用统一的 `-a/--arch` 参数。
- 移除不选择安装范围的命令中的 `-g/--global`。search/info/status 查看两个范围，reset 从安装信息决定范围，import 从 Scoopfile 读取，export 导出两个范围，bucket/cache/config 不选择安装范围。
- 移除 install/download 的 `-u/--no-update-scoop`，因为 rsc 没有 Scoop 自身更新步骤。
- 移除 update 的 `-q/--quiet`，原实现没有据此控制输出。
- 移除 VirusTotal 的 `-u/--no-update-scoop` 及 `-p/--passthru`，原来都不会改变 CLI 行为。
- alias/checkup/create/shim/virustotal 改为按具体操作解析，拒绝未知管理器选项及多余位置参数。自定义别名及 shim 目标参数属于明确转发。
- update 的软件操作选项必须指定软件；`--all` 不能悄悄覆盖明确的软件名称。
- `alias list -v` 现在通过展示描述改变输出，不再被忽略。
- 移除 `cache --all` 和 `cache show --all`，保留有作用的 `cache rm --all`。

版本与范围格式及配色说明见[命令](cli.zh-CN.md)。默认搜索仍与 Scoop 对齐。
