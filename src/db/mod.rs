pub mod categories;
pub mod consent;
pub mod decisions;
pub mod guild_config;
pub mod reports;
pub mod voice_events;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::Result;

pub async fn connect(database_path: &std::path::Path) -> Result<SqlitePool> {
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Helper for the many places we store Discord snowflakes as TEXT.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}
