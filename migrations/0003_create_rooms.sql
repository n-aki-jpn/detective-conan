-- 対戦ルーム状態永続化用テーブル
CREATE TABLE IF NOT EXISTS rooms (
  id TEXT PRIMARY KEY,
  state_json TEXT NOT NULL,
  p2_joined INTEGER NOT NULL DEFAULT 0,
  is_cpu INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
