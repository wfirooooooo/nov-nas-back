//! 流式分块拷贝：限速读取 → 写入临时文件 → 原子替换 → 回写 mtime。

use std::path::{Path, PathBuf};

use filetime::FileTime;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::BackupError;
use crate::throttle::RateLimiter;

/// 分块大小（1 MiB）。
const CHUNK_SIZE: usize = 1024 * 1024;
/// 临时文件前缀，崩溃遗留的半成品文件以此识别。
const TMP_PREFIX: &str = ".novtmp-";

/// 将 `source` 流式拷贝到 `dest`，全程遵守全局限速，成功后原子替换。
///
/// `size` 与 `mtime` 来自扫描阶段（单一数据源），避免拷贝后重读源元数据
/// 带来的竞态：`mtime` 反映的是「本次实际拷贝的那份内容」的时间戳，而
/// `size` 用于校验拷贝期间源文件未发生长度变化。
///
/// 返回实际复制的字节数。流程：
/// 1. 确保目标父目录存在；
/// 2. 边限速读取边写入同目录临时文件（目标不出现半成品）；
/// 3. 校验实际拷贝字节数与 `size` 一致；
/// 4. 回写 `mtime` 并原子重命名。
pub async fn copy_file(
    source: &Path,
    dest: &Path,
    size: u64,
    mtime: FileTime,
    limiter: &RateLimiter,
) -> Result<u64, BackupError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await?;
    }

    let tmp = tmp_path(dest);
    let mut reader = fs::File::open(source).await?;
    let mut writer = fs::File::create(&tmp).await?;

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut copied: u64 = 0;

    loop {
        // 先申请与块等量的配额，再读取，从而在读取层面实现带宽限速。
        limiter.acquire(CHUNK_SIZE as u64).await;
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        copied += n as u64;
    }
    drop(writer); // 关闭句柄，确保 Windows 上可 rename

    // 校验拷贝期间源文件未被修改（长度一致性）。
    if copied != size {
        let _ = fs::remove_file(&tmp).await;
        return Err(BackupError::FileChangedDuringCopy {
            path: source.display().to_string(),
            expected: size,
            actual: copied,
        });
    }

    // 回写源文件 mtime（同步 syscall 隔离到阻塞线程池）。
    let tmp_for_mtime = tmp.clone();
    tokio::task::spawn_blocking(move || filetime::set_file_mtime(&tmp_for_mtime, mtime)).await??;

    fs::rename(&tmp, dest).await?;
    Ok(copied)
}

/// 构造与 `dest` 同目录的临时文件路径：`.novtmp-<文件名>`。
fn tmp_path(dest: &Path) -> PathBuf {
    let file_name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut tmp = dest.to_path_buf();
    tmp.set_file_name(format!("{TMP_PREFIX}{file_name}"));
    tmp
}
