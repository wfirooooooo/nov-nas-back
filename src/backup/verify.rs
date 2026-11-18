//! 快照完整性校验：定位快照并逐块校验，供 CLI 与集成测试复用。

use std::path::{Path, PathBuf};

use crate::backup::{manifest, snapshot};
use crate::error::BackupError;

/// 单个快照的校验结果。
#[derive(Debug)]
pub struct SnapshotVerifyResult {
    pub snap_dir: PathBuf,
    pub report: manifest::VerifyReport,
}

/// 校验 `dest` 下的快照。`all` 为真时遍历全部，否则仅最新。
///
/// 返回按时间升序排列的各快照校验结果；无快照时返回空向量。
pub fn verify_snapshots(dest: &Path, all: bool) -> Result<Vec<SnapshotVerifyResult>, BackupError> {
    let snap_dirs: Vec<PathBuf> = if all {
        snapshot::all(dest)?
    } else {
        snapshot::latest(dest)?.into_iter().collect()
    };

    let mut results = Vec::with_capacity(snap_dirs.len());
    for snap_dir in snap_dirs {
        let report = manifest::verify(&snap_dir)?;
        results.push(SnapshotVerifyResult { snap_dir, report });
    }
    Ok(results)
}
