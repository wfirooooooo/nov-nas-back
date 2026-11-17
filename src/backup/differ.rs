//! 文件变化判断：基于大小与修改时间（mtime）的元数据比对。

use std::fs;
use std::time::SystemTime;

/// mtime 容差（秒）：覆盖跨文件系统（SMB/FAT/NTFS/ext4）的时间戳精度差异。
const MTIME_TOLERANCE_SECS: u64 = 2;

/// 比对结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 需要复制（目标缺失或内容可能变化）。
    Copy,
    /// 无需复制（大小与 mtime 均一致）。
    Skip,
}

/// 判断源文件是否需要复制到目标。
///
/// 判断顺序：目标不存在 → 复制；大小不同 → 复制；mtime 差超过容差 → 复制；
/// 否则跳过。任一元数据读取失败时保守返回 [`Action::Copy`]，确保不漏备份。
pub fn decide(src: &fs::Metadata, dst: Option<&fs::Metadata>) -> Action {
    let Some(dst) = dst else {
        return Action::Copy;
    };
    if src.len() != dst.len() {
        return Action::Copy;
    }
    if mtimes_differ(src.modified(), dst.modified()) {
        return Action::Copy;
    }
    Action::Skip
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
    fn missing_dest_means_copy() {
        let dir = temp_dir("missing");
        let src = dir.join("src.txt");
        fs::write(&src, b"hello").unwrap();
        let meta = fs::metadata(&src).unwrap();
        assert_eq!(decide(&meta, None), Action::Copy);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_size_and_mtime_skips() {
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
        assert_eq!(decide(&sm, Some(&dm)), Action::Skip);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn size_mismatch_copies() {
        let dir = temp_dir("size");
        let src = dir.join("a.txt");
        let dst = dir.join("b.txt");
        fs::write(&src, b"hello world").unwrap();
        fs::write(&dst, b"hello").unwrap();
        let sm = fs::metadata(&src).unwrap();
        let dm = fs::metadata(&dst).unwrap();
        assert_eq!(decide(&sm, Some(&dm)), Action::Copy);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mtime_difference_copies() {
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
        assert_eq!(decide(&sm, Some(&dm)), Action::Copy);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mtime_within_tolerance_skips() {
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
        assert_eq!(decide(&sm, Some(&dm)), Action::Skip);
        fs::remove_dir_all(&dir).ok();
    }
}
