use chrono::{DateTime, Utc};
use serenity::model::id::UserId;
use sqlx::SqlitePool;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentState {
    /// No row at all -- the user has never been through the consent flow (or it was reset
    /// after a decline, which is deliberately not stored as a terminal state).
    Unknown,
    Pending,
    Granted,
}

#[derive(sqlx::FromRow)]
struct Row {
    state: String,
}

pub async fn get_state(pool: &SqlitePool, user_id: UserId) -> Result<ConsentState> {
    let uid = user_id.to_string();
    let row: Option<Row> = sqlx::query_as("SELECT state FROM user_consent WHERE user_id = ?")
        .bind(uid)
        .fetch_optional(pool)
        .await?;

    Ok(match row.as_ref().map(|r| r.state.as_str()) {
        Some("granted") => ConsentState::Granted,
        Some("pending") => ConsentState::Pending,
        _ => ConsentState::Unknown,
    })
}

pub async fn mark_pending(pool: &SqlitePool, user_id: UserId) -> Result<()> {
    let uid = user_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query(
        "INSERT INTO user_consent (user_id, state, updated_at) VALUES (?, 'pending', ?)
         ON CONFLICT(user_id) DO UPDATE SET state = 'pending', updated_at = excluded.updated_at
         WHERE user_consent.state != 'granted'",
    )
    .bind(uid)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_granted(pool: &SqlitePool, user_id: UserId) -> Result<()> {
    let uid = user_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query(
        "INSERT INTO user_consent (user_id, state, granted_at, updated_at) VALUES (?, 'granted', ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET state = 'granted', granted_at = excluded.granted_at, updated_at = excluded.updated_at",
    )
    .bind(uid)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Decline is transient: delete the row entirely so the next join restarts the whole flow.
pub async fn clear(pool: &SqlitePool, user_id: UserId) -> Result<()> {
    let uid = user_id.to_string();
    sqlx::query("DELETE FROM user_consent WHERE user_id = ?")
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rate-limits the "you can opt out anytime" reminder to once per join, not once per event.
pub async fn should_send_reminder(pool: &SqlitePool, user_id: UserId, min_gap: chrono::Duration) -> Result<bool> {
    let uid = user_id.to_string();
    let last: Option<String> = sqlx::query_scalar("SELECT last_reminder_at FROM user_consent WHERE user_id = ?")
        .bind(uid)
        .fetch_optional(pool)
        .await?
        .flatten();

    Ok(match last.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()) {
        Some(last_at) => Utc::now() - last_at.with_timezone(&Utc) >= min_gap,
        None => true,
    })
}

pub async fn record_reminder_sent(pool: &SqlitePool, user_id: UserId) -> Result<()> {
    let uid = user_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query("UPDATE user_consent SET last_reminder_at = ? WHERE user_id = ?")
        .bind(now)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(())
}
