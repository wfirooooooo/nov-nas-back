//! 领域错误定义。
//!
//! 按工程规范：底层库/领域模型使用 [`thiserror`] 定义精确错误枚举，
//! 顶层二进制入口使用 [`anyhow`] 传播。

/// 配置文件相关错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("读取配置文件失败: {0}")]
    Read(#[source] std::io::Error),
    #[error("解析配置文件失败: {0}")]
    Parse(#[source] toml::de::Error),
    #[error("配置无效: {0}")]
    Invalid(String),
}

/// 备份执行相关错误。
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("文件 I/O 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("任务线程异常退出: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("未找到名为 '{name}' 的备份任务")]
    JobNotFound { name: String },
    #[error("备份任务 '{job}' 有 {failed} 个文件复制失败")]
    PartialFailure { job: String, failed: u64 },
}
