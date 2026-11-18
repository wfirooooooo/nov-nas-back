//! 快照目录管理：定位本次/上一快照，以及保留策略裁剪。
//!
//! 快照根目录位于 `dest/.snapshots/` 内部，保证与备份数据同卷，
//! 从而使「未变文件 → 硬链接上一快照」的去重得以成立（跨卷硬链接会失败，
//! 届时由 [`crate::backup::copier`] 降级为流式拷贝）。

use std::path::{Path, PathBuf};

use crate::error::BackupError;

/// 快照根目录名（位于 dest 内部）。
pub const SNAPSHOT_DIR: &str = ".snapshots";

/// 准备本次备份的快照目录，返回（本次快照目录，上一快照目录）。
///
/// 快照名使用本地时间戳 `%Y%m%d-%H%M%S`，字典序即时间序；
/// 同一秒内重复运行时追加 `-N` 后缀避免冲突。
pub fn prepare(dest: &Path) -> Result<(PathBuf, Option<PathBuf>), BackupError> {
    let root = dest.join(SNAPSHOT_DIR);
    std::fs::create_dir_all(&root)?;
    let prev = latest_snapshot(&root)?;
    let snap = root.join(unique_name(&root)?);
    std::fs::create_dir_all(&snap)?;
    Ok((snap, prev))
}

/// 返回 `dest` 下最新的快照目录（不存在则 `None`）。
pub fn latest(dest: &Path) -> Result<Option<PathBuf>, BackupError> {
    let root = dest.join(SNAPSHOT_DIR);
    latest_snapshot(&root)
}

/// 返回 `dest` 下全部快照目录，按时间升序排列。
pub fn all(dest: &Path) -> Result<Vec<PathBuf>, BackupError> {
    let root = dest.join(SNAPSHOT_DIR);
    let mut dirs = snapshot_dirs(&root)?;
    dirs.sort();
    Ok(dirs)
}

/// 裁剪快照，保留最近 `keep` 个。`keep == 0` 表示不裁剪。
///
/// 被淘汰快照目录中，硬链接指向的内容若仍被其他快照引用则不受影响，
/// [`std::fs::remove_dir_all`] 仅删除目录项（链接），不会误删内容数据。
pub fn prune(dest: &Path, keep: usize) -> Result<(), BackupError> {
    if keep == 0 {
        return Ok(());
    }
    let root = dest.join(SNAPSHOT_DIR);
    let mut dirs = snapshot_dirs(&root)?;
    dirs.sort();
    let excess = dirs.len().saturating_sub(keep);
    for dir in dirs.into_iter().take(excess) {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// 列出快照根目录下的全部子目录。
fn snapshot_dirs(root: &Path) -> Result<Vec<PathBuf>, BackupError> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    Ok(dirs)
}

/// 返回字典序最大的快照目录（即最近一次备份）。
fn latest_snapshot(root: &Path) -> Result<Option<PathBuf>, BackupError> {
    let mut dirs = snapshot_dirs(root)?;
    dirs.sort();
    Ok(dirs.pop())
}

/// 快照基本信息（供 `list` 展示）。
#[derive(Debug)]
pub struct SnapshotInfo {
    /// 快照目录名（时间戳）。
    pub name: String,
    /// 快照目录完整路径。
    pub dir: PathBuf,
    /// 清单中记录的文件数（清单缺失时为 0）。
    pub files: usize,
}

/// 列出 `dest` 下全部快照及其文件数，按时间升序排列。
pub fn list(dest: &Path) -> Result<Vec<SnapshotInfo>, BackupError> {
    let dirs = all(dest)?;
    let mut infos = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let files = crate::backup::manifest::Manifest::read(&dir)?
            .map(|m| m.files.len())
            .unwrap_or(0);
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        infos.push(SnapshotInfo { name, dir, files });
    }
    Ok(infos)
}

/// 按目录名（时间戳）定位快照；不存在返回 `None`。
pub fn find(dest: &Path, name: &str) -> Option<PathBuf> {
    let dir = dest.join(SNAPSHOT_DIR).join(name);
    dir.is_dir().then_some(dir)
}

/// 生成不与现有目录冲突的时间戳名称。
fn unique_name(root: &Path) -> Result<String, BackupError> {
    let base = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut name = base.clone();
    let mut seq = 0u32;
    while root.join(&name).exists() {
        seq += 1;
        name = format!("{base}-{seq}");
    }
    Ok(name)
}
