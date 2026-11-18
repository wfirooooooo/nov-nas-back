//! verify 端到端集成测试：备份 → 篡改 → 校验检出。

use std::fs;
use std::path::PathBuf;

use nov_nas_back::backup::BackupEngine;
use nov_nas_back::backup::snapshot;
use nov_nas_back::backup::verify::verify_snapshots;
use nov_nas_back::config::{Config, GlobalConfig, JobConfig};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nov-nas-back-it-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn backup_config(src: PathBuf, dest: PathBuf) -> Config {
    Config {
        global: GlobalConfig::default(),
        jobs: vec![JobConfig {
            name: "test".to_string(),
            source: src,
            dest,
        }],
    }
}

#[tokio::test]
async fn tampered_snapshot_is_detected() {
    let root = temp_dir("tamper");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"hello").unwrap();

    let engine = BackupEngine::new(backup_config(src, dest.clone()));
    engine.run(None).await.unwrap();

    // 篡改最新快照中的文件（大小不变，哈希改变）。
    let latest = snapshot::latest(&dest).unwrap().unwrap();
    fs::write(latest.join("a.txt"), b"HELLO").unwrap();

    let results = verify_snapshots(&dest, false).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].report.mismatch >= 1);

    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn intact_snapshot_passes_verification() {
    let root = temp_dir("intact");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"hello").unwrap();
    fs::write(src.join("b.txt"), b"world").unwrap();

    let engine = BackupEngine::new(backup_config(src, dest.clone()));
    engine.run(None).await.unwrap();

    let results = verify_snapshots(&dest, false).unwrap();
    assert_eq!(results.len(), 1);
    let report = &results[0].report;
    assert_eq!(report.matched, 2);
    assert_eq!(report.mismatch, 0);
    assert_eq!(report.missing, 0);

    fs::remove_dir_all(&root).ok();
}
