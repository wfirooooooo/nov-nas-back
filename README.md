# nov-nas-back

[![License: MIT](https://img.shields.io/github/license/wfirooooooo/nov-nas-back)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024%20edition-orange)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6)](https://github.com/wfirooooooo/nov-nas-back)
[![GitHub last commit](https://img.shields.io/github/last-commit/wfirooooooo/nov-nas-back)](https://github.com/wfirooooooo/nov-nas-back)

[简体中文](README.zh-CN.md)

A lightweight, high-performance offline backup tool that incrementally backs up
Synology NAS shared folders to local disk on Windows.

nov-nas-back reads NAS shared folders over native SMB/UNC, authenticating with
credentials already stored in Windows Credential Manager — no extra password
management. It performs append-only incremental backups (skipping unchanged files
by size + mtime), throttles global bandwidth with a token bucket, and runs
non-interactively with explicit exit codes, ready for Windows Task Scheduler.

## Features

- **Native SMB / UNC access** — reads `\\nas\share` directly and reuses credentials already stored in Windows Credential Manager. No separate password management.
- **Incremental append-only backup** — skips unchanged files (compared by size + mtime) and copies only new or modified files. Extra files on the destination are never deleted.
- **Global bandwidth throttle** — a token-bucket rate limiter keeps backups from saturating the network.
- **Bounded concurrency & streaming copy** — a fixed worker pool with 1 MiB chunked streaming I/O keeps memory usage bounded and resource-friendly.
- **Non-interactive CLI** — explicit exit codes, ready to be driven by Windows Task Scheduler.

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

## Installation

```bash
cargo build --release
```

On Windows, build natively. To cross-compile from macOS/Linux:

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

> Windows paths must be wrapped in single quotes in TOML to avoid backslash escaping.

## Logging

Logs are written to both the console (stderr) and a daily rolling file
(`<log_dir>/nov-nas-back.2026-11-17.log` by default, where `<log_dir>` is
`global.log_dir`). Set the `RUST_LOG` environment variable to control verbosity:

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

## Troubleshooting

| Symptom | Likely cause & fix |
|---------|--------------------|
| `check` reports the source is unreachable | NAS is offline, the UNC path is wrong, or the network is unreachable. Verify `ping` and the path, then re-run `check`. |
| `Access denied` when reading the share | The NAS credentials are missing or expired in Windows Credential Manager. Re-open `\\nas\share` in File Explorer and tick "Remember my credentials". |
| `job not found` error | The `--job` value does not match any `jobs[].name` in the config. Check spelling. |
| `.novtmp-*` files appear in the destination | Leftover temporary files from an interrupted run; they are overwritten on the next run and can be deleted manually. |
| Want to confirm throttling works | Run with `RUST_LOG=debug` and observe the transfer rate, or monitor network throughput. |

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
