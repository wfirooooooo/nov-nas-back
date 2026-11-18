# nov-nas-back

[![License: MIT](https://img.shields.io/github/license/wfirooooooo/nov-nas-back)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024%20edition-orange)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6)](https://github.com/wfirooooooo/nov-nas-back)
[![GitHub last commit](https://img.shields.io/github/last-commit/wfirooooooo/nov-nas-back)](https://github.com/wfirooooooo/nov-nas-back)

[English](README.md)

轻量、高性能的群晖 NAS 共享文件夹离线备份工具，在 Windows 上将 NAS 数据增量备份到本地磁盘，并内置防勒索能力。

nov-nas-back 通过原生 SMB/UNC 读取 NAS 共享文件夹，复用 Windows 凭据管理器已存凭据完成认证，无需额外管理密码。它执行「快照式」增量备份（未变文件硬链接复用上一快照，历史版本几乎零成本），并叠加多层防勒索防护：版本化快照可回滚、变化率门禁中止大规模篡改、内容哈希捕获伪造时间戳、逐快照完整性清单。它以令牌桶进行全局限速，无交互运行、给出明确退出码，可直接接入 Windows 任务计划程序。

## 特性

- **原生 SMB / UNC 访问** — 直接读取 `\\nas\share`，复用 Windows 凭据管理器已存凭据，无需单独管理密码。
- **快照式增量备份** — 每次备份写入 `dest/.snapshots/` 下的新时间戳快照目录，永不原地覆盖。未变文件硬链接复用上一快照（几乎不占额外空间），仅新增或变化的文件才真正拷贝。
- **防勒索** — 见下文[防勒索防护](#防勒索防护)。
- **完整性校验与恢复** — `verify`、`list`、`restore` 子命令用于审计与恢复快照。
- **全局限速** — 令牌桶限速，避免备份占满网络带宽。
- **有界并发 + 流式拷贝** — 固定 worker 池与 1 MiB 分块流式 I/O，内存占用有上界、省系统资源。
- **无交互 CLI** — 明确退出码，可直接接入 Windows 任务计划程序。

## 防勒索防护

勒索软件加密 NAS 后，若下一次增量备份「好心」地用加密数据覆盖干净副本，备份就一并被毁。nov-nas-back 在四层设防：

1. **版本化快照（P0）** — 每次备份写入全新的 `dest/.snapshots/<时间戳>/` 目录，从不原地覆盖，随时可回滚到任意历史快照。
2. **变化率门禁（P1）** — 拷贝前扫描阶段统计「已存在文件被修改」的比例，若超过 `global.change_rate_threshold`（且达到 `global.min_changed_files`），则中止备份并报 `RansomwareSuspected`，而非传播加密。
3. **内容哈希（P2）** — 对按大小 + mtime 看似未变的文件，可进一步做采样或全量 BLAKE3 哈希复核（`global.verify`），捕获伪造时间戳的勒索软件。
4. **完整性清单（P3）** — 每个快照生成 `manifest.json`，记录每个文件的大小与 BLAKE3 哈希。`verify` 重新哈希磁盘文件并报告不一致、缺失或多余文件。

## 工作原理

```
┌──────────────────────┐         UNC (SMB)          ┌─────────────────────┐
│  Synology NAS        │ ────────────────────────▶ │  Windows (this tool) │
│  \\nas\share\...     │   与上一快照比对          │  ──▶ local disk      │
└──────────────────────┘   硬链接 / 拷贝           └─────────────────────┘
                                    │
                                    ▼
                      dest/.snapshots/<时间戳>/
                        ├── 未变 → 硬链接复用
                        ├── 新增/变化 → 拷贝
                        └── manifest.json（大小 + 哈希）
```

每个任务（`source` → `dest`）遍历源目录，与**上一快照**逐一比对（大小 + 修改时间，2 秒容差兼容跨文件系统时间戳精度）：

- **未变** → 从上一快照硬链接复用（零额外字节）；
- **新增或变化** → 经全局限速以 1 MiB 分块流式拷贝，先写临时文件再原子重命名。

拷贝完成后向快照写入 `manifest.json`，记录每个文件的大小与 BLAKE3 哈希。旧快照按 `global.keep_snapshots` 裁剪（仅在完全成功后裁剪，失败时保留全部快照以保安全）。

## 快速开始

1. 通过 [rustup](https://rustup.rs) 安装 Rust，然后构建：

   ```bash
   cargo build --release
   ```

2. 找到 NAS 共享文件夹的 UNC 路径，例如 `\\192.168.1.10\photo`。

3. **首次保存 NAS 凭据**：打开文件资源管理器，输入 UNC 路径，登录并勾选「记住我的凭据」。凭据会存入 Windows 凭据管理器并被自动复用。

4. 参照 [`config.example.toml`](config.example.toml) 创建 `config.toml`。

5. 校验可达性：

   ```bash
   nov-nas-back check --config config.toml
   ```

6. 执行备份：

   ```bash
   nov-nas-back run --config config.toml
   ```

7. 列出快照并校验完整性：

   ```bash
   nov-nas-back list --config config.toml
   nov-nas-back verify --config config.toml --all
   ```

## 安装

```bash
cargo build --release
```

在 Windows 上直接编译即可。如需从 macOS/Linux 交叉编译：

```bash
cargo build --release --target x86_64-pc-windows-msvc
```

## 使用

```bash
# 查看顶层帮助
nov-nas-back --help

# 查看子命令帮助
nov-nas-back run --help
nov-nas-back verify --help
```

### 命令

| 命令 | 说明 |
|------|------|
| `run` | 执行全部任务，或用 `--job <name>` 仅执行单个任务。 |
| `check` | 校验配置与源/目标路径可达性（用于排障）。 |
| `verify` | 依据清单校验快照完整性；`--all` 审计全部快照。 |
| `list` | 列出全部（或指定 job）的快照及其文件数。 |
| `restore` | 将指定快照恢复到目标目录。 |

### `run` 参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--config` | `config.toml` | TOML 配置文件路径。 |
| `--job` | （全部任务） | 仅执行指定名称的任务。 |

### `verify` 参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--config` | `config.toml` | TOML 配置文件路径。 |
| `--job` | （全部任务） | 仅校验指定名称的任务。 |
| `--all` | （否） | 校验所有快照；默认仅最新。 |

### `restore` 参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--config` | `config.toml` | TOML 配置文件路径。 |
| `--job` | （必填） | 要恢复的任务名称。 |
| `--snapshot` | （必填） | 快照目录名（时间戳，如 `20261118-090000`）。 |
| `--target` | （必填） | 恢复目标目录。 |

示例：

```bash
# 备份全部任务
nov-nas-back run --config config.toml

# 仅备份指定任务
nov-nas-back run --config config.toml --job photo

# 列出某任务的快照
nov-nas-back list --config config.toml --job photo

# 校验最新快照
nov-nas-back verify --config config.toml --job photo

# 校验所有快照
nov-nas-back verify --config config.toml --job photo --all

# 恢复指定快照
nov-nas-back restore --config config.toml \
  --job photo --snapshot 20261118-090000 --target D:\restore\photo
```

退出码：`0` 成功，非 `0` 失败。

## 配置

配置文件见 [`config.example.toml`](config.example.toml)。Windows 路径请用单引号包裹，避免 TOML 反斜杠转义。

### 配置参考

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `global.concurrency` | 整数 | `4` | 并发复制的文件数。 |
| `global.rate_limit_mbps` | 数值 | `0` | 全局限速，单位 MB/s；`0` 表示不限速。 |
| `global.log_dir` | 字符串 | `"logs"` | 滚动日志输出目录。 |
| `global.keep_snapshots` | 整数 | `7` | 保留的快照数量；`0` 表示不自动裁剪。 |
| `global.change_rate_threshold` | 数值 | `0.3` | 变化率门禁阈值；`0` 表示禁用门禁。 |
| `global.min_changed_files` | 整数 | `100` | 门禁绝对值兜底（避免小目录误报）。 |
| `global.verify` | 字符串 | `"sample"` | 内容校验级别：`size`、`sample` 或 `full`。 |
| `jobs[].name` | 字符串 | （必填） | 任务名称，供 `--job` 与日志使用。 |
| `jobs[].source` | 字符串 | （必填） | NAS 共享文件夹的 UNC 路径。 |
| `jobs[].dest` | 字符串 | （必填） | 本地目标目录。 |

示例：

```toml
[global]
concurrency = 4            # 并发复制的文件数
rate_limit_mbps = 20       # 全局限速，单位 MB/s；0 表示不限速
log_dir = "logs"
keep_snapshots = 7         # 保留快照数；0 表示不裁剪
change_rate_threshold = 0.3  # 已有文件被修改比例超过 30% 则中止
min_changed_files = 100    # 门禁绝对值兜底
verify = "sample"          # size | sample | full

[[jobs]]
name = "photo"
source = '\\192.168.1.10\photo\2025'   # NAS 共享文件夹（UNC）
dest   = 'D:\backup\photo'             # 本地目标目录
```

> Windows 路径在 TOML 中需用单引号包裹，以避免反斜杠转义。

## 日志

日志同时输出到控制台（stderr）和按天滚动的文件（默认为 `<log_dir>/nov-nas-back.2026-11-17.log`，`<log_dir>` 即 `global.log_dir`）。可通过 `RUST_LOG` 环境变量控制日志级别：

```bash
RUST_LOG=debug nov-nas-back run --config config.toml
```

## 任务计划程序

1. `Win + R` → `taskschd.msc` 打开任务计划程序。
2. 新建任务，操作选择「启动程序」，程序填 `nov-nas-back.exe` 完整路径，参数填 `run --config "C:\path\to\config.toml"`。
3. 「起始于」填写 `nov-nas-back.exe` 所在目录。
4. 按需设置触发器（如每日），并勾选「不管用户是否登录都要运行」以支持无人值守执行。

> 使用「不管用户是否登录都要运行」时，请确保 NAS 共享凭据已保存到
> 「控制面板 → 凭据管理器 → Windows 凭据」，以便非交互方式完成 SMB 认证。

## 排障

| 现象 | 可能原因与处理 |
|------|----------------|
| `check` 报「源不可达」 | NAS 关机、UNC 路径错误或网络不通。确认能 `ping` 通、路径正确后重新 `check`。 |
| 读取共享时提示「拒绝访问」 | 凭据管理器中的 NAS 凭据缺失或过期。在资源管理器重新打开 `\\nas\share` 并勾选「记住我的凭据」。 |
| 报「未找到名为 ... 的备份任务」 | `--job` 的值与配置中 `jobs[].name` 不匹配，检查拼写。 |
| `run` 报「疑似勒索软件」 | 大量已有文件被修改——可能是勒索加密。排查源数据；确认安全后才可调低 `change_rate_threshold` 或设为 `0` 覆盖。 |
| `verify` 报不一致 | 快照中的文件在备份后被改动（如被篡改/加密）。必要时从更早的快照恢复。 |
| 目标目录出现 `.novtmp-*` 文件 | 中断运行遗留的临时文件，下次运行会覆盖，也可手动删除。 |
| 想确认限速是否生效 | 用 `RUST_LOG=debug` 运行观察传输速率，或监控网络吞吐。 |

## 项目结构

```
src/
  main.rs          # CLI 入口：解析参数、加载配置、初始化日志、分发命令
  cli.rs           # clap 命令定义（run / check / verify / list / restore）
  check.rs         # 任务预检（源可达、目标可写）
  config.rs        # TOML 配置加载与校验
  error.rs         # 领域错误类型（thiserror）
  throttle.rs      # 全局令牌桶限速器
  backup/
    engine.rs      # 有界并发调度、变化率门禁、清单生成
    differ.rs      # 大小 + mtime 变化检测 + 内容哈希
    copier.rs      # 分块流式拷贝 + 硬链接降级
    snapshot.rs    # 快照目录管理（prepare / prune / list / find）
    manifest.rs    # 完整性清单读写 + 校验
    restore.rs     # 将快照恢复到目标目录
    verify.rs      # 快照校验编排
tests/             # 集成测试（backup / check / verify / list / restore）
```

## License

采用 [MIT License](LICENSE) 许可。
