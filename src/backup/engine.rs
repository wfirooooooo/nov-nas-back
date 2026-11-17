//! 备份引擎：有界并发调度、目录遍历与汇总统计。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Mutex, mpsc};
use tracing::{error, info, warn};
use walkdir::WalkDir;

use crate::backup::copier;
use crate::backup::differ::{self, Action};
use crate::config::{Config, JobConfig};
use crate::error::BackupError;
use crate::throttle::RateLimiter;

/// 单个待复制文件。
struct WorkItem {
    src: PathBuf,
    dest: PathBuf,
}

/// 整体备份结果汇总。
#[derive(Debug, Default)]
pub struct Summary {
    pub jobs: Vec<JobSummary>,
}

/// 单个任务的汇总统计。
#[derive(Debug, Default)]
pub struct JobSummary {
    pub name: String,
    /// 扫描到的文件总数。
    pub scanned: u64,
    /// 实际复制的文件数。
    pub copied: u64,
    /// 跳过的未变文件数。
    pub skipped: u64,
    /// 复制失败的文件数。
    pub failed: u64,
    /// 复制的总字节数。
    pub bytes: u64,
}

/// 扫描阶段统计。
#[derive(Debug, Default)]
struct ScanStats {
    scanned: u64,
    skipped: u64,
}

/// 备份引擎，负责调度所有任务。
pub struct BackupEngine {
    config: Config,
    limiter: RateLimiter,
}

impl BackupEngine {
    pub fn new(config: Config) -> Self {
        let bytes_per_sec = config.global.rate_limit_mbps * 1024.0 * 1024.0;
        let limiter = if bytes_per_sec > 0.0 {
            RateLimiter::new(bytes_per_sec)
        } else {
            RateLimiter::unlimited()
        };
        Self { config, limiter }
    }

    /// 执行全部（或指定）任务。单个任务失败不阻断其余任务，
    /// 但会在结束后返回首个错误以给出非零退出码。
    pub async fn run(&self, job_filter: Option<&str>) -> Result<Summary, BackupError> {
        let mut jobs = Vec::new();
        let mut first_error: Option<BackupError> = None;
        let mut matched = false;

        for job in &self.config.jobs {
            let is_match = job_filter.map(|f| f == job.name).unwrap_or(true);
            if !is_match {
                continue;
            }
            matched = true;
            match self.run_job(job).await {
                Ok(summary) => jobs.push(summary),
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }

        if job_filter.is_some() && !matched {
            return Err(BackupError::JobNotFound {
                name: job_filter.unwrap_or_default().to_string(),
            });
        }

        if let Some(err) = first_error {
            return Err(err);
        }
        Ok(Summary { jobs })
    }

    /// 执行单个任务：遍历源目录 → 有界 channel → 固定 worker 并发拷贝。
    async fn run_job(&self, job: &JobConfig) -> Result<JobSummary, BackupError> {
        info!(
            job = %job.name,
            source = %job.source.display(),
            dest = %job.dest.display(),
            "开始备份"
        );

        let (tx, rx) = mpsc::channel::<WorkItem>(1024);
        let rx = Arc::new(Mutex::new(rx));

        let copied = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicU64::new(0));
        let bytes = Arc::new(AtomicU64::new(0));

        let concurrency = self.config.global.concurrency;
        let mut handles = Vec::with_capacity(concurrency);
        for _ in 0..concurrency {
            let rx = Arc::clone(&rx);
            let limiter = self.limiter.clone();
            let copied = Arc::clone(&copied);
            let failed = Arc::clone(&failed);
            let bytes = Arc::clone(&bytes);
            handles.push(tokio::spawn(async move {
                loop {
                    let item = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    let Some(item) = item else {
                        break;
                    };
                    match copier::copy_file(&item.src, &item.dest, &limiter).await {
                        Ok(n) => {
                            copied.fetch_add(1, Ordering::Relaxed);
                            bytes.fetch_add(n, Ordering::Relaxed);
                            info!(file = %item.src.display(), "已复制");
                        }
                        Err(err) => {
                            failed.fetch_add(1, Ordering::Relaxed);
                            error!(file = %item.src.display(), error = %err, "复制失败");
                        }
                    }
                }
            }));
        }

        // 源目录遍历（含目录创建）放入阻塞线程池，避免阻塞异步运行时。
        let source = job.source.clone();
        let dest = job.dest.clone();
        let scan = tokio::task::spawn_blocking(move || scan_source(&source, &dest, &tx))
            .await
            .map_err(BackupError::Join)?;

        for handle in handles {
            handle.await.map_err(BackupError::Join)?;
        }

        let summary = JobSummary {
            name: job.name.clone(),
            scanned: scan.scanned,
            copied: copied.load(Ordering::Relaxed),
            skipped: scan.skipped,
            failed: failed.load(Ordering::Relaxed),
            bytes: bytes.load(Ordering::Relaxed),
        };

        info!(
            job = %summary.name,
            scanned = summary.scanned,
            copied = summary.copied,
            skipped = summary.skipped,
            failed = summary.failed,
            bytes = summary.bytes,
            "备份完成"
        );

        if summary.failed > 0 {
            return Err(BackupError::PartialFailure {
                job: summary.name,
                failed: summary.failed,
            });
        }
        Ok(summary)
    }
}

/// 遍历源目录：创建目标目录、比对元数据、将待复制文件送入 channel。
///
/// 该函数在 [`tokio::task::spawn_blocking`] 中执行，内部使用同步 I/O。
fn scan_source(source: &Path, dest: &Path, tx: &mpsc::Sender<WorkItem>) -> ScanStats {
    let mut stats = ScanStats::default();

    for entry in WalkDir::new(source).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!(
                    path = ?err.path().unwrap_or(source),
                    error = %err,
                    "跳过无法访问的目录项"
                );
                continue;
            }
        };
        let path = entry.path();

        if entry.file_type().is_dir() {
            // 同步创建目标目录（含空目录）。
            if let Ok(rel) = path.strip_prefix(source) {
                let dest_dir = dest.join(rel);
                if let Err(err) = std::fs::create_dir_all(&dest_dir) {
                    warn!(dir = %dest_dir.display(), error = %err, "创建目标目录失败");
                }
            }
            continue;
        }

        // 仅处理普通文件，跳过符号链接等其他类型。
        if !entry.file_type().is_file() {
            continue;
        }

        stats.scanned += 1;

        let Ok(rel) = path.strip_prefix(source) else {
            continue;
        };
        let dest_path = dest.join(rel);

        let src_meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(err) => {
                warn!(file = %path.display(), error = %err, "读取源文件元数据失败，跳过");
                continue;
            }
        };
        let dest_meta = std::fs::metadata(&dest_path).ok();

        if differ::decide(&src_meta, dest_meta.as_ref()) == Action::Skip {
            stats.skipped += 1;
            continue;
        }

        if tx
            .blocking_send(WorkItem {
                src: path.to_path_buf(),
                dest: dest_path,
            })
            .is_err()
        {
            // 接收端已关闭，停止扫描。
            break;
        }
    }

    stats
}
