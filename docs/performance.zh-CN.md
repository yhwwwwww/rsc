# 查询性能

[English — 主版本](performance.md) · [README](../README.zh-CN.md)

## 当前搜索选项

完成选项核对及输出调整的本机安装版，用 `hyperfine --warmup 5 --runs 20 --shell none` 测量。默认搜索包含名称及二进制，共 80 个结果；rsc 仅名称搜索及 Hok 默认搜索均为 76 个结果。生成物校验值与全部样本保存在 [cli-benchmark.json](cli-benchmark.json)。

| 命令 | 平均耗时 |
| --- | ---: |
| `rsc search git` | 72.5 ms |
| `rsc search -N git` | 26.1 ms |
| `hok search git` | 53.5 ms |

这是同一本机的热文件缓存测量；默认搜索与仅名称搜索覆盖的字段不同。

## 对齐 Scoop 的搜索与本机命令入口

下列搜索基准是记录中校验值所对应发布版的快照，测于后续的已安装版本配色和搜索选项修改之前。它使用 `hyperfine --warmup 5 --runs 20 --shell none`，比较同一本机的已安装命令，共用 5 个 bucket / 5,189 份清单。重定向输出，耗时包含启动。原始样本和程序校验值见 [search-benchmark.json](search-benchmark.json)。

| 已安装命令 | 平均耗时 |
| --- | ---: |
| `rsc search git` 优化前 | 112.6 ms |
| `rsc search git` 优化后 | 81.3 ms |
| `hok search git` | 58.3 ms |
| `hok search -B git` | 108.4 ms |
| `scoop search git` (经由 `scoop.cmd`) | 2446.4 ms |

新版 rsc 比之前快 1.39 倍。Hok 默认只搜名称，仍比 rsc 的名称/二进制搜索快 1.39 倍；rsc 还读取匹配软件的来源、范围及版本状态。Hok 开启二进制搜索后，rsc 耗时为其 0.75 倍。耗时受文件系统缓存、防病毒扫描及进程调度影响。

`search git` 在 Scoop 和 rsc 中均返回 80 条，Hok 默认返回 76 条。多出的 `biodiff`、`gcloud`、`psutils`、`worktrunk` 来自可执行文件名匹配。已使用本机实际 Scoop 对照七组查询，名称、版本、bucket 顺序及匹配的二进制名称一致。

搜索只解析必要字段，跳过无法贡献二进制匹配的清单，仅读取结果涉及的软件安装信息。清单变化即时生效，没有增加持久搜索缓存。支持硬链接的文件系统中，经校验的 rsc 命令入口与管理器共用程序文件，省去子进程启动；固定参数或目标变化时仍正常转发。

复现：

```powershell
.\scripts\benchmark_search.ps1
python tests/search_scoop.py
```

## 较早的查询基准

### 测量方式

Windows 11，GNU x64 release 构建，16 个逻辑处理器。本机同一份 Scoop 安装包含 5 个 bucket、5,189 份清单，关闭 SQLite 搜索。两者读取相同的 bucket 目录；Hok 搜索使用 `-B`，把二进制别名纳入搜索。

每项先预热一次，再执行 7 次取中位数。耗时包含启动进程和捕获 stdout。这是文件系统缓存已预热的测量，不是冷磁盘或终端重绘测试。程序版本、校验值和每次样本见 [query-benchmark.json](query-benchmark.json)。这些样本对应性能优化发布版；后续 status 报告修复已将 bucket 更新改为汇总提示。

| 命令 | 优化前 rsc | 优化后 rsc | Hok |
| --- | ---: | ---: | ---: |
| `search jq` | 575.9 ms | 90.0 ms | 97.2 ms |
| `search ^git` | 631.1 ms | 109.0 ms | 115.4 ms |
| `status --local` | 510.6 ms | 44.0 ms | — |
| `list` | 34.5 ms | 30.0 ms | 36.2 ms |
| `cat jq` | 49.7 ms | 29.6 ms | 41.0 ms |

Hok 0.1.0-beta.7 没有 `status` 命令，因此本地状态使用之前的原生 rsc 版本作为基线。这些数据说明本数据集上的查询速度已与 Hok 接近，不保证其他机器具有相同耗时。

### 实现调整

- 一次状态操作只枚举一次 bucket 文件名，按名称查询每个已安装软件。
- 安装信息只读取一次，重复使用依赖名称集合。
- 每个进程只编译一次版本比较表达式。
- 搜索直接处理解析后的清单，取消完整 JSON 序列化和第二次解析。
- 最多 16 个线程并行读取清单，保留 bucket 优先级、结果顺序和错误清单警告。
- 仅传输操作建立下载任务线程池；查询使用更小的运行时，shim 在创建运行时前直接分发。
- 并行检查远端 bucket 状态，保留检查范围及警告顺序。

没有新增持久搜索缓存，清单的修改和删除会在下一次命令中立即生效。已有 Scoop SQLite 配置仍然有效。

不加 `--local` 的 `status` 仍按 Scoop 行为检查远端 bucket 更新，表格未计入网络耗时。本地没有匹配结果时，搜索仍会查询其他已知 bucket，因此这条路径也受网络影响。


另行测量默认 `status` 的实际联网路径（3 次取中位数）：优化前 5.11 秒，优化后 0.99 秒，bucket 更新警告一致。耗时受网络影响；样本在原始记录的 `remote_status` 中单独保存。

### 结果核对

基准程序核对优化前后的搜索和列表文本，以及清单 JSON。本地状态在归一化新增的明确 `current` 状态后进行比较。Rust 测试覆盖忽略大小写、二进制别名、架构字段、嵌套清单、稳定顺序、错误清单、修改与删除、用户与全局安装、锁定与失败与已移除的软件、缺失依赖及夜间版本。

上表测量的是程序本身。另行测量已安装命令入口，包含 shim 和子进程启动。`search jq`：rsc 102.3 ms，Hok 105.4 ms。`list`：rsc 44.6 ms，Hok 41.5 ms。同样先预热一次，再测量 7 次取中位数；样本保存于 `installed_launchers`。

### 重现

```powershell
python scripts/benchmark_queries.py after hok
python scripts/benchmark_queries.py before after hok --before .test-lab/rsc-before-query.exe --output .test-lab/query-benchmark.json
```

旧程序是可选的本地对照文件，不随仓库分发。先构建 `dist/rsc.exe`。脚本使用当前 Scoop 配置，仅执行本地只读查询。

[测试](testing.zh-CN.md) · [架构](architecture.zh-CN.md)
