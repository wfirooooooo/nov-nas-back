//! 备份任务预检：校验源路径可达性与目标路径可写性。
//!
//! 供 CLI 的 `check` 子命令与集成测试复用。

use tokio::fs;
use tracing::{error, info};

use crate::config::{Config, JobConfig};
use crate::error::CheckError;

/// 校验每个（或指定）任务的源可达性与目标可写性。
pub async fn check(config: &Config, job_filter: Option<&str>) -> Result<(), CheckError> {
    let mut matched = false;
    for job in &config.jobs {
        let is_match = job_filter.map(|f| f == job.name).unwrap_or(true);
        if !is_match {
            continue;
        }
        matched = true;
        check_job(job).await?;
    }
    if job_filter.is_some() && !matched {
        return Err(CheckError::JobNotFound {
            name: job_filter.unwrap_or_default().to_string(),
        });
    }
    info!("校验通过，共 {} 个任务", config.jobs.len());
    Ok(())
}

async fn check_job(job: &JobConfig) -> Result<(), CheckError> {
    match fs::metadata(&job.source).await {
        Ok(_) => info!(job = %job.name, source = %job.source.display(), "源路径可达"),
        Err(err) => {
            error!(
                job = %job.name,
                source = %job.source.display(),
                error = %err,
                "源路径不可达，请检查 NAS 连接与 Windows 已存凭据"
            );
            return Err(CheckError::SourceUnreachable {
                job: job.name.clone(),
                source: err,
            });
        }
    }
    fs::create_dir_all(&job.dest)
        .await
        .map_err(|source| CheckError::DestUnwritable {
            job: job.name.clone(),
            source,
        })?;
    info!(job = %job.name, dest = %job.dest.display(), "目标路径可写");
    Ok(())
}
