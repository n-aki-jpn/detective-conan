use std::collections::HashMap;
use std::path::PathBuf;

use crate::models::{Session, User};

const SESSION_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000; // 7日間

pub struct UserManager {
    pub users: HashMap<String, User>,
    pub sessions: HashMap<String, Session>,
    pub users_path: PathBuf,
    pub sessions_path: PathBuf,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn rand_hex(len: usize) -> String {
    let mut s = String::with_capacity(len);
    while s.len() < len {
        let v = rand::random::<u8>();
        s.push_str(&format!("{:02x}", v));
    }
    s
}

impl UserManager {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let users_path = data_dir.join("users.json");
        let sessions_path = data_dir.join("sessions.json");
        let users = load_json(&users_path).unwrap_or_default();
        let sessions = load_json(&sessions_path).unwrap_or_default();
        Self {
            users,
            sessions,
            users_path,
            sessions_path,
        }
    }

    /// ニックネームからユーザーを作成（新規登録）。ニックネーム重複時はエラー。
    pub fn register(&mut self, nickname: &str) -> Result<(User, Session), String> {
        let nickname = nickname.trim();
        let char_len = nickname.chars().count();
        if char_len < 2 || char_len > 16 {
            return Err("ニックネームは2〜16文字で入力してください".to_string());
        }
        if self.users.values().any(|u| u.nickname == nickname) {
            return Err("そのニックネームは既に使用されています".to_string());
        }
        let user = User {
            id: rand_hex(16),
            nickname: nickname.to_string(),
            created_at: now_ms(),
        };
        let session = self.create_session(&user.id);
        self.users.insert(user.id.clone(), user.clone());
        self.save();
        Ok((user, session))
    }

    /// ニックネームでログイン。該当ユーザーが存在しなければエラー。
    pub fn login(&mut self, nickname: &str) -> Result<(User, Session), String> {
        let nickname = nickname.trim();
        let user = self
            .users
            .values()
            .find(|u| u.nickname == nickname)
            .cloned()
            .ok_or_else(|| "そのニックネームのユーザーが見つかりません".to_string())?;
        let session = self.create_session(&user.id);
        self.save();
        Ok((user, session))
    }

    fn create_session(&mut self, user_id: &str) -> Session {
        let now = now_ms();
        let session = Session {
            token: rand_hex(32),
            user_id: user_id.to_string(),
            created_at: now,
            expires_at: now + SESSION_TTL_MS,
        };
        self.sessions.insert(session.token.clone(), session.clone());
        session
    }

    /// トークンからユーザーを解決。期限切れセッションは破棄する。
    pub fn resolve_token(&mut self, token: &str) -> Option<User> {
        self.cleanup_expired();
        let session = self.sessions.get(token)?;
        self.users.get(&session.user_id).cloned()
    }

    pub fn user_by_id(&self, user_id: &str) -> Option<&User> {
        self.users.get(user_id)
    }

    pub fn cleanup_expired(&mut self) {
        let now = now_ms();
        let before = self.sessions.len();
        self.sessions.retain(|_, s| s.expires_at > now);
        if self.sessions.len() != before {
            self.save();
        }
    }

    pub fn save(&self) {
        save_json(&self.users_path, &self.users);
        save_json(&self.sessions_path, &self.sessions);
    }
}

fn load_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn save_json<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(path, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_login_flow() {
        let dir = std::env::temp_dir().join(format!("conan_user_test_{}", rand_hex(8)));
        let _ = std::fs::create_dir_all(&dir);
        let mut mgr = UserManager::new(&dir);

        let (user, session) = mgr.register("コナン").expect("register ok");
        assert_eq!(user.nickname, "コナン");
        assert!(mgr.resolve_token(&session.token).is_some());

        // 重複登録は失敗
        assert!(mgr.register("コナン").is_err());

        // ログイン
        let (user2, session2) = mgr.login("コナン").expect("login ok");
        assert_eq!(user2.id, user.id);
        assert!(mgr.resolve_token(&session2.token).is_some());

        // 存在しないニックネーム
        assert!(mgr.login("存在しない").is_err());

        // 無効なニックネーム
        assert!(mgr.register("あ").is_err());
        let long = "あ".repeat(17);
        assert!(mgr.register(&long).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("conan_user_test_{}", rand_hex(8)));
        let _ = std::fs::create_dir_all(&dir);
        let mut mgr = UserManager::new(&dir);
        let (user, session) = mgr.register("毛利").expect("register ok");

        // 再読み込み
        let mut mgr2 = UserManager::new(&dir);
        assert!(mgr2.users.get(&user.id).is_some());
        assert!(mgr2.resolve_token(&session.token).is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
