# nov-nas-back

[![License: MIT](https://img.shields.io/github/license/trendoverflow/nov-nas-back)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024%20edition-orange)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6)](https://github.com/trendoverflow/nov-nas-back)
[![GitHub last commit](https://img.shields.io/github/last-commit/trendoverflow/nov-nas-back)](https://github.com/trendoverflow/nov-nas-back)

A lightweight, high-performance offline backup tool that incrementally backs up
Synology NAS shared folders to local disk on Windows.

nov-nas-back reads NAS shared folders over native SMB/UNC, authenticating with
credentials already stored in Windows Credential Manager — no extra password
management. It performs append-only incremental backups (skipping unchanged files
by size + mtime), throttles global bandwidth with a token bucket, and runs
non-interactively with explicit exit codes, ready for Windows Task Scheduler.

> 轻量、高性能的群晖 NAS 共享文件夹离线备份工具，在 Windows 上将 NAS 数据增量备份到本地磁盘。
> 通过原生 SMB/UNC 访问并复用 Windows 凭据管理器已存凭据，无需额外管理密码；
> 执行「追加式」增量备份（按大小 + 修改时间跳过未变文件），令牌桶全局限速，
> 无交互运行、明确退出码，可直接接入 Windows 任务计划程序。

## Features

- **Native SMB / UNC access** — reads `\\nas\share` directly and reuses credentials already stored in Windows Credential Manager. No separate password management.
- **Incremental append-only backup** — skips unchanged files (compared by size + mtime) and copies only new or modified files. Extra files on the destination are never deleted.
- **Global bandwidth throttle** — a token-bucket rate limiter keeps backups from saturating the network.
- **Bounded concurrency & streaming copy** — a fixed worker pool with 1 MiB chunked streaming I/O keeps memory usage bounded and resource-friendly.
- **Non-interactive CLI** — explicit exit codes, ready to be driven by Windows Task Scheduler.

### 特性（中文）

- **原生 SMB / UNC 访问** — 直接读取 `\\nas\share`，复用 Windows 凭据管理器已存凭据，无需单独管理密码。
- **增量追加备份** — 按「大小 + 修改时间」跳过未变文件，仅复制新增或修改的文件，永不删除目标端多余文件。
- **全局限速** — 令牌桶限速，避免备份占满网络带宽。
- **有界并发 + 流式拷贝** — 固定 worker 池与 1 MiB 分块流式 I/O，内存占用有上界、省系统资源。
- **无交互 CLI** — 明确退出码，可直接接入 Windows 任务计划程序。

## How it works

```
┌──────────────────────┐         UNC (SMB)          ┌─────────────────────┐
│  Synology NAS        │ ────────────────────────▶ │  Windows (this tool) │
│  \\nas\share\...     │   size + mtime compare    │  ──▶ local disk      │
└──────────────────────┘   skip unchanged files    └─────────────────────┘
```

For each job (`source` → `dest`), the tool walks the source tree, compares every
file against its destination counterpart using **size + modification time** (with a
2-second tolerance to absorb cross-filesystem timestamp granularity), and copies
only files that are new or changed. The copy streams through a global token-bucket
throttle in 1 MiB chunks, writes to a temporary file, restores the original mtime,
then atomically renames it into place. Files are never deleted on the destination.

每个任务（`source` → `dest`）会遍历源目录，用「大小 + 修改时间」（2 秒容差，兼容跨文件系统时间戳精度）与目标端逐一比对，仅复制新增或变化的文件。拷贝经全局限速以 1 MiB 分块流式进行，先写临时文件、回写原始 mtime、再原子重命名，且永不删除目标端文件。

## Quick Start

1. Install Rust via [rustup](https://rustup.rs), then build:

   ```bash
   cargo build --release
   ```

2. Locate your NAS shared folder's UNC path, e.g. `\\192.168.1.10\photo`.

3. **Store the NAS credentials once**: open File Explorer, enter the UNC path,
   sign in, and tick **"Remember my credentials"**. The credentials are saved to
   Windows Credential Manager and reused automatically.

4. Create `config.toml` from [`config.example.toml`](config.example.toml).

5. Verify reachability:

   ```bash
   nov-nas-back check --config config.toml
   ```

6. Run the backup:

   ```bash
   nov-nas-back run --config config.toml
   ```

### 快速开始（中文）

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

## Installation

```bash
cargo build --release
```

On Windows, build natively. To cross-compile from macOS/Linux:

```bash
cargo build --release --target x86_64-pc-windows-msvc
```

在 Windows 上直接编译即可。如需从 macOS/Linux 交叉编译：

```bash
cargo build --release --target x86_64-pc-windows-msvc
```

## Usage

```bash
# Show top-level help
nov-nas-back --help

# Show help for a subcommand
nov-nas-back run --help
nov-nas-back check --help
```

### Commands

| Command | Description |
|---------|-------------|
| `run`   | Execute backups for all jobs, or a single job via `--job <name>`. |
| `check` | Validate the config and source/destination reachability (for troubleshooting). |

### `run` options

| Option       | Default        | Description |
|--------------|----------------|-------------|
| `--config`   | `config.toml`  | Path to the TOML config file. |
| `--job`      | *(all jobs)*   | Only run the job with this name. |

Examples:

```bash
# Back up all jobs
nov-nas-back run --config config.toml

# Back up a single job by name
nov-nas-back run --config config.toml --job photo
```

Exit code is `0` on success and non-zero on failure.

### 命令（中文）

| 命令 | 说明 |
|------|------|
| `run` | 执行全部任务，或用 `--job <name>` 仅执行单个任务。 |
| `check` | 校验配置与源/目标路径可达性（用于排障）。 |

### `run` 参数（中文）

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--config` | `config.toml` | TOML 配置文件路径。 |
| `--job` | （全部任务） | 仅执行指定名称的任务。 |

示例：

```bash
# 备份全部任务
nov-nas-back run --config config.toml

# 仅备份指定任务
nov-nas-back run --config config.toml --job photo
```

退出码：`0` 成功，非 `0` 失败。

## Configuration

See [`config.example.toml`](config.example.toml). Windows paths use single quotes
to avoid TOML backslash escaping.

### Reference

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `global.concurrency` | integer | `4` | Number of concurrent file copies. |
| `global.rate_limit_mbps` | number | `0` | Global bandwidth throttle in MB/s; `0` means unlimited. |
| `global.log_dir` | string | `"logs"` | Directory for rolling log files. |
| `jobs[].name` | string | *(required)* | Job name; used by `--job` and in logs. |
| `jobs[].source` | string | *(required)* | UNC path of the NAS shared folder. |
| `jobs[].dest` | string | *(required)* | Local destination directory. |

Example:

```toml
[global]
concurrency = 4          # concurrent file copies
rate_limit_mbps = 20     # global throttle in MB/s; 0 = unlimited
log_dir = "logs"

[[jobs]]
name = "photo"
source = '\\192.168.1.10\photo\2025'   # NAS shared folder (UNC)
dest   = 'D:\backup\photo'             # local destination
```

### 配置参考（中文）

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `global.concurrency` | 整数 | `4` | 并发复制的文件数。 |
| `global.rate_limit_mbps` | 数值 | `0` | 全局限速，单位 MB/s；`0` 表示不限速。 |
| `global.log_dir` | 字符串 | `"logs"` | 滚动日志输出目录。 |
| `jobs[].name` | 字符串 | （必填） | 任务名称，供 `--job` 与日志使用。 |
| `jobs[].source` | 字符串 | （必填） | NAS 共享文件夹的 UNC 路径。 |
| `jobs[].dest` | 字符串 | （必填） | 本地目标目录。 |

示例：

```toml
[global]
concurrency = 4          # 并发复制的文件数
rate_limit_mbps = 20     # 全局限速，单位 MB/s；0 表示不限速
log_dir = "logs"

[[jobs]]
name = "photo"
source = '\\192.168.1.10\photo\2025'   # NAS 共享文件夹（UNC）
dest   = 'D:\backup\photo'             # 本地目标目录
```

> Windows paths must be wrapped in single quotes in TOML to avoid backslash escaping.
> Windows 路径在 TOML 中需用单引号包裹，以避免反斜杠转义。

## Logging

Logs are written to both the console (stderr) and a daily rolling file
(`<log_dir>/nov-nas-back.2026-11-17.log` by default, where `<log_dir>` is
`global.log_dir`). Set the `RUST_LOG` environment variable to control verbosity:

```bash
RUST_LOG=debug nov-nas-back run --config config.toml
```

日志同时输出到控制台（stderr）和按天滚动的文件（默认为 `<log_dir>/nov-nas-back.2026-11-17.log`，`<log_dir>` 即 `global.log_dir`）。可通过 `RUST_LOG` 环境变量控制日志级别：

```bash
RUST_LOG=debug nov-nas-back run --config config.toml
```

## Windows Task Scheduler

1. Open Task Scheduler (`Win + R` → `taskschd.msc`).
2. Create a task whose action is **Start a program**, pointing to `nov-nas-back.exe`
   with arguments `run --config "C:\path\to\config.toml"`.
3. Set **Start in** to the directory containing `nov-nas-back.exe`.
4. Configure a trigger (e.g. daily) and enable **Run whether user is logged on or not**
   for unattended execution.

> When using "Run whether user is logged on or not", make sure the NAS share
> credentials are saved under **Control Panel → Credential Manager → Windows
> Credentials** so the SMB authentication works non-interactively.

### 任务计划程序（中文）

1. `Win + R` → `taskschd.msc` 打开任务计划程序。
2. 新建任务，操作选择「启动程序」，程序填 `nov-nas-back.exe` 完整路径，参数填 `run --config "C:\path\to\config.toml"`。
3. 「起始于」填写 `nov-nas-back.exe` 所在目录。
4. 按需设置触发器（如每日），并勾选「不管用户是否登录都要运行」以支持无人值守执行。

> 使用「不管用户是否登录都要运行」时，请确保 NAS 共享凭据已保存到
> 「控制面板 → 凭据管理器 → Windows 凭据」，以便非交互方式完成 SMB 认证。

## Troubleshooting

| Symptom | Likely cause & fix |
|---------|--------------------|
| `check` reports the source is unreachable | NAS is offline, the UNC path is wrong, or the network is unreachable. Verify `ping` and the path, then re-run `check`. |
| `Access denied` when reading the share | The NAS credentials are missing or expired in Windows Credential Manager. Re-open `\\nas\share` in File Explorer and tick "Remember my credentials". |
| `未找到名为 ... 的备份任务` / job not found | The `--job` value does not match any `jobs[].name` in the config. Check spelling. |
| `.novtmp-*` files appear in the destination | Leftover temporary files from an interrupted run; they are overwritten on the next run and can be deleted manually. |
| Want to confirm throttling works | Run with `RUST_LOG=debug` and observe the transfer rate, or monitor network throughput. |

### 排障（中文）

| 现象 | 可能原因与处理 |
|------|----------------|
| `check` 报「源不可达」 | NAS 关机、UNC 路径错误或网络不通。确认能 `ping` 通、路径正确后重新 `check`。 |
| 读取共享时提示「拒绝访问」 | 凭据管理器中的 NAS 凭据缺失或过期。在资源管理器重新打开 `\\nas\share` 并勾选「记住我的凭据」。 |
| 报「未找到名为 ... 的备份任务」 | `--job` 的值与配置中 `jobs[].name` 不匹配，检查拼写。 |
| 目标目录出现 `.novtmp-*` 文件 | 中断运行遗留的临时文件，下次运行会覆盖，也可手动删除。 |
| 想确认限速是否生效 | 用 `RUST_LOG=debug` 运行观察传输速率，或监控网络吞吐。 |

## Project layout

```
src/
  main.rs          # CLI entry point: parse args, load config, init logging
  cli.rs           # clap command definitions (run / check)
  config.rs        # TOML config loading and validation
  error.rs         # domain error types (thiserror)
  throttle.rs      # global token-bucket rate limiter
  backup/
    engine.rs      # bounded-concurrency orchestration + summary stats
    differ.rs      # size + mtime change detection
    copier.rs      # chunked streaming copy with atomic rename
```

## License

Licensed under the [MIT License](LICENSE).
