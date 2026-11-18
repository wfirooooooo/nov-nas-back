//! 文件变化判断：基于大小与修改时间（mtime）的元数据比对，以及
//! 可选的内容级复核（采样/全量哈希）捕获伪造 mtime 的篡改。
//!
//! 快照式备份中，比对基准是「上一快照」而非「目标顶层」：
//! 未变文件通过硬链接复用上一快照，变化/缺失文件才真正拷贝。

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

use crate::config::Verify;

/// mtime 容差（秒）：覆盖跨文件系统（SMB/FAT/NTFS/ext4）的时间戳精度差异。
const MTIME_TOLERANCE_SECS: u64 = 2;

/// 采样哈希的头部/尾部字节数。
const SAMPLE_BYTES: u64 = 4096;
/// 全量哈希的分块读取大小（1 MiB）。
const FULL_CHUNK_SIZE: usize = 1024 * 1024;

/// 比对结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 无需复制：与上一快照大小、mtime 一致，硬链接复用即可。
    Link,
    /// 新增文件（上一快照不存在），需要复制。
    New,
    /// 已存在文件被修改（大小或 mtime 变化），需要复制。
    ///
    /// 与 [`Action::New`] 区分开：变化率门禁只统计「已存在文件被修改」的
    /// 比例，因为批量新增（如新导出照片）不是勒索信号，而批量篡改才是。
    Changed,
}

/// 判断源文件与上一快照的差异，决定硬链接、新增拷贝或修改拷贝。
///
/// 判断顺序：上一快照不存在 → 新增；大小不同 → 修改；mtime 差超过容差 → 修改；
/// 否则硬链接。任一元数据读取失败时保守返回 [`Action::Changed`]，确保不漏备份。
pub fn decide(src: &fs::Metadata, prev: Option<&fs::Metadata>) -> Action {
    let Some(prev) = prev else {
        return Action::New;
    };
    if src.len() != prev.len() {
        return Action::Changed;
    }
    if mtimes_differ(src.modified(), prev.modified()) {
        return Action::Changed;
    }
    Action::Link
}

fn mtimes_differ(a: std::io::Result<SystemTime>, b: std::io::Result<SystemTime>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => {
            let diff = match a.duration_since(b) {
                Ok(d) => d,
                Err(_) => match b.duration_since(a) {
                    Ok(d) => d,
                    Err(_) => return true,
                },
            };
            diff.as_secs() > MTIME_TOLERANCE_SECS
        }
        _ => true,
    }
}

/// 内容级复核：比较源文件与上一快照文件的内容是否一致。
///
/// 调用方仅在元数据判定为 [`Action::Link`] 时使用本函数，以捕获
/// 「保留 mtime/size 但内容已被篡改」的伪装。返回 `true` 表示内容一致
/// （维持硬链接），`false` 表示内容变化（应改为拷贝）。
///
/// [`Verify::Size`] 不做任何内容读取，恒返回 `true`（元数据已由 [`decide`] 判定一致）。
pub fn contents_match(src: &Path, prev: &Path, verify: Verify) -> std::io::Result<bool> {
    match verify {
        Verify::Size => Ok(true),
        Verify::Sample => Ok(sample_hash(src)? == sample_hash(prev)?),
        Verify::Full => Ok(full_hash(src)? == full_hash(prev)?),
    }
}

/// 计算文件采样哈希（头 `SAMPLE_BYTES` + 尾 `SAMPLE_BYTES`，小文件则全量）。
///
/// 对勒索加密（整文件字节变化）而言，头尾采样足以检出；相比全量哈希，
/// 大幅降低大文件的 I/O 成本。
fn sample_hash(path: &Path) -> std::io::Result<blake3::Hash> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();

    // 小文件直接全量哈希。
    if len <= 2 * SAMPLE_BYTES {
        let mut content = Vec::with_capacity(len as usize);
        file.read_to_end(&mut content)?;
        return Ok(blake3::hash(&content));
    }

    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; SAMPLE_BYTES as usize];
    file.read_exact(&mut buf)?;
    hasher.update(&buf);
    file.seek(SeekFrom::End(-(SAMPLE_BYTES as i64)))?;
    file.read_exact(&mut buf)?;
    hasher.update(&buf);
    Ok(hasher.finalize())
}

/// 计算文件全量 BLAKE3 哈希（分块读取，避免一次性加载大文件）。
///
/// `pub(crate)`：供完整性清单（manifest）的 `verify` 校验复用。
pub(crate) fn full_hash(path: &Path) -> std::io::Result<blake3::Hash> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; FULL_CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nov-nas-back-test-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_prev_means_new() {
        let dir = temp_dir("missing");
        let src = dir.join("src.txt");
        fs::write(&src, b"hello").unwrap();
        let meta = fs::metadata(&src).unwrap();
        assert_eq!(decide(&meta, None), Action::New);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_size_and_mtime_links() {
        let dir = temp_dir("same");
        let src = dir.join("a.txt");
        let dst = dir.join("b.txt");
        fs::write(&src, b"hello").unwrap();
        fs::write(&dst, b"hello").unwrap();
        let mtime = SystemTime::now();
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(mtime)).unwrap();
        filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(mtime)).unwrap();
        let sm = fs::metadata(&src).unwrap();
        let dm = fs::metadata(&dst).unwrap();
        assert_eq!(decide(&sm, Some(&dm)), Action::Link);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn size_mismatch_changes() {
        let dir = temp_dir("size");
        let src = dir.join("a.txt");
        let dst = dir.join("b.txt");
        fs::write(&src, b"hello world").unwrap();
        fs::write(&dst, b"hello").unwrap();
        let sm = fs::metadata(&src).unwrap();
        let dm = fs::metadata(&dst).unwrap();
        assert_eq!(decide(&sm, Some(&dm)), Action::Changed);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mtime_difference_changes() {
        let dir = temp_dir("mtime");
        let src = dir.join("a.txt");
        let dst = dir.join("b.txt");
        fs::write(&src, b"hello").unwrap();
        fs::write(&dst, b"hello").unwrap();
        let now = SystemTime::now();
        let later = now + Duration::from_secs(10);
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(later)).unwrap();
        filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(now)).unwrap();
        let sm = fs::metadata(&src).unwrap();
        let dm = fs::metadata(&dst).unwrap();
        assert_eq!(decide(&sm, Some(&dm)), Action::Changed);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mtime_within_tolerance_links() {
        let dir = temp_dir("tol");
        let src = dir.join("a.txt");
        let dst = dir.join("b.txt");
        fs::write(&src, b"hello").unwrap();
        fs::write(&dst, b"hello").unwrap();
        let now = SystemTime::now();
        let later = now + Duration::from_secs(1);
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(later)).unwrap();
        filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(now)).unwrap();
        let sm = fs::metadata(&src).unwrap();
        let dm = fs::metadata(&dst).unwrap();
        assert_eq!(decide(&sm, Some(&dm)), Action::Link);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contents_match_size_always_true() {
        // Size 级别不做内容读取，即使内容不同也恒为 true。
        let dir = temp_dir("verify-size");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        fs::write(&a, b"hello").unwrap();
        fs::write(&b, b"world").unwrap();
        assert!(contents_match(&a, &b, Verify::Size).unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contents_match_sample_detects_difference() {
        let dir = temp_dir("verify-sample-diff");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        fs::write(&a, b"hello").unwrap();
        fs::write(&b, b"world").unwrap();
        assert!(!contents_match(&a, &b, Verify::Sample).unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contents_match_sample_same_content() {
        let dir = temp_dir("verify-sample-same");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        fs::write(&a, b"same-content").unwrap();
        fs::write(&b, b"same-content").unwrap();
        assert!(contents_match(&a, &b, Verify::Sample).unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contents_match_full_detects_middle_difference() {
        // 头尾相同、仅中间字节不同：Sample 会漏检，Full 能检出。
        let dir = temp_dir("verify-full");
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        let data_a = vec![0u8; 10_000];
        let mut data_b = data_a.clone();
        data_b[5_000] = 1;
        fs::write(&a, &data_a).unwrap();
        fs::write(&b, &data_b).unwrap();
        assert!(!contents_match(&a, &b, Verify::Full).unwrap());
        fs::remove_dir_all(&dir).ok();
    }
}
