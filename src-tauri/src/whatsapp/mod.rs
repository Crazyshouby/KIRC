//! WhatsApp, as a device linked to the account (like WhatsApp Web), through the `whatsapp-rust` library.
//!
//! This is an unofficial client: WhatsApp's terms do not allow it and an account that uses one may be suspended. It is
//! therefore only part of the builds made with the `whatsapp` feature, and the address book says so before linking.
//!
//! The session (keys of the account!) lives in `<data>/whatsapp/<id>.db`; the names of the chats in `<id>-chats.json`.

pub mod chats;
#[cfg(feature = "whatsapp")]
mod engine;

use crate::chat::{Start, Task};
use std::path::Path;

pub fn launch(start: Start) -> Result<Task, String> {
    #[cfg(feature = "whatsapp")]
    {
        engine::launch(start)
    }
    #[cfg(not(feature = "whatsapp"))]
    {
        let _ = start;
        Err("WhatsApp n'est pas inclus dans cette version de Messages".into())
    }
}

/// The name of an account's files: its identifier, if it is made of letters, digits, '-' and '_' only.
fn stem(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(id)
}

/// The session files of an account of this application.
pub fn files(dir: &Path, id: &str) -> Vec<std::path::PathBuf> {
    let Some(stem) = stem(id) else { return Vec::new() };
    let base = dir.join("whatsapp");
    [".db", ".db-wal", ".db-shm", "-chats.json", "-chats.tmp"].iter().map(|ext| base.join(format!("{stem}{ext}"))).collect()
}

/// Deletes the session of an account that is removed from the address book. The linked device stays listed on the
/// phone until it is removed there (or `/logout` was typed before).
pub fn forget_files(dir: &Path, id: &str) {
    for f in files(dir, id) {
        let _ = std::fs::remove_file(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identifier_cannot_point_outside_the_session_folder() {
        let dir = Path::new("data");
        assert!(files(dir, "../secret").is_empty());
        assert!(files(dir, "a/b").is_empty());
        assert!(files(dir, "").is_empty());
        let f = files(dir, "s1");
        assert_eq!(f.len(), 5);
        assert!(f.iter().all(|p| p.starts_with(dir.join("whatsapp"))));
    }

    #[test]
    fn forgetting_an_account_removes_its_files() {
        let dir = std::env::temp_dir().join(format!("messages-wa-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("whatsapp")).unwrap();
        for f in files(&dir, "s9") {
            std::fs::write(f, "x").unwrap();
        }
        std::fs::write(dir.join("whatsapp").join("autre.db"), "x").unwrap();
        forget_files(&dir, "s9");
        let left: Vec<String> = std::fs::read_dir(dir.join("whatsapp")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec!["autre.db"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
