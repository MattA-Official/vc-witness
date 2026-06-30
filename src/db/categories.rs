use serenity::model::id::GuildId;
use sqlx::SqlitePool;

use crate::error::{Result, WitnessError};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReportCategory {
    pub id: i64,
    pub guild_id: String,
    pub label: String,
    pub value: String,
    pub sort_order: i64,
    pub active: bool,
}

const DEFAULTS: &[(&str, &str)] = &[
    ("Harassment", "harassment"),
    ("Slur / hate speech", "slur"),
    ("Threats", "threats"),
    ("Sexual content / NSFW", "sexual_content"),
    ("Other", "other"),
];

/// Seeds default categories the first time a guild has none. Done in code (not a migration)
/// since migrations run before GUILD_ID is known.
pub async fn ensure_defaults(pool: &SqlitePool, guild_id: GuildId) -> Result<()> {
    let gid = guild_id.to_string();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_categories WHERE guild_id = ?")
        .bind(&gid)
        .fetch_one(pool)
        .await?;

    if count > 0 {
        return Ok(());
    }

    for (i, (label, value)) in DEFAULTS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO report_categories (guild_id, label, value, sort_order, active) VALUES (?, ?, ?, ?, 1)",
        )
        .bind(&gid)
        .bind(label)
        .bind(value)
        .bind(i as i64)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn list_active(pool: &SqlitePool, guild_id: GuildId) -> Result<Vec<ReportCategory>> {
    let gid = guild_id.to_string();
    let rows = sqlx::query_as::<_, ReportCategory>(
        "SELECT id, guild_id, label, value, sort_order, active FROM report_categories
         WHERE guild_id = ? AND active = 1 ORDER BY sort_order ASC, label ASC",
    )
    .bind(gid)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add(pool: &SqlitePool, guild_id: GuildId, label: &str, value: &str) -> Result<()> {
    let gid = guild_id.to_string();
    let next_sort: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM report_categories WHERE guild_id = ?",
    )
    .bind(&gid)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        "INSERT INTO report_categories (guild_id, label, value, sort_order, active) VALUES (?, ?, ?, ?, 1)
         ON CONFLICT(guild_id, value) DO UPDATE SET label = excluded.label, active = 1",
    )
    .bind(gid)
    .bind(label)
    .bind(value)
    .bind(next_sort)
    .execute(pool)
    .await?;
    Ok(())
}

/// Soft-delete only -- historical reports keep their `category_label_snapshot`.
pub async fn remove(pool: &SqlitePool, guild_id: GuildId, value: &str) -> Result<()> {
    let gid = guild_id.to_string();
    let affected = sqlx::query(
        "UPDATE report_categories SET active = 0 WHERE guild_id = ? AND value = ?",
    )
    .bind(gid)
    .bind(value)
    .execute(pool)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(WitnessError::Config(format!("no category with value '{value}'")));
    }
    Ok(())
}

pub async fn edit_label(pool: &SqlitePool, guild_id: GuildId, value: &str, new_label: &str) -> Result<()> {
    let gid = guild_id.to_string();
    let affected = sqlx::query(
        "UPDATE report_categories SET label = ? WHERE guild_id = ? AND value = ? AND active = 1",
    )
    .bind(new_label)
    .bind(gid)
    .bind(value)
    .execute(pool)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(WitnessError::Config(format!("no active category with value '{value}'")));
    }
    Ok(())
}
