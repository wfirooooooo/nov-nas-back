//! 备份完整性清单：记录快照中每个文件的大小与 BLAKE3 哈希，
//! 用于事后 `verify` 校验快照是否被篡改或加密。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;
use walkdir::WalkDir;

use crate::backup::differ::full_hash;
use crate::error::BackupError;

/// 清单文件名（位于每个快照目录内）。
pub const MANIFEST_FILE: &str = "manifest.json";

/// 单个文件的清单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub size: u64,
    /// BLAKE3 十六进制字符串。
    pub hash: String,
}

/// 快照完整性清单：相对路径（`/` 分隔）→ 条目。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub files: HashMap<String, ManifestEntry>,
}

impl Manifest {
    /// 将清单写入快照目录（`<snap_dir>/manifest.json`）。
    pub fn write(&self, snap_dir: &Path) -> Result<(), BackupError> {
        let path = snap_dir.join(MANIFEST_FILE);
        let json = serde_json::to_string(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// 从快照目录读取清单；文件不存在时返回 `None`。
    pub fn read(snap_dir: &Path) -> Result<Option<Manifest>, BackupError> {
        let path = snap_dir.join(MANIFEST_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(path)?;
        let manifest = serde_json::from_str(&json)?;
        Ok(Some(manifest))
    }
}

/// `verify` 校验结果汇总。
#[derive(Debug, Default)]
pub struct VerifyReport {
    /// 清单中记录的、磁盘上找到的文件数。
    pub checked: u64,
    /// 内容与清单一致的文件数。
    pub matched: u64,
    /// 大小或哈希不一致的文件数（疑似被篡改/加密）。
    pub mismatch: u64,
    /// 清单有记录但磁盘缺失的文件数。
    pub missing: u64,
    /// 磁盘存在但清单无记录的文件数（如残留临时文件）。
    pub extra: u64,
}

/// 校验快照完整性：遍历磁盘文件，逐一对大小与全量哈希，
/// 与清单比对，输出差异汇总。
pub fn verify(snap_dir: &Path) -> Result<VerifyReport, BackupError> {
    let manifest = Manifest::read(snap_dir)?
        .ok_or_else(|| BackupError::MissingManifest(snap_dir.display().to_string()))?;
    let mut report = VerifyReport::default();
    let mut seen: HashSet<String> = HashSet::new();

    for entry in WalkDir::new(snap_dir).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!(
                    path = ?err.path().unwrap_or(snap_dir),
                    error = %err,
                    "校验时跳过无法访问的目录项"
                );
                continue;
            }
        };
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        // 跳过清单文件自身。
        if path.file_name().and_then(|n| n.to_str()) == Some(MANIFEST_FILE) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(snap_dir) else {
            continue;
        };
        let key = rel_to_key(rel);
        seen.insert(key.clone());

        match manifest.files.get(&key) {
            Some(expected) => {
                report.checked += 1;
                let actual_size = match fs::metadata(path) {
                    Ok(m) => m.len(),
                    Err(_) => {
                        report.mismatch += 1;
                        continue;
                    }
                };
                if actual_size != expected.size {
                    report.mismatch += 1;
                    continue;
                }
                match full_hash(path) {
                    Ok(h) if h.to_hex().as_str() == expected.hash.as_str() => {
                        report.matched += 1;
                    }
                    _ => report.mismatch += 1,
                }
            }
            None => report.extra += 1,
        }
    }

    // 清单有记录但磁盘上未找到的文件。
    for key in manifest.files.keys() {
        if !seen.contains(key) {
            report.missing += 1;
        }
    }

    Ok(report)
}

/// 将相对路径转换为清单 key（统一使用 `/` 分隔，跨平台一致）。
pub(crate) fn rel_to_key(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nov-nas-back-manifest-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest_with(file: &Path, content: &[u8]) -> Manifest {
        fs::write(file, content).unwrap();
        let hash = full_hash(file).unwrap();
        let size = content.len() as u64;
        let key = rel_to_key(file.file_name().unwrap().as_ref());
        let mut manifest = Manifest::default();
        manifest.files.insert(
            key,
            ManifestEntry {
                size,
                hash: hash.to_hex().to_string(),
            },
        );
        manifest
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = temp_dir("roundtrip");
        let mut manifest = Manifest::default();
        manifest.files.insert(
            "a.txt".into(),
            ManifestEntry {
                size: 5,
                hash: "abc".into(),
            },
        );
        manifest.write(&dir).unwrap();
        let read = Manifest::read(&dir).unwrap().unwrap();
        assert_eq!(read.files.len(), 1);
        assert_eq!(read.files["a.txt"].size, 5);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_detects_tampering() {
        let dir = temp_dir("tamper");
        let file = dir.join("a.txt");
        let manifest = manifest_with(&file, b"hello");
        manifest.write(&dir).unwrap();

        // 篡改内容（大小不变），哈希改变。
        fs::write(&file, b"HELLO").unwrap();
        let report = verify(&dir).unwrap();
        assert_eq!(report.mismatch, 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_detects_missing() {
        let dir = temp_dir("missing");
        let file = dir.join("a.txt");
        let mut manifest = manifest_with(&file, b"hello");
        // 记录一个磁盘上不存在的文件。
        manifest.files.insert(
            "b.txt".into(),
            ManifestEntry {
                size: 3,
                hash: "abc".into(),
            },
        );
        manifest.write(&dir).unwrap();

        let report = verify(&dir).unwrap();
        assert_eq!(report.matched, 1);
        assert_eq!(report.missing, 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_detects_extra() {
        let dir = temp_dir("extra");
        let file = dir.join("a.txt");
        let manifest = manifest_with(&file, b"hello");
        manifest.write(&dir).unwrap();

        // 多出一个清单未记录的文件。
        fs::write(dir.join("stray.txt"), b"stray").unwrap();
        let report = verify(&dir).unwrap();
        assert_eq!(report.matched, 1);
        assert_eq!(report.extra, 1);
        fs::remove_dir_all(&dir).ok();
    }
}
