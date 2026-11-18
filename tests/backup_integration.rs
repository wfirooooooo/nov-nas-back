//! Run 子命令端到端集成测试：快照、清单、增量硬链接。

use std::fs;
use std::path::PathBuf;

use nov_nas_back::backup::BackupEngine;
use nov_nas_back::backup::manifest::Manifest;
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
async fn first_backup_creates_snapshot_and_manifest() {
    let root = temp_dir("first");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"hello").unwrap();
    fs::write(src.join("b.txt"), b"world").unwrap();

    let engine = BackupEngine::new(backup_config(src, dest.clone()));
    let summary = engine.run(None).await.unwrap();
    assert_eq!(summary.jobs[0].copied, 2);

    // 最新快照存在，且含清单与两个文件。
    let latest = snapshot::latest(&dest).unwrap().unwrap();
    assert!(latest.join("a.txt").exists());
    assert!(latest.join("b.txt").exists());
    assert!(Manifest::read(&latest).unwrap().is_some());

    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn incremental_backup_links_unchanged_files() {
    let root = temp_dir("incremental");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"hello").unwrap();
    fs::write(src.join("b.txt"), b"world").unwrap();

    let config = backup_config(src, dest.clone());

    // 首次备份：全部拷贝。
    let first = BackupEngine::new(config.clone()).run(None).await.unwrap();
    assert_eq!(first.jobs[0].copied, 2);

    // 第二次备份（源未变）：全部硬链接复用，无实际传输。
    let second = BackupEngine::new(config).run(None).await.unwrap();
    assert_eq!(second.jobs[0].linked, 2);
    assert_eq!(second.jobs[0].copied, 0);
    assert_eq!(second.jobs[0].bytes, 0);

    // 快照数应为 2。
    assert_eq!(snapshot::all(&dest).unwrap().len(), 2);

    fs::remove_dir_all(&root).ok();
}
