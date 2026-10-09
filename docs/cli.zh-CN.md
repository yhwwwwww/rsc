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

## 搜索结果

`search [query]` 遵循 Scoop 默认的名称和二进制搜索。关闭 SQLite 时，使用忽略大小写的正则表达式，搜索名称及顶层 `bin`，并在 `Binaries` 列展示匹配到的可执行文件名或别名。名称匹配时该列留空，默认情况下架构下的 `bin` 和描述不会额外增加结果。默认二进制匹配前也遵循 Scoop 的原始文本预筛选规则。启用 SQLite 后，沿用 Scoop 对名称、二进制及快捷方式的 LIKE 搜索行为。

`Installed` 将同一 bucket 的已安装版本与范围整合为 `2.48.1 (user)` 或 `2.48.1 (global)`，直接给版本号着色：蓝色表示版本一致，黄色表示版本落后或无法判断，紫色表示已安装版本更高，红色表示安装损坏。不再单列状态。每个范围的版本独立着色，hold 和缺少来源信息的提示保留在版本旁。重定向和 `NO_COLOR` 模式用简短文字标注非当前状态。其他 bucket 中安装的同名软件不会错误标记在当前结果上；无固定版本的 nightly 清单保留为无法判断。

搜索选项都有明确作用：

| 选项 | 行为 | 示例 |
| --- | --- | --- |
| `-e` / `--explicit` | 忽略大小写的字面量子串匹配，正则字符不再具有特殊意义 | `rsc search -e 'c++'` |
| `-N` / `--name-only` | 只搜软件名称，跳过读取名称不匹配的清单，不包含二进制/别名匹配 | `rsc search -N git` |
| `-D` / `--with-description` | 同时搜索解码后的描述文本，并展示 `Description` 列 | `rsc search -D editor` |

`-e` 可与任一范围选项组合，`-N` 与 `-D` 互斥。使用这些选项时只搜本地 bucket；默认模式在本地无结果时仍沿用 Scoop 的已知 bucket 回退。SQLite 保留 LIKE 匹配，`-e` 同时转义 LIKE 通配符。搜索拒绝 `-g` 和 `--arch`：安装标记同时覆盖用户及全局范围，Scoop 默认的二进制搜索使用顶层条目。`rsc search --help` 中有完整示例。

版本与本地 bucket 清单比较，搜索不会联网拉取更新。保留 bucket 优先级及重复名称结果，没有新增持久搜索缓存。

## 选项

选项放在其作用的命令之后。[命令选项核对](cli-options.zh-CN.md)逐项列出所有命令及保留选项的实际作用。

- `-g` / `--global` 只用于 install、uninstall、update、cleanup、hold、unhold、list、prefix、which 和 shim。全局写操作需要管理员权限。
- `-a` / `--arch 64bit|32bit|arm64` 只用于 install、download 和 depends。update 保留已有安装的架构。
- install/update：`-i` / `--independent`、`-k` / `--no-cache`、`-s` / `--skip-hash-check`。
- download：`-f` / `--force`、`-s` / `--skip-hash-check`。
- update：`-f` / `--force`、`-a` / `--all`。软件操作选项必须同时提供软件名称或 `--all`。
- reset/cleanup：`-a` / `--all`。uninstall：`-p` / `--purge`。cleanup：`-k` / `--cache`。
- `alias list -v` / `--verbose` 展示描述，其他 alias 操作没有选项。
- `cache rm -a` / `--all` 删除全部下载缓存，`cache show` 没有选项。
- VirusTotal：`-a` / `--all`、`-s` / `--scan`、`-n` / `--no-depends`。
- 移除原来忽略的 `--no-update-scoop`、update 的 `--quiet`、VirusTotal 的 `--passthru`。未知选项在执行操作前报错；自定义别名和 shim 目标的转发参数由用户定义。
- 明确的软件名称不能与 `--all` 同时使用。

运行 `rsc help command` 或 `rsc command --help` 可查看对应语法及选项说明。

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

表格遵循终端宽度，并考虑 Unicode 显示宽度。横向表头、纵向详情字段及帮助标题专用淡蓝色，第一列主体使用粗体亮紫红色，bucket 内容使用绿色。search、list、info、depends 及操作汇总中的已安装版本和范围统一显示为 `2.48.1 (user)`；status 保留独立的版本和范围列用于比较。本地已安装版本使用普通蓝色，与标题的淡蓝色区分；search 的非当前状态仍使用警示颜色。只给版本号着色，范围后缀弱化。

下载进度使用铺满可用宽度的块状条，配有旋转符、百分比、速度及准确剩余时间，并适配窄终端。未知大小时显示已用时间，不虚构百分比或剩余时间。创建清单时的下载也使用同一显示，完成后给出简短的已保存/缓存汇总。

`cat` 内置 JSON 键、字符串、数字、字面量及标点高亮，设置 `cat_style` 后仍可使用可选的 bat 展示器。重定向及 `NO_COLOR` 关闭颜色，重定向时进度使用纯文本事件。诊断与进度写入 stderr，路径、清单和 Scoopfile 可从 stdout 重定向。

`status` 整份状态报告使用 stdout：正常的 bucket 更新提示汇总在软件表格前，检查警告集中在表格后的 `Checks needing attention` 区域。致命错误仍写入 stderr，避免终端合并输出通道时把警告插入表格。

失败返回非零。VirusTotal 的 2 表示不安全报告，4 表示请求失败，8 表示无法解析清单，16 表示未配置 API key，报告错误可组合这些位。
