//! 命令行参数定义。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "nov-nas-back", version, about = "群晖 NAS 离线增量备份工具")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 执行备份（全部或指定 job）
    Run {
        /// 配置文件路径
        #[arg(long, default_value = "config.toml")]
        config: PathBuf,
        /// 仅执行指定名称的 job
        #[arg(long)]
        job: Option<String>,
    },
    /// 校验配置与源/目标路径可达性
    Check {
        /// 配置文件路径
        #[arg(long, default_value = "config.toml")]
        config: PathBuf,
    },
    /// 校验最新快照的完整性（与清单比对）
    Verify {
        /// 配置文件路径
        #[arg(long, default_value = "config.toml")]
        config: PathBuf,
        /// 仅校验指定名称的 job
        #[arg(long)]
        job: Option<String>,
        /// 校验所有快照（默认仅最新）
        #[arg(long)]
        all: bool,
    },
    /// 列出全部（或指定 job）的快照
    List {
        /// 配置文件路径
        #[arg(long, default_value = "config.toml")]
        config: PathBuf,
        /// 仅列出指定名称的 job
        #[arg(long)]
        job: Option<String>,
    },
    /// 从指定快照恢复到目标目录
    Restore {
        /// 配置文件路径
        #[arg(long, default_value = "config.toml")]
        config: PathBuf,
        /// 要恢复的 job 名称
        #[arg(long)]
        job: String,
        /// 快照目录名（时间戳，如 20261118-090000）
        #[arg(long)]
        snapshot: String,
        /// 恢复目标目录
        #[arg(long)]
        target: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse_verify(args: &[&str]) -> (PathBuf, Option<String>, bool) {
        let cli = Cli::try_parse_from(args).unwrap();
        match cli.command {
            Command::Verify { config, job, all } => (config, job, all),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn verify_uses_defaults() {
        let (config, job, all) = parse_verify(&["nov-nas-back", "verify"]);
        assert_eq!(config, PathBuf::from("config.toml"));
        assert!(job.is_none());
        assert!(!all);
    }

    #[test]
    fn verify_all_flag() {
        let (_, _, all) = parse_verify(&["nov-nas-back", "verify", "--all"]);
        assert!(all);
    }

    #[test]
    fn verify_job_filter() {
        let (_, job, _) = parse_verify(&["nov-nas-back", "verify", "--job", "photo"]);
        assert_eq!(job.as_deref(), Some("photo"));
    }

    #[test]
    fn verify_config_override() {
        let (config, _, _) = parse_verify(&["nov-nas-back", "verify", "--config", "/tmp/c.toml"]);
        assert_eq!(config, PathBuf::from("/tmp/c.toml"));
    }

    #[test]
    fn verify_combined_flags() {
        let (config, job, all) = parse_verify(&[
            "nov-nas-back",
            "verify",
            "--config",
            "/tmp/c.toml",
            "--job",
            "photo",
            "--all",
        ]);
        assert_eq!(config, PathBuf::from("/tmp/c.toml"));
        assert_eq!(job.as_deref(), Some("photo"));
        assert!(all);
    }
}
