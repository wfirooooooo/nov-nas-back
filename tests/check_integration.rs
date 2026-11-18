//! Check 子命令端到端集成测试：源可达性与目标可写性。

use std::fs;
use std::path::PathBuf;

use nov_nas_back::check::check;
use nov_nas_back::config::{Config, GlobalConfig, JobConfig};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nov-nas-back-it-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(src: PathBuf, dest: PathBuf) -> Config {
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
async fn check_passes_when_source_and_dest_ok() {
    let root = temp_dir("ok");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).unwrap();

    check(&config(src, dest.clone()), None).await.unwrap();
    // 目标目录应被创建。
    assert!(dest.exists());

    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn check_fails_when_source_missing() {
    let root = temp_dir("missing");
    let src = root.join("no-such-src");
    let dest = root.join("dest");

    let result = check(&config(src, dest), None).await;
    assert!(result.is_err());

    fs::remove_dir_all(&root).ok();
}
