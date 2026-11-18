//! 快照恢复：将指定快照的内容复制到目标目录。

use std::fs;
use std::path::Path;

use tracing::{info, warn};
use walkdir::WalkDir;

use crate::backup::manifest::MANIFEST_FILE;
use crate::error::BackupError;

/// 恢复结果汇总。
#[derive(Debug, Default)]
pub struct RestoreSummary {
    /// 成功恢复的文件数。
    pub restored: u64,
    /// 恢复失败的文件数。
    pub failed: u64,
}

/// 将快照目录 `snap_dir` 的内容恢复到 `target`。
///
/// 跳过清单文件 `manifest.json` 与符号链接等非普通文件；目标已存在的文件
/// 会被覆盖（恢复语义）。该函数为同步 I/O，调用方应置于 `spawn_blocking`。
pub fn restore(snap_dir: &Path, target: &Path) -> Result<RestoreSummary, BackupError> {
    let mut summary = RestoreSummary::default();

    for entry in WalkDir::new(snap_dir).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!(
                    path = ?err.path().unwrap_or(snap_dir),
                    error = %err,
                    "恢复时跳过无法访问的目录项"
                );
                continue;
            }
        };
        let path = entry.path();

        if entry.file_type().is_dir() {
            if let Ok(rel) = path.strip_prefix(snap_dir) {
                fs::create_dir_all(target.join(rel))?;
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        // 跳过清单文件。
        if path.file_name().and_then(|n| n.to_str()) == Some(MANIFEST_FILE) {
            continue;
        }

        let Ok(rel) = path.strip_prefix(snap_dir) else {
            continue;
        };
        let dest = target.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::copy(path, &dest) {
            Ok(_) => {
                summary.restored += 1;
                info!(file = %dest.display(), "已恢复");
            }
            Err(err) => {
                summary.failed += 1;
                warn!(file = %dest.display(), error = %err, "恢复失败");
            }
        }
    }

    Ok(summary)
}
