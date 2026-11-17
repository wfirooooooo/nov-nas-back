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
# Back up all jobs
nov-nas-back run --config config.toml

# Back up a single job by name
nov-nas-back run --config config.toml --job photo

# Validate config and source/destination reachability
nov-nas-back check --config config.toml
```

Exit code is `0` on success and non-zero on failure.

```bash
# 备份全部任务
nov-nas-back run --config config.toml

# 仅备份指定任务
nov-nas-back run --config config.toml --job photo

# 校验配置与源/目标可达性
nov-nas-back check --config config.toml
```

退出码：`0` 成功，非 `0` 失败。

## Configuration

See [`config.example.toml`](config.example.toml). Windows paths use single quotes
to avoid TOML backslash escaping.

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

配置文件见 [`config.example.toml`](config.example.toml)。Windows 路径请用单引号包裹，避免 TOML 反斜杠转义。

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
