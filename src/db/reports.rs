use serenity::model::id::{ChannelId, MessageId, UserId};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::transcription::types::Transcript;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    Pending,
    ActionTaken,
    NoActionTaken,
    Dismissed,
}

impl ReportStatus {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            ReportStatus::Pending => "pending",
            ReportStatus::ActionTaken => "action_taken",
            ReportStatus::NoActionTaken => "no_action_taken",
            ReportStatus::Dismissed => "dismissed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticipantRole {
    Reporter,
    Reported,
    BystanderRecorded,
}

impl ParticipantRole {
    fn as_db_str(&self) -> &'static str {
        match self {
            ParticipantRole::Reporter => "reporter",
            ParticipantRole::Reported => "reported",
            ParticipantRole::BystanderRecorded => "bystander_recorded",
        }
    }
}

pub struct NewReport {
    pub id: String,
    pub reporter_id: UserId,
    pub reported_user_id: UserId,
    pub channel_id: ChannelId,
    pub category_id: Option<i64>,
    pub category_label_snapshot: String,
    pub details_text: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Report {
    pub id: String,
    pub reporter_id: String,
    pub reported_user_id: String,
    pub channel_id: String,
    pub category_label_snapshot: String,
    pub details_text: String,
    pub has_audio: bool,
    pub audio_dir: Option<String>,
    pub transcript_json: Option<String>,
    pub status: String,
    pub report_message_id: Option<String>,
    pub created_at: String,
    pub finalized_at: Option<String>,
}

pub async fn create(pool: &SqlitePool, new: &NewReport) -> Result<()> {
    let now = crate::db::now_iso();
    sqlx::query(
        "INSERT INTO reports (id, reporter_id, reported_user_id, channel_id, category_id,
            category_label_snapshot, details_text, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
    )
    .bind(&new.id)
    .bind(new.reporter_id.to_string())
    .bind(new.reported_user_id.to_string())
    .bind(new.channel_id.to_string())
    .bind(new.category_id)
    .bind(&new.category_label_snapshot)
    .bind(&new.details_text)
    .bind(now)
    .execute(pool)
    .await?;

    add_participant(pool, &new.id, new.reporter_id, ParticipantRole::Reporter).await?;
    add_participant(pool, &new.id, new.reported_user_id, ParticipantRole::Reported).await?;
    Ok(())
}

pub async fn add_participant(pool: &SqlitePool, report_id: &str, user_id: UserId, role: ParticipantRole) -> Result<()> {
    sqlx::query(
        "INSERT INTO report_participants (report_id, user_id, role) VALUES (?, ?, ?)
         ON CONFLICT(report_id, user_id) DO NOTHING",
    )
    .bind(report_id)
    .bind(user_id.to_string())
    .bind(role.as_db_str())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finalize_with_audio(
    pool: &SqlitePool,
    report_id: &str,
    audio_dir: &str,
    transcript: &Transcript,
) -> Result<()> {
    // Must match the `{ "lines": [...] }` shape `Transcript` deserializes on the read side
    // (report_message.rs) -- serializing just the inner `Vec<TranscriptLine>` here previously
    // produced a bare JSON array, which silently failed to deserialize back into `Transcript`
    // and always rendered as "no speech detected" regardless of what whisper actually produced.
    let transcript_json = serde_json::to_string(transcript).unwrap_or_default();
    let now = crate::db::now_iso();
    sqlx::query(
        "UPDATE reports SET has_audio = 1, audio_dir = ?, transcript_json = ?, finalized_at = ? WHERE id = ?",
    )
    .bind(audio_dir)
    .bind(transcript_json)
    .bind(now)
    .bind(report_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finalize_without_audio(pool: &SqlitePool, report_id: &str) -> Result<()> {
    let now = crate::db::now_iso();
    sqlx::query("UPDATE reports SET has_audio = 0, finalized_at = ? WHERE id = ?")
        .bind(now)
        .bind(report_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_message_id(pool: &SqlitePool, report_id: &str, message_id: MessageId) -> Result<()> {
    sqlx::query("UPDATE reports SET report_message_id = ? WHERE id = ?")
        .bind(message_id.to_string())
        .bind(report_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_status(pool: &SqlitePool, report_id: &str, status: ReportStatus) -> Result<()> {
    sqlx::query("UPDATE reports SET status = ? WHERE id = ?")
        .bind(status.as_db_str())
        .bind(report_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, report_id: &str) -> Result<Option<Report>> {
    let row = sqlx::query_as::<_, Report>("SELECT * FROM reports WHERE id = ?")
        .bind(report_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_involving(pool: &SqlitePool, user_id: UserId) -> Result<Vec<Report>> {
    let uid = user_id.to_string();
    let rows = sqlx::query_as::<_, Report>(
        "SELECT r.* FROM reports r
         JOIN report_participants p ON p.report_id = r.id
         WHERE p.user_id = ?
         ORDER BY r.created_at DESC",
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
