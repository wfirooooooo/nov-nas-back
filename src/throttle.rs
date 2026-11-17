//! 全局令牌桶限速器。
//!
//! 所有备份任务共享同一个 [`RateLimiter`]，在每次分块读取源文件前
//! 通过 [`RateLimiter::acquire`] 申请与块大小等量的字节配额，
//! 从而在读取层面对网络带宽进行限速。零额外依赖、无忙等。

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// 令牌桶内部状态。
struct State {
    /// 当前可用令牌数（单位：字节）。
    tokens: f64,
    /// 上次补充令牌的时间。
    last: Instant,
}

/// 线程安全、可克隆的全局限速器。
#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<State>>,
    /// 令牌补充速率（字节/秒）。
    rate: f64,
    /// 令牌桶容量上限（字节），即允许的最大突发。
    burst: f64,
}

impl RateLimiter {
    /// 以指定速率创建限速器。`bytes_per_sec` 必须大于 0。
    pub fn new(bytes_per_sec: f64) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                tokens: bytes_per_sec,
                last: Instant::now(),
            })),
            rate: bytes_per_sec,
            burst: bytes_per_sec,
        }
    }

    /// 创建不限速的限速器（`acquire` 立即返回）。
    pub fn unlimited() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                tokens: f64::MAX,
                last: Instant::now(),
            })),
            rate: 0.0,
            burst: 0.0,
        }
    }

    /// 当前速率（字节/秒），0 表示不限速。
    pub fn bytes_per_sec(&self) -> f64 {
        self.rate
    }

    /// 申请 `n` 字节配额，必要时挂起等待直至配额充足。
    ///
    /// 该实现支持单次申请超过桶容量（`burst`）的情况：
    /// 每次耗尽可用令牌后，仅等待「一个桶容量」的补充时间，循环直至满足。
    pub async fn acquire(&self, n: u64) {
        if n == 0 || self.rate <= 0.0 {
            return;
        }
        let mut need = n as f64;
        loop {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            let elapsed = now.duration_since(state.last).as_secs_f64();
            state.last = now;
            // 补充令牌；正向累积以 burst 为上限，避免长时间空闲囤积。
            state.tokens = (state.tokens + elapsed * self.rate).min(self.burst);

            if state.tokens >= need {
                state.tokens -= need;
                return;
            }
            // 耗尽当前可用令牌，计算剩余缺口。
            need -= state.tokens;
            state.tokens = 0.0;
            drop(state);

            // 每次最多等待补充一个桶容量的时间，避免大请求下的限速失真。
            let wait = need.min(self.burst) / self.rate;
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unlimited_returns_immediately() {
        let limiter = RateLimiter::unlimited();
        let start = Instant::now();
        limiter.acquire(1024 * 1024 * 1024).await; // 1 GiB
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn rate_limited_acquire_takes_time() {
        // 10 KB/s，桶容量 10 KB；申请 20 KB 约需 1s（前 10 KB 瞬时）。
        let limiter = RateLimiter::new(10_000.0);
        let start = Instant::now();
        limiter.acquire(20_000).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(800), "elapsed={elapsed:?}");
        assert!(elapsed < Duration::from_secs(3), "elapsed={elapsed:?}");
    }

    #[tokio::test]
    async fn acquire_larger_than_burst_terminates() {
        // 桶容量 10 KB，单次申请 100 KB，确保不会死循环。
        let limiter = RateLimiter::new(10_000.0);
        let start = Instant::now();
        limiter.acquire(100_000).await;
        let elapsed = start.elapsed();
        // 理论上 (100KB - 10KB) / 10KB/s = 9s，允许一定误差。
        assert!(elapsed >= Duration::from_secs(8), "elapsed={elapsed:?}");
        assert!(elapsed < Duration::from_secs(11), "elapsed={elapsed:?}");
    }

    #[tokio::test]
    async fn concurrent_acquires_share_bandwidth() {
        // 10 KB/s；两个任务各申请 10 KB，合计约需 1s。
        let limiter = Arc::new(RateLimiter::new(10_000.0));
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let limiter = Arc::clone(&limiter);
            handles.push(tokio::spawn(async move {
                limiter.acquire(10_000).await;
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(800), "elapsed={elapsed:?}");
        assert!(elapsed < Duration::from_secs(3), "elapsed={elapsed:?}");
    }
}
