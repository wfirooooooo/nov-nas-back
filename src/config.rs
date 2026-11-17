//! 配置加载与校验。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConfigError;

fn default_concurrency() -> usize {
    4
}

fn default_log_dir() -> PathBuf {
    PathBuf::from("logs")
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
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            concurrency: default_concurrency(),
            rate_limit_mbps: 0.0,
            log_dir: default_log_dir(),
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
        for job in &self.jobs {
            if job.name.trim().is_empty() {
                return Err(ConfigError::Invalid("job.name 不能为空".into()));
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
