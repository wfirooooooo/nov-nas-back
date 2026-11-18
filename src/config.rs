//! 配置加载与校验。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConfigError;

fn default_concurrency() -> usize {
    4
}

fn default_log_dir() -> PathBuf {
    PathBuf::from("logs")
}

fn default_keep_snapshots() -> usize {
    7
}

fn default_change_rate_threshold() -> f64 {
    0.3
}

fn default_min_changed_files() -> usize {
    100
}

/// 内容校验级别：决定「元数据一致」的文件是否进一步比对内容。
///
/// 勒索软件可能保留 mtime/size 以伪装「未变化」，仅靠元数据会漏备份；
/// 更高校验级别通过比对文件内容捕获这类伪造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verify {
    /// 仅元数据（size + mtime）比对，最快。
    Size,
    /// 元数据一致时采样哈希（头尾 4 KiB）复核。
    #[default]
    Sample,
    /// 元数据一致时全量 BLAKE3 复核，最慢但最可靠。
    Full,
}

/// 全局配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    /// 并发复制的文件数。
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// 全局限速（MB/s），0 表示不限速。
    #[serde(default)]
    pub rate_limit_mbps: f64,
    /// 滚动日志输出目录。
    #[serde(default = "default_log_dir")]
    pub log_dir: PathBuf,
    /// 保留的快照数量，0 表示不自动裁剪。
    #[serde(default = "default_keep_snapshots")]
    pub keep_snapshots: usize,
    /// 变化率门禁阈值（被修改文件占已有文件的比例），超过则中止备份；
    /// 0 表示禁用门禁。
    #[serde(default = "default_change_rate_threshold")]
    pub change_rate_threshold: f64,
    /// 变化率门禁的绝对值兜底：被修改文件数达到该值才可能触发，
    /// 避免小目录（如仅数十个文件）因比例波动而误报。
    #[serde(default = "default_min_changed_files")]
    pub min_changed_files: usize,
    /// 内容校验级别（默认 Sample）。
    #[serde(default)]
    pub verify: Verify,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            concurrency: default_concurrency(),
            rate_limit_mbps: 0.0,
            log_dir: default_log_dir(),
            keep_snapshots: default_keep_snapshots(),
            change_rate_threshold: default_change_rate_threshold(),
            min_changed_files: default_min_changed_files(),
            verify: Verify::default(),
        }
    }
}

/// 单个备份任务（源 → 目标）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobConfig {
    /// 任务名称（用于日志与 `--job` 过滤）。
    pub name: String,
    /// 源路径：NAS 共享文件夹的 UNC 路径，如 `\\192.168.1.10\photo`。
    pub source: PathBuf,
    /// 目标路径：本地磁盘目录，如 `D:\backup\photo`。
    pub dest: PathBuf,
}

/// 完整配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub global: GlobalConfig,
    #[serde(default)]
    pub jobs: Vec<JobConfig>,
}

impl Config {
    /// 从 TOML 文件加载并校验配置。
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(ConfigError::Read)?;
        let config: Config = toml::from_str(&content).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    /// 校验配置的合法性。
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.jobs.is_empty() {
            return Err(ConfigError::Invalid(
                "未配置任何备份任务（[[jobs]] 为空）".into(),
            ));
        }
        if self.global.concurrency == 0 {
            return Err(ConfigError::Invalid("global.concurrency 必须 >= 1".into()));
        }
        if self.global.rate_limit_mbps < 0.0 {
            return Err(ConfigError::Invalid(
                "global.rate_limit_mbps 不能为负数".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.global.change_rate_threshold) {
            return Err(ConfigError::Invalid(
                "global.change_rate_threshold 必须在 [0.0, 1.0] 区间内".into(),
            ));
        }
        let mut seen = HashSet::with_capacity(self.jobs.len());
        for job in &self.jobs {
            if job.name.trim().is_empty() {
                return Err(ConfigError::Invalid("job.name 不能为空".into()));
            }
            if !seen.insert(&job.name) {
                return Err(ConfigError::Invalid(format!(
                    "job.name 重复: '{}'",
                    job.name
                )));
            }
            if job.source.as_os_str().is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "任务 '{}' 的 source 不能为空",
                    job.name
                )));
            }
            if job.dest.as_os_str().is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "任务 '{}' 的 dest 不能为空",
                    job.name
                )));
            }
        }
        Ok(())
    }
}
