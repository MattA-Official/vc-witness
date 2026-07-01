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
    pub description: Option<String>,
}

const DEFAULTS: &[(&str, &str, &str)] = &[
    ("Harassment", "harassment", "Repeated unwanted contact, insults, or intimidation."),
    ("Slur / hate speech", "slur", "Slurs or hate speech targeting a protected group."),
    ("Threats", "threats", "Threats of harm, violence, or doxxing."),
    ("Sexual content / NSFW", "sexual_content", "Unwanted sexual content or comments."),
    ("Other", "other", "Doesn't fit the categories above."),
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

    for (i, (label, value, description)) in DEFAULTS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO report_categories (guild_id, label, value, sort_order, active, description) VALUES (?, ?, ?, ?, 1, ?)",
        )
        .bind(&gid)
        .bind(label)
        .bind(value)
        .bind(i as i64)
        .bind(description)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn list_active(pool: &SqlitePool, guild_id: GuildId) -> Result<Vec<ReportCategory>> {
    let gid = guild_id.to_string();
    let rows = sqlx::query_as::<_, ReportCategory>(
        "SELECT id, guild_id, label, value, sort_order, active, description FROM report_categories
         WHERE guild_id = ? AND active = 1 ORDER BY sort_order ASC, label ASC",
    )
    .bind(gid)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// All categories for the guild regardless of active state -- used to check stable-identifier
/// collisions, since the `(guild_id, value)` uniqueness constraint applies to soft-deleted rows too.
pub async fn list_all(pool: &SqlitePool, guild_id: GuildId) -> Result<Vec<ReportCategory>> {
    let gid = guild_id.to_string();
    let rows = sqlx::query_as::<_, ReportCategory>(
        "SELECT id, guild_id, label, value, sort_order, active, description FROM report_categories
         WHERE guild_id = ? ORDER BY sort_order ASC, label ASC",
    )
    .bind(gid)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add(pool: &SqlitePool, guild_id: GuildId, label: &str, value: &str, description: Option<&str>) -> Result<()> {
    let gid = guild_id.to_string();
    let next_sort: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM report_categories WHERE guild_id = ?",
    )
    .bind(&gid)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        "INSERT INTO report_categories (guild_id, label, value, sort_order, active, description) VALUES (?, ?, ?, ?, 1, ?)
         ON CONFLICT(guild_id, value) DO UPDATE SET label = excluded.label, active = 1, description = excluded.description",
    )
    .bind(gid)
    .bind(label)
    .bind(value)
    .bind(next_sort)
    .bind(description)
    .execute(pool)
    .await?;
    Ok(())
}

/// Turns a display label into a stable identifier: lowercase, non-alphanumeric runs collapsed
/// to underscores. Falls back to "category" if the label has no alphanumeric characters at all.
fn slugify(label: &str) -> String {
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_");

    if slug.is_empty() {
        "category".to_string()
    } else {
        slug
    }
}

/// Adds a category, generating its stable identifier from the label instead of requiring one
/// from the caller. Re-adding a label that already exists (active or not) re-enables that same
/// category rather than creating a duplicate; otherwise a fresh slug is generated, numbering
/// past any collision with an existing, differently-labeled category.
pub async fn add_with_generated_value(
    pool: &SqlitePool,
    guild_id: GuildId,
    label: &str,
    description: Option<&str>,
) -> Result<String> {
    let existing = list_all(pool, guild_id).await?;

    if let Some(existing_match) = existing.iter().find(|c| c.label.eq_ignore_ascii_case(label)) {
        let value = existing_match.value.clone();
        add(pool, guild_id, label, &value, description).await?;
        return Ok(value);
    }

    let base = slugify(label);
    let mut candidate = base.clone();
    let mut n = 2;
    while existing.iter().any(|c| c.value == candidate) {
        candidate = format!("{base}_{n}");
        n += 1;
    }

    add(pool, guild_id, label, &candidate, description).await?;
    Ok(candidate)
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

/// Renames a category's label and, if provided, replaces its description (omitting it leaves
/// the existing description untouched).
pub async fn edit_label(pool: &SqlitePool, guild_id: GuildId, value: &str, new_label: &str, new_description: Option<&str>) -> Result<()> {
    let gid = guild_id.to_string();
    let affected = sqlx::query(
        "UPDATE report_categories SET label = ?, description = COALESCE(?, description) WHERE guild_id = ? AND value = ? AND active = 1",
    )
    .bind(new_label)
    .bind(new_description)
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
