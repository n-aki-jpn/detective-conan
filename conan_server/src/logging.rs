// ─── Log Rotation ────────────────────────────────────────────────────────────
//
// logs/all_rooms.log と logs/matches.jsonl の肥大化を防ぐための
// サイズベースログローテーションを提供する。
//
// 動作:
//   - ファイルサイズが上限（デフォルト 5MB）を超えたタイミングで、
//     現在のログを `<file>.1` にリネームし、以降の追記は新しいファイルへ向ける。
//   - 既存の `<file>.1` は `<file>.2` へ…と世代を繰り下げ、
//     保持数（デフォルト 5 世代）を超える最も古いファイルは削除する。
//
// 環境変数で調整可能:
//   - LOG_MAX_BYTES: 1ファイルあたりの上限バイト数（デフォルト 5242880 = 5MB）
//   - LOG_KEEP:     ローテーションで保持するバックアップ世代数（デフォルト 5）

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// 1ファイルあたりの上限バイト数（環境変数 LOG_MAX_BYTES で上書き可）
pub fn max_bytes() -> u64 {
    std::env::var("LOG_MAX_BYTES")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(5 * 1024 * 1024)
}

/// 保持するバックアップ世代数（環境変数 LOG_KEEP で上書き可）
pub fn keep_count() -> usize {
    std::env::var("LOG_KEEP")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(5)
}

/// `<path>` が上限を超えていればローテーションを行う。
/// 成功した場合 true を返す（実際にローテーションが発生した場合のみ）。
/// 上限・保持数を明示的に指定してローテーションを行う。
pub fn rotate_if_needed(path: &Path, max_b: u64, keep: usize) -> bool {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.len() < max_b {
        return false;
    }

    // 最も古い世代から削除・繰り下げを行う（.N -> 削除、.N-1 -> .N ... .1 -> .2）
    for idx in (1..=keep).rev() {
        let src = backup_path(path, idx);
        if !src.exists() {
            continue;
        }
        if idx == keep {
            let _ = fs::remove_file(&src);
        } else {
            let dst = backup_path(path, idx + 1);
            let _ = fs::rename(&src, &dst);
        }
    }

    // 現在のログを .1 へ
    let _ = fs::rename(path, backup_path(path, 1));
    true
}

/// ログファイルへ1行追記する（親ディレクトリ作成 + 上限超過時のローテーション込み）。
/// 書き込み失敗時はサイレントに無視する（既存の挙動に合わせる）。
pub fn append_line(path: &Path, line: &str) {
    append_line_with(path, line, max_bytes(), keep_count());
}

/// 明示的に上限・保持数を指定して1行追記する（テスト用にも使用）。
pub fn append_line_with(path: &Path, line: &str, max_b: u64, keep: usize) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    rotate_if_needed(path, max_b, keep);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", line);
        let _ = f.flush();
    }
}

fn backup_path(path: &Path, idx: usize) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "log".to_string());
    name.push('.');
    name.push_str(&idx.to_string());
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_log_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "conan_log_rotate_{}_{}",
            tag,
            rand_suffix()
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn rand_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn rotates_when_exceeding_limit() {
        let dir = temp_log_dir("rotate");
        let path = dir.join("all_rooms.log");

        // 100バイト超のファイルを作る
        let big = "x".repeat(120);
        fs::write(&path, &big).unwrap();

        append_line_with(&path, "new line", 100, 2);

        // 旧ログは .1 に退避されていること
        let bak = backup_path(&path, 1);
        assert!(bak.exists(), "旧ログが .1 に退避されているべき");
        // 新しい本体は退避後の短い内容であること
        let cur = fs::read_to_string(&path).unwrap();
        assert_eq!(cur.trim(), "new line");

        // 上限超過を繰り返して世代繰り下げと削除を確認
        for _ in 0..5 {
            fs::write(&path, &big).unwrap();
            append_line_with(&path, "x", 100, 2);
        }
        // keep=2 なので .3 は存在してはならない
        assert!(!backup_path(&path, 3).exists());
        assert!(backup_path(&path, 1).exists());
        assert!(backup_path(&path, 2).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_rotation_below_limit() {
        let dir = temp_log_dir("norotate");
        let path = dir.join("matches.jsonl");

        append_line_with(&path, "line1", 1_000_000, 5);
        append_line_with(&path, "line2", 1_000_000, 5);

        assert!(!backup_path(&path, 1).exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("line1") && content.contains("line2"));

        let _ = fs::remove_dir_all(&dir);
    }
}
