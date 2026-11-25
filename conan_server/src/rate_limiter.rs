// ─── Rate Limiter ────────────────────────────────────────────────────────────
//
// 認証系エンドポイント（/api/register, /api/login など）に対する
// 総当たり・暴走リクエストを緩和するための、固定ウィンドウ式レートリミッタ。
//
// キー（通常はクライアントIP）ごとに「ウィンドウ開始時刻」と「件数」を持ち、
// ウィンドウ内で上限を超えたら拒否する。期限切れエントリは掃除する。
//
// 環境変数で調整可能:
//   - RATE_LIMIT_MAX:   ウィンドウ内の上限リクエスト数（デフォルト 20）
//   - RATE_LIMIT_WINDOW: ウィンドウ長（秒）（デフォルト 60）

use std::collections::HashMap;

#[derive(Clone, Copy)]
struct Bucket {
    window_start_ms: i64,
    count: u32,
}

pub struct RateLimiter {
    buckets: HashMap<String, Bucket>,
}

/// ウィンドウ内の上限リクエスト数（環境変数 RATE_LIMIT_MAX で上書き可）
pub fn max_requests() -> u32 {
    std::env::var("RATE_LIMIT_MAX")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(20)
}

/// ウィンドウ長（秒）（環境変数 RATE_LIMIT_WINDOW で上書き可）
pub fn window_secs() -> i64 {
    std::env::var("RATE_LIMIT_WINDOW")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(60)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// `key` に対する1リクエストを記録し、許可するなら true を返す。
    /// 上限超過なら false（この場合カウントは増やさない）。
    /// 明示的に上限・ウィンドウを指定して判定する（テスト用にも使用）。
    pub fn check_with(&mut self, key: &str, max: u32, window_s: i64, now: i64) -> bool {
        let window_ms = window_s * 1000;
        let bucket = self.buckets.entry(key.to_string()).or_insert(Bucket {
            window_start_ms: now,
            count: 0,
        });

        // ウィンドウ期限切れならリセット
        if now - bucket.window_start_ms >= window_ms {
            bucket.window_start_ms = now;
            bucket.count = 0;
        }

        if bucket.count >= max {
            return false;
        }
        bucket.count += 1;
        true
    }

    /// 環境変数の設定で判定する。
    pub fn check(&mut self, key: &str) -> bool {
        self.check_with(key, max_requests(), window_secs(), now_ms())
    }

    /// 期限切れエントリを掃除する。
    pub fn cleanup(&mut self, window_s: i64, now: i64) {
        let window_ms = window_s * 1000;
        self.buckets
            .retain(|_, b| now - b.window_start_ms < window_ms);
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_limit_then_blocks() {
        let mut rl = RateLimiter::new();
        let base = 1_000_000i64;
        // 上限3件まで許可
        for i in 0..3 {
            assert!(rl.check_with("1.2.3.4", 3, 60, base + i), "{}件目は許可", i + 1);
        }
        // 4件目は拒否
        assert!(!rl.check_with("1.2.3.4", 3, 60, base + 3), "上限超過は拒否");
        // 別IPは影響を受けない
        assert!(rl.check_with("5.6.7.8", 3, 60, base + 3), "別IPは許可");
    }

    #[test]
    fn resets_after_window() {
        let mut rl = RateLimiter::new();
        let base = 2_000_000i64;
        for i in 0..2 {
            assert!(rl.check_with("ip", 2, 60, base + i));
        }
        assert!(!rl.check_with("ip", 2, 60, base + 2));
        // ウィンドウ経過後（61秒後）にリセットされて許可される
        assert!(rl.check_with("ip", 2, 60, base + 61_000), "ウィンドウ経過後は許可");
    }

    #[test]
    fn cleanup_removes_expired() {
        let mut rl = RateLimiter::new();
        let base = 3_000_000i64;
        rl.check_with("old", 10, 10, base);
        rl.check_with("fresh", 10, 10, base + 15_000);
        // base+20秒時点では "old"(経過20秒)は期限切れ、"fresh"(経過5秒)は生存
        rl.cleanup(10, base + 20_000);
        assert!(!rl.buckets.contains_key("old"));
        assert!(rl.buckets.contains_key("fresh"));
    }
}
