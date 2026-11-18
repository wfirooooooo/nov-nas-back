//! Restore 端到端集成测试：从快照恢复文件到目标目录。

use std::fs;
use std::path::PathBuf;

use nov_nas_back::backup::BackupEngine;
use nov_nas_back::backup::restore;
use nov_nas_back::backup::snapshot;
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
async fn restore_recovers_files_to_target() {
    let root = temp_dir("restore");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"hello").unwrap();
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("sub").join("b.txt"), b"world").unwrap();

    let engine = BackupEngine::new(backup_config(src, dest.clone()));
    engine.run(None).await.unwrap();

    // 从最新快照恢复到目标目录。
    let latest = snapshot::latest(&dest).unwrap().unwrap();
    let target = root.join("restored");
    let summary = restore::restore(&latest, &target).unwrap();

    assert_eq!(summary.restored, 2);
    assert_eq!(summary.failed, 0);
    assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"hello");
    assert_eq!(
        fs::read(target.join("sub").join("b.txt")).unwrap(),
        b"world"
    );

    fs::remove_dir_all(&root).ok();
}
