# nov-nas-back

[![License: MIT](https://img.shields.io/github/license/wfirooooooo/nov-nas-back)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024%20edition-orange)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6)](https://github.com/wfirooooooo/nov-nas-back)
[![GitHub last commit](https://img.shields.io/github/last-commit/wfirooooooo/nov-nas-back)](https://github.com/wfirooooooo/nov-nas-back)

[简体中文](README.zh-CN.md)

A lightweight, high-performance offline backup tool that incrementally backs up
Synology NAS shared folders to local disk on Windows — with built-in ransomware
protection.

nov-nas-back reads NAS shared folders over native SMB/UNC, authenticating with
credentials already stored in Windows Credential Manager — no extra password
management. It performs snapshot-based incremental backups (unchanged files are
hard-linked from the previous snapshot, so history costs almost nothing), and
adds multiple layers of ransomware defense: versioned snapshots for rollback, a
change-rate gate that aborts mass tampering, content hashing to catch forged
timestamps, and a per-snapshot integrity manifest. It throttles global bandwidth
with a token bucket and runs non-interactively with explicit exit codes, ready
for Windows Task Scheduler.

## Features

- **Native SMB / UNC access** — reads `\\nas\share` directly and reuses credentials already stored in Windows Credential Manager. No separate password management.
- **Snapshot-based incremental backup** — every run writes a new timestamped snapshot under `dest/.snapshots/`. Unchanged files are hard-linked from the previous snapshot (near-zero extra storage); only new or changed files are actually copied.
- **Ransomware protection** — see [below](#ransomware-protection).
- **Integrity verification & restore** — `verify`, `list`, and `restore` subcommands to audit and recover snapshots.
- **Global bandwidth throttle** — a token-bucket rate limiter keeps backups from saturating the network.
- **Bounded concurrency & streaming copy** — a fixed worker pool with 1 MiB chunked streaming I/O keeps memory usage bounded and resource-friendly.
- **Non-interactive CLI** — explicit exit codes, ready to be driven by Windows Task Scheduler.

## Ransomware protection

Ransomware that encrypts the NAS can silently corrupt backups if the next
incremental run happily overwrites good copies with encrypted data. nov-nas-back
defends at four layers:

1. **Versioned snapshots (P0)** — each backup is written to a fresh
   `dest/.snapshots/<timestamp>/` directory, never overwritten in place. You can
   always roll back to a previous snapshot.
2. **Change-rate gate (P1)** — before copying, the scan phase counts how many
   *existing* files changed. If the ratio exceeds `global.change_rate_threshold`
   (and at least `global.min_changed_files`), the run aborts and raises
   `RansomwareSuspected` instead of propagating the encryption.
3. **Content hashing (P2)** — files that look unchanged by size + mtime can be
   re-checked by sampled or full BLAKE3 hashing (`global.verify`), catching
   ransomware that forges timestamps.
4. **Integrity manifest (P3)** — every snapshot gets a `manifest.json` recording
   each file's size + BLAKE3 hash. `verify` re-hashes the files on disk and
   reports any mismatch, missing, or extra file.

## How it works

```
┌──────────────────────┐         UNC (SMB)          ┌─────────────────────┐
│  Synology NAS        │ ────────────────────────▶ │  Windows (this tool) │
│  \\nas\share\...     │   compare vs prev snapshot│  ──▶ local disk      │
└──────────────────────┘   hard-link / copy        └─────────────────────┘
                                    │
                                    ▼
                      dest/.snapshots/<timestamp>/
                        ├── unchanged → hard link
                        ├── new/changed → copy
                        └── manifest.json (size + hash)
```

For each job (`source` → `dest`), the tool walks the source tree and compares
every file against the **previous snapshot** (size + modification time, with a
2-second tolerance to absorb cross-filesystem timestamp granularity):

- **unchanged** → hard-linked from the previous snapshot (zero extra bytes);
- **new or changed** → copied through the global token-bucket throttle in 1 MiB
  chunks, written to a temporary file, then atomically renamed into place.

After the copy phase, a `manifest.json` is written to the snapshot recording each
file's size and BLAKE3 hash. Old snapshots are pruned to `global.keep_snapshots`
(only after a fully successful run; failed runs keep all snapshots).

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

7. List snapshots and verify integrity:

   ```bash
   nov-nas-back list --config config.toml
   nov-nas-back verify --config config.toml --all
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
nov-nas-back verify --help
```

### Commands

| Command   | Description |
|-----------|-------------|
| `run`     | Execute backups for all jobs, or a single job via `--job <name>`. |
| `check`   | Validate the config and source/destination reachability (for troubleshooting). |
| `verify`  | Verify snapshot integrity against its manifest; `--all` audits every snapshot. |
| `list`    | List all (or a single job's) snapshots with their file counts. |
| `restore` | Restore a specific snapshot to a target directory. |

### `run` options

| Option       | Default        | Description |
|--------------|----------------|-------------|
| `--config`   | `config.toml`  | Path to the TOML config file. |
| `--job`      | *(all jobs)*   | Only run the job with this name. |

### `verify` options

| Option     | Default       | Description |
|------------|---------------|-------------|
| `--config` | `config.toml` | Path to the TOML config file. |
| `--job`    | *(all jobs)*  | Only verify the job with this name. |
| `--all`    | *(false)*     | Verify all snapshots; by default only the latest. |

### `restore` options

| Option       | Default       | Description |
|--------------|---------------|-------------|
| `--config`   | `config.toml` | Path to the TOML config file. |
| `--job`      | *(required)*  | The job whose snapshot to restore. |
| `--snapshot` | *(required)*  | Snapshot directory name (timestamp, e.g. `20261118-090000`). |
| `--target`   | *(required)*  | Directory to restore into. |

Examples:

```bash
# Back up all jobs
nov-nas-back run --config config.toml

# Back up a single job by name
nov-nas-back run --config config.toml --job photo

# List snapshots for a job
nov-nas-back list --config config.toml --job photo

# Verify the latest snapshot
nov-nas-back verify --config config.toml --job photo

# Verify every snapshot
nov-nas-back verify --config config.toml --job photo --all

# Restore a snapshot
nov-nas-back restore --config config.toml \
  --job photo --snapshot 20261118-090000 --target D:\restore\photo
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
| `global.keep_snapshots` | integer | `7` | Number of snapshots to keep; `0` disables pruning. |
| `global.change_rate_threshold` | number | `0.3` | Change-rate gate threshold; `0` disables the gate. |
| `global.min_changed_files` | integer | `100` | Absolute floor for the gate (avoids false positives on small directories). |
| `global.verify` | string | `"sample"` | Content verification level: `size`, `sample`, or `full`. |
| `jobs[].name` | string | *(required)* | Job name; used by `--job` and in logs. |
| `jobs[].source` | string | *(required)* | UNC path of the NAS shared folder. |
| `jobs[].dest` | string | *(required)* | Local destination directory. |

Example:

```toml
[global]
concurrency = 4            # concurrent file copies
rate_limit_mbps = 20       # global throttle in MB/s; 0 = unlimited
log_dir = "logs"
keep_snapshots = 7         # snapshots to keep; 0 = never prune
change_rate_threshold = 0.3  # abort if >30% of existing files changed
min_changed_files = 100    # gate floor (absolute)
verify = "sample"          # size | sample | full

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
| `RansomwareSuspected` error on run | A large fraction of existing files changed — possibly ransomware encryption. Investigate the source, then lower `change_rate_threshold` or set it to `0` to override only after confirming it is safe. |
| `verify` reports mismatches | Files in the snapshot were modified after backup (e.g. tampered/encrypted). Restore from an earlier snapshot if needed. |
| `.novtmp-*` files appear in the destination | Leftover temporary files from an interrupted run; they are overwritten on the next run and can be deleted manually. |
| Want to confirm throttling works | Run with `RUST_LOG=debug` and observe the transfer rate, or monitor network throughput. |

## Project layout

```
src/
  main.rs          # CLI entry point: parse args, load config, init logging, dispatch
  cli.rs           # clap command definitions (run / check / verify / list / restore)
  check.rs         # job preflight (source reachable, dest writable)
  config.rs        # TOML config loading and validation
  error.rs         # domain error types (thiserror)
  throttle.rs      # global token-bucket rate limiter
  backup/
    engine.rs      # bounded-concurrency orchestration, change-rate gate, manifest generation
    differ.rs      # size + mtime change detection + content hashing
    copier.rs      # chunked streaming copy + hard-link fallback
    snapshot.rs    # snapshot directory management (prepare / prune / list / find)
    manifest.rs    # integrity manifest read/write + verify
    restore.rs     # restore a snapshot to a target directory
    verify.rs      # snapshot verification orchestration
tests/             # integration tests (backup / check / verify / list / restore)
```

## License

Licensed under the [MIT License](LICENSE).
