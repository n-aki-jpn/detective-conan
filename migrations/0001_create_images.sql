-- JPG画像のメタデータおよびR2キー・公開URLを管理するテーブル
CREATE TABLE IF NOT EXISTS images (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  object_key TEXT NOT NULL UNIQUE,
  public_url TEXT NOT NULL,
  card_id TEXT,
  title TEXT,
  content_type TEXT NOT NULL DEFAULT 'image/jpeg',
  size_bytes INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_images_card_id ON images(card_id);
