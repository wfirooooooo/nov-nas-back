//! List 端到端集成测试：列出快照信息。

use std::fs;
use std::path::PathBuf;

use nov_nas_back::backup::BackupEngine;
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
async fn list_reports_snapshots_with_file_counts() {
    let root = temp_dir("list");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"hello").unwrap();
    fs::write(src.join("b.txt"), b"world").unwrap();

    let config = backup_config(src, dest.clone());
    BackupEngine::new(config.clone()).run(None).await.unwrap();
    BackupEngine::new(config).run(None).await.unwrap();

    let infos = snapshot::list(&dest).unwrap();
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0].files, 2);
    assert_eq!(infos[1].files, 2);

    fs::remove_dir_all(&root).ok();
}
