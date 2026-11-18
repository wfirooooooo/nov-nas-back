//! 备份引擎：目录遍历、文件比对与流式拷贝。

pub mod copier;
pub mod differ;
pub mod engine;
pub mod manifest;
pub mod restore;
pub mod snapshot;
pub mod verify;

pub use engine::{BackupEngine, JobSummary, Summary};
