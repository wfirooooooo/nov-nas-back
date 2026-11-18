//! 备份引擎：有界并发调度、目录遍历与汇总统计。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use filetime::FileTime;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use crate::backup::copier::{self, CopyOutcome};
use crate::backup::differ::{self, Action};
use crate::backup::manifest::{self, Manifest, ManifestEntry};
use crate::backup::snapshot;
use crate::config::{Config, JobConfig, Verify};
use crate::error::BackupError;
use crate::throttle::RateLimiter;

/// 单个待处理文件。
struct WorkItem {
    src: PathBuf,
    dest: PathBuf,
    /// 相对路径（用于完整性清单的 key）。
    rel: PathBuf,
    /// 上一快照中的同名文件（仅 `Action::Link` 使用）。
    prev: Option<PathBuf>,
    /// 源文件大小（扫描时），用于拷贝后校验完整性。
    size: u64,
    /// 源文件 mtime（扫描时），用于回写，避免拷贝后重读的竞态。
    mtime: FileTime,
    /// 处理方式：硬链接复用或流式拷贝。
    op: Action,
}

/// 单个文件处理完成后，用于构建完整性清单的记录。
struct ManifestRecord {
    rel: PathBuf,
    size: u64,
    /// 拷贝文件为 `Some`（hex 哈希）；硬链接文件为 `None`（继承上一清单）。
    hash: Option<String>,
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
    /// 硬链接复用上一快照的文件数。
    pub linked: u64,
    /// 复制失败的文件数。
    pub failed: u64,
    /// 复制的总字节数。
    pub bytes: u64,
}

/// 扫描阶段统计。
#[derive(Debug, Default)]
struct ScanStats {
    scanned: u64,
    linked: u64,
    /// 已存在文件被修改的数量（变化率门禁的关键指标）。
    changed: u64,
    /// 新增文件数量。
    new: u64,
}

/// 扫描阶段的行为配置（变化率门禁与内容校验）。
struct ScanConfig {
    change_rate_threshold: f64,
    min_changed_files: usize,
    verify: Verify,
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

    /// 执行单个任务：定位快照 → 遍历源目录 → 有界 channel → 固定 worker 并发处理。
    async fn run_job(&self, job: &JobConfig) -> Result<JobSummary, BackupError> {
        info!(
            job = %job.name,
            source = %job.source.display(),
            dest = %job.dest.display(),
            "开始备份"
        );

        // 定位本次快照目录与上一快照目录（作为硬链接与比对基准）。
        let (snap_dir, prev_dir) = snapshot::prepare(&job.dest)?;
        info!(
            job = %job.name,
            snap = %snap_dir.display(),
            prev = ?prev_dir,
            "快照已就绪"
        );

        let (tx, rx) = mpsc::channel::<WorkItem>(1024);
        let rx = Arc::new(Mutex::new(rx));

        let copied = Arc::new(AtomicU64::new(0));
        let linked = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicU64::new(0));
        let bytes = Arc::new(AtomicU64::new(0));

        let concurrency = self.config.global.concurrency;
        let mut handles = Vec::with_capacity(concurrency);
        for _ in 0..concurrency {
            let rx = Arc::clone(&rx);
            let limiter = self.limiter.clone();
            let copied = Arc::clone(&copied);
            let linked = Arc::clone(&linked);
            let failed = Arc::clone(&failed);
            let bytes = Arc::clone(&bytes);
            handles.push(tokio::spawn(async move {
                let mut records = Vec::new();
                loop {
                    let item = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    let Some(item) = item else {
                        break;
                    };
                    match process_item(&item, &limiter).await {
                        Ok(CopyOutcome::Linked) => {
                            linked.fetch_add(1, Ordering::Relaxed);
                            info!(file = %item.src.display(), "已链接");
                            records.push(ManifestRecord {
                                rel: item.rel,
                                size: item.size,
                                hash: None,
                            });
                        }
                        Ok(CopyOutcome::Copied { bytes: n, hash }) => {
                            copied.fetch_add(1, Ordering::Relaxed);
                            bytes.fetch_add(n, Ordering::Relaxed);
                            info!(file = %item.src.display(), "已复制");
                            records.push(ManifestRecord {
                                rel: item.rel,
                                size: item.size,
                                hash: Some(hash.to_hex().to_string()),
                            });
                        }
                        Err(err) => {
                            failed.fetch_add(1, Ordering::Relaxed);
                            error!(file = %item.src.display(), error = %err, "处理失败");
                        }
                    }
                }
                records
            }));
        }

        // 源目录遍历（含目录创建）放入阻塞线程池，避免阻塞异步运行时。
        let source = job.source.clone();
        let snap_for_scan = snap_dir.clone();
        let prev_for_scan = prev_dir.clone();
        let scan_cfg = ScanConfig {
            change_rate_threshold: self.config.global.change_rate_threshold,
            min_changed_files: self.config.global.min_changed_files,
            verify: self.config.global.verify,
        };
        let scan = match tokio::task::spawn_blocking(move || {
            scan_source(
                &source,
                &snap_for_scan,
                prev_for_scan.as_deref(),
                &tx,
                &scan_cfg,
            )
        })
        .await
        .map_err(BackupError::Join)?
        {
            Ok(stats) => stats,
            Err(err) => {
                // 门禁触发：清理本次残留的空快照目录，避免其成为下次运行的
                // 「上一快照」而使所有文件被误判为新增、绕过门禁。
                if let Err(cleanup_err) = tokio::fs::remove_dir_all(&snap_dir).await {
                    warn!(dir = %snap_dir.display(), error = %cleanup_err, "清理残留空快照目录失败");
                }
                return Err(err);
            }
        };

        let mut records = Vec::new();
        for handle in handles {
            records.extend(handle.await.map_err(BackupError::Join)?);
        }

        info!(
            job = %job.name,
            scanned = scan.scanned,
            linked = scan.linked,
            changed = scan.changed,
            new = scan.new,
            "扫描完成，变化率门禁通过"
        );

        // 生成完整性清单：拷贝文件使用拷贝时计算的哈希，
        // 硬链接文件从上一清单继承（内容一致，哈希相同）。
        let prev_manifest = prev_dir
            .as_deref()
            .and_then(|d| Manifest::read(d).ok().flatten());
        let mut new_manifest = Manifest::default();
        for record in records {
            let key = manifest::rel_to_key(&record.rel);
            let hash = match record.hash {
                Some(h) => h,
                None => match prev_manifest.as_ref().and_then(|m| m.files.get(&key)) {
                    Some(entry) => entry.hash.clone(),
                    None => {
                        warn!(file = %record.rel.display(), "硬链接文件无法继承上一清单哈希，跳过");
                        continue;
                    }
                },
            };
            new_manifest.files.insert(
                key,
                ManifestEntry {
                    size: record.size,
                    hash,
                },
            );
        }
        new_manifest.write(&snap_dir)?;

        let summary = JobSummary {
            name: job.name.clone(),
            scanned: scan.scanned,
            copied: copied.load(Ordering::Relaxed),
            linked: linked.load(Ordering::Relaxed),
            failed: failed.load(Ordering::Relaxed),
            bytes: bytes.load(Ordering::Relaxed),
        };

        info!(
            job = %summary.name,
            scanned = summary.scanned,
            copied = summary.copied,
            linked = summary.linked,
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

        // 仅在全部成功时裁剪旧快照，失败时保留全部快照以保安全。
        if let Err(err) = snapshot::prune(&job.dest, self.config.global.keep_snapshots) {
            warn!(job = %summary.name, error = %err, "裁剪旧快照失败");
        }

        Ok(summary)
    }
}

/// 按 [`Action`] 分发单个文件到硬链接或流式拷贝。
///
/// `Action::Link` 在上一快照路径缺失（理论上不发生）时降级为拷贝，
/// 避免引入 `unwrap`/`expect`。
async fn process_item(item: &WorkItem, limiter: &RateLimiter) -> Result<CopyOutcome, BackupError> {
    match item.op {
        Action::New | Action::Changed => {
            let (bytes, hash) =
                copier::copy_file(&item.src, &item.dest, item.size, item.mtime, limiter).await?;
            Ok(CopyOutcome::Copied { bytes, hash })
        }
        Action::Link => match &item.prev {
            Some(prev) => {
                copier::link_or_fallback(
                    prev, &item.dest, &item.src, item.size, item.mtime, limiter,
                )
                .await
            }
            None => {
                let (bytes, hash) =
                    copier::copy_file(&item.src, &item.dest, item.size, item.mtime, limiter)
                        .await?;
                Ok(CopyOutcome::Copied { bytes, hash })
            }
        },
    }
}

/// 遍历源目录：创建快照子目录、与上一快照比对元数据、按变化率门禁判断后
/// 将待处理文件送入 channel。
///
/// 该函数在 [`tokio::task::spawn_blocking`] 中执行，内部使用同步 I/O。
/// 采用「先收集、后门禁、再分发」两阶段：只有门禁通过才真正发送到拷贝队列，
/// 从而在加密传染大规模发生时（大量已有文件被修改）中止备份，不清空旧快照。
///
/// 内存权衡：本函数将待处理文件（含硬链接与拷贝）缓冲在 `Vec` 中，占用约为
/// 文件数 × 数百字节；对个人 NAS 备份规模（数万～数十万文件）可接受。
fn scan_source(
    source: &Path,
    snap_dir: &Path,
    prev_dir: Option<&Path>,
    tx: &mpsc::Sender<WorkItem>,
    cfg: &ScanConfig,
) -> Result<ScanStats, BackupError> {
    let mut stats = ScanStats::default();
    let mut items: Vec<WorkItem> = Vec::new();

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
            // 同步创建当前快照目录（含空目录）。
            if let Ok(rel) = path.strip_prefix(source) {
                let snap_sub = snap_dir.join(rel);
                if let Err(err) = std::fs::create_dir_all(&snap_sub) {
                    warn!(dir = %snap_sub.display(), error = %err, "创建目标目录失败");
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
        let dest_path = snap_dir.join(rel);

        let src_meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(err) => {
                warn!(file = %path.display(), error = %err, "读取源文件元数据失败，跳过");
                continue;
            }
        };
        // 上一快照中的同名文件元数据；缺失或读取失败视为 None（保守走拷贝）。
        let prev_path = prev_dir.map(|p| p.join(rel));
        let prev_meta = prev_path.as_deref().and_then(|p| std::fs::metadata(p).ok());

        let mut op = differ::decide(&src_meta, prev_meta.as_ref());
        // 内容复核：元数据判定「未变」的文件，按校验级别比对内容，
        // 捕获勒索软件伪造 mtime/size 的篡改。
        if op == Action::Link
            && let Some(prev_path) = prev_path.as_deref()
        {
            match differ::contents_match(path, prev_path, cfg.verify) {
                Ok(true) => {}
                Ok(false) => {
                    debug!(file = %path.display(), "内容复核发现变化，改为拷贝");
                    op = Action::Changed;
                }
                Err(err) => {
                    warn!(file = %path.display(), error = %err, "内容复核失败，保守按已修改处理");
                    op = Action::Changed;
                }
            }
        }
        match op {
            Action::Link => stats.linked += 1,
            Action::New => stats.new += 1,
            Action::Changed => stats.changed += 1,
        }

        // 仅在确定需要处理时提取元数据，作为处理阶段的单一数据源。
        let size = src_meta.len();
        let mtime = FileTime::from_last_modification_time(&src_meta);

        items.push(WorkItem {
            src: path.to_path_buf(),
            dest: dest_path,
            rel: rel.to_path_buf(),
            prev: prev_path,
            size,
            mtime,
            op,
        });
    }

    // 变化率门禁：仅统计「已存在文件被修改」的比例，新增不计入。
    let existing = stats.linked + stats.changed;
    if cfg.change_rate_threshold > 0.0
        && stats.changed >= cfg.min_changed_files as u64
        && existing > 0
    {
        let ratio = stats.changed as f64 / existing as f64;
        if ratio > cfg.change_rate_threshold {
            return Err(BackupError::RansomwareSuspected {
                existing,
                changed: stats.changed,
                ratio,
                threshold: cfg.change_rate_threshold,
            });
        }
    }

    // 门禁通过后分发到拷贝队列。
    for item in items {
        if tx.blocking_send(item).is_err() {
            // 接收端已关闭，停止分发。
            break;
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tokio::sync::mpsc;

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nov-nas-back-engine-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 构造源目录与上一快照目录，各含 `n` 个同名文件。
    /// `modified = true` 时源与上一快照内容长度不同（触发 `Changed` 判定）。
    fn setup_dirs(root: &Path, n: usize, modified: bool) -> (PathBuf, PathBuf) {
        let src = root.join("src");
        let prev = root.join("prev");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&prev).unwrap();
        for i in 0..n {
            let name = format!("file-{i}.txt");
            let (src_content, prev_content) = if modified {
                (
                    format!("src-data-{i}").into_bytes(),
                    format!("prev-data-longer-{i}").into_bytes(),
                )
            } else {
                (b"same-content".to_vec(), b"same-content".to_vec())
            };
            fs::write(src.join(&name), src_content).unwrap();
            fs::write(prev.join(&name), prev_content).unwrap();
        }
        (src, prev)
    }

    fn scan_cfg(threshold: f64, min_changed: usize, verify: Verify) -> ScanConfig {
        ScanConfig {
            change_rate_threshold: threshold,
            min_changed_files: min_changed,
            verify,
        }
    }

    #[test]
    fn gate_triggers_on_mass_modification() {
        let root = temp_root("gate-trigger");
        let (src, prev) = setup_dirs(&root, 10, true);
        let snap = root.join("snap");
        let (tx, _rx) = mpsc::channel(1024);
        let cfg = scan_cfg(0.3, 5, Verify::Size);
        let result = scan_source(&src, &snap, Some(&prev), &tx, &cfg);
        assert!(matches!(
            result,
            Err(BackupError::RansomwareSuspected { .. })
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gate_passes_when_below_absolute_floor() {
        let root = temp_root("gate-floor");
        let (src, prev) = setup_dirs(&root, 10, true);
        let snap = root.join("snap");
        let (tx, _rx) = mpsc::channel(1024);
        // 被修改文件数 10 < 兜底值 100，不触发门禁。
        let cfg = scan_cfg(0.3, 100, Verify::Size);
        let result = scan_source(&src, &snap, Some(&prev), &tx, &cfg);
        assert!(result.is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gate_passes_when_all_new() {
        let root = temp_root("gate-new");
        let (src, _prev) = setup_dirs(&root, 10, false);
        // 上一快照为空：全部视为新增，changed = 0，不触发门禁。
        let empty_prev = root.join("empty-prev");
        fs::create_dir_all(&empty_prev).unwrap();
        let snap = root.join("snap");
        let (tx, _rx) = mpsc::channel(1024);
        let cfg = scan_cfg(0.3, 1, Verify::Size);
        let result = scan_source(&src, &snap, Some(&empty_prev), &tx, &cfg);
        assert!(result.is_ok());
        fs::remove_dir_all(&root).ok();
    }
}
