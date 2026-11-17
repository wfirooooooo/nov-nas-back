//! 二进制入口：解析命令行 → 加载配置 → 初始化日志 → 分发命令。

use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use tracing::{error, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use nov_nas_back::backup::BackupEngine;
use nov_nas_back::cli::{Cli, Command};
use nov_nas_back::config::Config;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("执行失败: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Run { config, job } => {
            let config = Config::load(&config)?;
            let _guard = init_logging(&config.global.log_dir)?;
            let engine = BackupEngine::new(config);
            engine.run(job.as_deref()).await?;
        }
        Command::Check { config } => {
            let config = Config::load(&config)?;
            let _guard = init_logging(&config.global.log_dir)?;
            check(&config).await?;
        }
    }
    Ok(())
}

/// 初始化日志：控制台（stderr）+ 滚动文件（按天）。
///
/// 返回的 [`WorkerGuard`] 需在调用方存活期间保持，以保证文件日志完整落盘。
fn init_logging(log_dir: &Path) -> anyhow::Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let file_appender = tracing_appender::rolling::daily(log_dir, "nov-nas-back.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(filter),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_filter(file_filter),
        )
        .init();

    Ok(guard)
}

/// 校验配置与源/目标路径可达性（供排障使用）。
async fn check(config: &Config) -> anyhow::Result<()> {
    for job in &config.jobs {
        match tokio::fs::metadata(&job.source).await {
            Ok(_) => info!(job = %job.name, source = %job.source.display(), "源路径可达"),
            Err(err) => {
                error!(
                    job = %job.name,
                    source = %job.source.display(),
                    error = %err,
                    "源路径不可达，请检查 NAS 连接与 Windows 已存凭据"
                );
                anyhow::bail!("任务 '{}' 源不可达: {err}", job.name);
            }
        }
        tokio::fs::create_dir_all(&job.dest).await?;
        info!(job = %job.name, dest = %job.dest.display(), "目标路径可写");
    }
    info!("校验通过，共 {} 个任务", config.jobs.len());
    Ok(())
}
