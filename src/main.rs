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
use nov_nas_back::backup::verify;
use nov_nas_back::backup::{restore, snapshot};
use nov_nas_back::check;
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
            check::check(&config, None).await?;
        }
        Command::Verify { config, job, all } => {
            let config = Config::load(&config)?;
            let _guard = init_logging(&config.global.log_dir)?;
            verify(&config, job.as_deref(), all).await?;
        }
        Command::List { config, job } => {
            let config = Config::load(&config)?;
            list(&config, job.as_deref()).await?;
        }
        Command::Restore {
            config,
            job,
            snapshot,
            target,
        } => {
            let config = Config::load(&config)?;
            let _guard = init_logging(&config.global.log_dir)?;
            restore_snapshot(&config, &job, &snapshot, &target).await?;
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

/// 校验每个（或指定）任务快照的完整性。`all` 为真时校验全部快照，否则仅最新。
async fn verify(config: &Config, job_filter: Option<&str>, all: bool) -> anyhow::Result<()> {
    let mut matched = false;
    let mut any_failed = false;
    for job in &config.jobs {
        let is_match = job_filter.map(|f| f == job.name).unwrap_or(true);
        if !is_match {
            continue;
        }
        matched = true;

        let name = job.name.clone();
        let dest = job.dest.clone();
        let name_for_closure = name.clone();
        let ok = tokio::task::spawn_blocking(move || verify_job(&name_for_closure, &dest, all))
            .await??;
        if !ok {
            any_failed = true;
        }
    }
    if job_filter.is_some() && !matched {
        anyhow::bail!("未找到名为 '{}' 的备份任务", job_filter.unwrap_or_default());
    }
    if any_failed {
        anyhow::bail!("完整性校验未通过");
    }
    info!("完整性校验通过");
    Ok(())
}

/// 校验单个任务的快照（`all` 为真时遍历全部，否则仅最新），
/// 返回是否全部通过。
fn verify_job(name: &str, dest: &Path, all: bool) -> anyhow::Result<bool> {
    let results = verify::verify_snapshots(dest, all)?;
    if results.is_empty() {
        anyhow::bail!("任务 '{}' 尚无快照可校验", name);
    }

    let mut ok = true;
    for result in &results {
        let report = &result.report;
        info!(
            job = %name,
            snap = %result.snap_dir.display(),
            checked = report.checked,
            matched = report.matched,
            mismatch = report.mismatch,
            missing = report.missing,
            extra = report.extra,
            "快照完整性校验完成"
        );
        if report.mismatch > 0 || report.missing > 0 {
            error!(
                job = %name,
                snap = %result.snap_dir.display(),
                mismatch = report.mismatch,
                missing = report.missing,
                "快照校验未通过"
            );
            ok = false;
        }
    }
    Ok(ok)
}

/// 列出每个（或指定）任务的快照信息。
async fn list(config: &Config, job_filter: Option<&str>) -> anyhow::Result<()> {
    let mut matched = false;
    for job in &config.jobs {
        let is_match = job_filter.map(|f| f == job.name).unwrap_or(true);
        if !is_match {
            continue;
        }
        matched = true;

        let dest = job.dest.clone();
        let name = job.name.clone();
        let infos = tokio::task::spawn_blocking(move || snapshot::list(&dest)).await??;
        println!("任务 '{name}' 共 {} 个快照：", infos.len());
        for info in &infos {
            println!("  {}  ({} 个文件)", info.name, info.files);
        }
    }
    if job_filter.is_some() && !matched {
        anyhow::bail!("未找到名为 '{}' 的备份任务", job_filter.unwrap_or_default());
    }
    Ok(())
}

/// 将指定 job 的指定快照恢复到目标目录。
async fn restore_snapshot(
    config: &Config,
    job_name: &str,
    snapshot_name: &str,
    target: &Path,
) -> anyhow::Result<()> {
    let job = config
        .jobs
        .iter()
        .find(|j| j.name == job_name)
        .ok_or_else(|| anyhow::anyhow!("未找到名为 '{job_name}' 的备份任务"))?;
    let snap_dir = snapshot::find(&job.dest, snapshot_name)
        .ok_or_else(|| anyhow::anyhow!("任务 '{job_name}' 未找到名为 '{snapshot_name}' 的快照"))?;

    let target = target.to_path_buf();
    let snap_dir_for_closure = snap_dir.clone();
    let summary =
        tokio::task::spawn_blocking(move || restore::restore(&snap_dir_for_closure, &target))
            .await??;

    info!(
        job = %job_name,
        snapshot = %snapshot_name,
        restored = summary.restored,
        failed = summary.failed,
        "恢复完成"
    );
    if summary.failed > 0 {
        anyhow::bail!("任务 '{job_name}' 有 {} 个文件恢复失败", summary.failed);
    }
    Ok(())
}
