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
}
