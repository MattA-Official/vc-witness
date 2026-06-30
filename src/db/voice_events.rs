use chrono::{DateTime, Utc};
use serenity::model::id::{ChannelId, UserId};
use sqlx::SqlitePool;

use crate::error::Result;

#[derive(Debug, Clone, Copy)]
pub enum VoiceEventType {
    Join,
    Leave,
    Move,
    ConsentGranted,
}

impl VoiceEventType {
    fn as_str(&self) -> &'static str {
        match self {
            VoiceEventType::Join => "join",
            VoiceEventType::Leave => "leave",
            VoiceEventType::Move => "move",
            VoiceEventType::ConsentGranted => "consent_granted",
        }
    }
}

pub async fn log(pool: &SqlitePool, channel_id: ChannelId, user_id: UserId, event: VoiceEventType) -> Result<()> {
    let now = crate::db::now_iso();
    sqlx::query("INSERT INTO voice_activity_log (channel_id, user_id, event_type, at) VALUES (?, ?, ?, ?)")
        .bind(channel_id.to_string())
        .bind(user_id.to_string())
        .bind(event.as_str())
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// Latest activity timestamp for a channel, surviving process restarts -- backs the
/// `most_recent_activity` VC-selection strategy.
pub async fn last_activity(pool: &SqlitePool, channel_id: ChannelId) -> Result<Option<DateTime<Utc>>> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT at FROM voice_activity_log WHERE channel_id = ? ORDER BY at DESC LIMIT 1",
    )
    .bind(channel_id.to_string())
    .fetch_optional(pool)
    .await?
    .flatten();

    Ok(raw
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc)))
}

/// Most recent time the given user left any voice channel (used for report eligibility's
/// "recently left" check). Inferred as the latest 'leave'/'move' event for that user.
pub async fn last_left_at(pool: &SqlitePool, user_id: UserId) -> Result<Option<(ChannelId, DateTime<Utc>)>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        channel_id: String,
        at: String,
    }

    let row: Option<Row> = sqlx::query_as(
        "SELECT channel_id, at FROM voice_activity_log
         WHERE user_id = ? AND event_type = 'leave'
         ORDER BY at DESC LIMIT 1",
    )
    .bind(user_id.to_string())
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|r| {
        let cid: u64 = r.channel_id.parse().ok()?;
        let at = DateTime::parse_from_rfc3339(&r.at).ok()?.with_timezone(&Utc);
        Some((ChannelId::new(cid), at))
    }))
}
