# 命令与配置

[English — 主版本](cli.md) · [README](../README.zh-CN.md)

命令沿用 Scoop 名称，以直接的单个命令为主。

| 命令 | 行为 |
| --- | --- |
| `install app...` | 安装及解析依赖，支持本地/URL 清单和固定版本 |
| `uninstall app...` | 卸载；`--purge` 同时删除持久数据 |
| `update` | 同步 bucket |
| `update app...` / `update *` | 更新软件，尊重 hold，可强制重装 |
| `status [--local]` | 可用版本、未完成安装和缺失依赖 |
| `reset app[@version]` / `reset *` | 激活已有版本，重建系统集成 |
| `cleanup app...` / `cleanup *` | 清理旧版本；`--cache` 同时删除旧缓存 |
| `hold app...` / `unhold app...` | 禁止/允许更新 |
| `bucket add name [repo]` | 添加已知或指定 Git bucket |
| `bucket rm name` / `bucket list` / `bucket known` | 删除、列出和查询已知 bucket |
| `depends app` | 依赖及解压工具安装顺序 |
| `search [query]` / `list [query]` | 搜索清单或已安装软件 |
| `info app [-v]` / `cat app` | 软件详情或清单 JSON |
| `prefix app` / `which command` | 活动安装目录或实际命令目标 |
| `download app...` | 下载并校验，不安装 |
| `cache show [app...]` / `cache rm app...` | 查看/删除匹配缓存 |
| `config [name [value]]` / `config rm name` | 查询、设置或删除配置 |
| `export [--config]` / `import file-or-url` | 导出/导入 Scoopfile |
| `home app` | 打开软件主页 |
| `alias add/rm/list` | 管理用户脚本别名 |
| `shim add/rm/list/info/alter` | 管理原生启动器和候选目标 |
| `checkup` | 检查目录、工具、环境和未完成安装 |
| `create url [output]` | 下载、校验并交互式创建清单 |
| `virustotal app...` | 查询 VirusTotal，需要 `virustotal_api_key` |
| `help [command]` | 查看帮助 |

## 选项

- `-g` / `--global`：全局范围；写操作需要管理员权限。
- `--arch 64bit|32bit|arm64`：架构；install/download/depends 也支持 `-a`。
- install/update：`-i` / `--independent`、`-k` / `--no-cache`、`-s` / `--skip-hash-check`。
- download：`-f` / `--force`、`-s` / `--skip-hash-check`。
- update：`-f` / `--force`、`-a` / `--all`；reset/cleanup 也支持 `--all`。
- uninstall：`-p` / `--purge`；cleanup：`-k` / `--cache`。
- 接受 `--no-update-scoop` 以兼容命令；管理器自身升级不在范围内。
- VirusTotal 接受 `--all`、`--scan`、`--no-depends`、`--no-update-scoop` 和 `--passthru`。

完整参数语法可运行 `rsc help command`。

## 配置

Scoop 共用配置保存在发现的 Scoop `config.json`，下载调优保存在配置主目录下的 `rsc/config.json`。

| 下载设置 | 默认 |
| --- | --- |
| `download.threads` | 4 |
| `download.concurrent` | 4 |
| `download.per_host` | 8 |
| `download.split_size` | 4,194,304 字节 |
| `download.retries` | 3 |
| `download.timeout` | 30 秒 |

`show_manifest` 启用安装前确认，`cat_style` 使用 bat 展示清单。`use_sqlite_cache` 启用兼容 SQLite 索引，`use_isolated_path` 将软件路径迁移到指定环境变量。

普通查询用忽略大小写的正则，SQLite 搜索使用 LIKE。正则及 autoupdate 限制见[兼容状态](compatibility.zh-CN.md)。

## 输出与退出码

表格遵循终端宽度，考虑 Unicode 显示宽度，使用语义颜色：名称为青色，版本为紫色，正常状态为绿色，待处理状态为黄色，错误为红色，路径与来源等次要信息弱化。多个状态分别着色。`cat` 内置 JSON 键、字符串、数字、字面量及标点高亮，设置 `cat_style` 后仍可使用可选的 bat 展示器。重定向和 `NO_COLOR` 关闭动态及彩色格式。诊断与进度输出到 stderr，路径、清单和 Scoopfile 可从 stdout 重定向。

`status` 整份状态报告使用 stdout：正常的 bucket 更新提示汇总在软件表格前，检查警告集中在表格后的 `Checks needing attention` 区域。致命错误仍写入 stderr，避免终端合并输出通道时把警告插入表格。

失败返回非零。VirusTotal 的 2 表示不安全报告，4 表示请求失败，8 表示无法解析清单，16 表示未配置 API key，报告错误可组合这些位。
