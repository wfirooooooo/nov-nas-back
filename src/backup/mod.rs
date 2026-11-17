//! 备份引擎：目录遍历、文件比对与流式拷贝。

pub mod copier;
pub mod differ;
pub mod engine;

pub use engine::{BackupEngine, JobSummary, Summary};
