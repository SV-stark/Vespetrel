use tracing::info;

/// Headless app coordinator - drives SyncCoordinator + storage without UI (useful for tests / offline mode)
pub struct VespetrelApp {
    pub state: crate::state::AppState,
    pub db_path: String,
}

impl VespetrelApp {
    pub fn new(db_path: impl Into<String>) -> Self {
        Self { state: crate::state::AppState::new(), db_path: db_path.into() }
    }

    pub fn db_path(&self) -> &str { &self.db_path }

    pub async fn init_storage(&self) -> anyhow::Result<()> {
        // Ensure storage can be opened
        let conn = vespetrel_storage::db::open_in_memory()?; // in headless mode use memory; real path uses create_pool
        info!("storage initialized (headless check) - {} tables ready", count_tables(&conn)?);
        Ok(())
    }
}

fn count_tables(conn: &rusqlite::Connection) -> anyhow::Result<i64> {
    Ok(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0))?)
}
