use std::path::Path;
use tracing::info;

/// App coordinator - drives SyncCoordinator + storage with or without UI
pub struct VespetrelApp {
    pub state: crate::state::AppState,
    pub db_path: String,
}

impl VespetrelApp {
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            state: crate::state::AppState::new(),
            db_path: db_path.into(),
        }
    }

    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    pub async fn init_storage(&self) -> anyhow::Result<()> {
        if self.db_path == ":memory:" || self.db_path.is_empty() {
            let conn = vespetrel_storage::db::open_in_memory()?;
            info!(
                "storage initialized (in-memory) - {} tables ready",
                count_tables(&conn)?
            );
        } else {
            // Ensure parent directory exists before opening database file
            if let Some(parent) = Path::new(&self.db_path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)?;
            }
            let conn = rusqlite::Connection::open(&self.db_path)?;

            vespetrel_storage::db::init_connection(&conn)?;
            info!(
                "storage initialized ({}) - {} tables ready",
                self.db_path,
                count_tables(&conn)?
            );
        }
        Ok(())
    }

    pub fn create_storage_pool(&self) -> anyhow::Result<vespetrel_storage::db::StoragePool> {
        vespetrel_storage::db::create_pool(&self.db_path)
    }
}

fn count_tables(conn: &rusqlite::Connection) -> anyhow::Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table'",
        [],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_init_in_memory() {
        let app = VespetrelApp::new(":memory:");
        assert!(app.init_storage().await.is_ok());
    }

    #[tokio::test]
    async fn test_app_init_temp_file() {
        let temp_dir =
            std::env::temp_dir().join(format!("vespetrel_test_{}", uuid::Uuid::new_v4()));
        let db_path = temp_dir.join("test.db");
        let app = VespetrelApp::new(db_path.to_str().unwrap());
        assert!(app.init_storage().await.is_ok());
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
