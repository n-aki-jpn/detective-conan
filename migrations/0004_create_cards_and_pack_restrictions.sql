-- カード収録弾情報および未発売弾・PR制限設定テーブル
CREATE TABLE IF NOT EXISTS cards (
  definition_id TEXT PRIMARY KEY,
  canonical_id TEXT NOT NULL,
  title TEXT NOT NULL,
  card_type TEXT NOT NULL,
  package_name TEXT NOT NULL DEFAULT '',
  pack_code TEXT NOT NULL DEFAULT '',
  release_date TEXT,
  is_pr INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_cards_pack_code ON cards(pack_code);

-- 未発売の弾やPRカード全体の制限設定（is_restricted = 1 でデッキ選択から非表示）
CREATE TABLE IF NOT EXISTS pack_restrictions (
  pack_code TEXT PRIMARY KEY,
  package_name TEXT NOT NULL DEFAULT '',
  is_restricted INTEGER NOT NULL DEFAULT 0,
  reason TEXT,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 個別カード（特定のPRカード等）の制限設定
CREATE TABLE IF NOT EXISTS card_restrictions (
  definition_id TEXT PRIMARY KEY,
  is_restricted INTEGER NOT NULL DEFAULT 0,
  reason TEXT,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
