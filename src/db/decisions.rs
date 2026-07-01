use serenity::model::id::UserId;
use sqlx::SqlitePool;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    ActionTaken,
    NoActionTaken,
    Dismissed,
}

impl Decision {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Decision::ActionTaken => "action_taken",
            Decision::NoActionTaken => "no_action_taken",
            Decision::Dismissed => "dismissed",
        }
    }

    /// Dismiss is the only decision that does not notify the reporter.
    pub fn notifies_reporter(&self) -> bool {
        !matches!(self, Decision::Dismissed)
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Decision::ActionTaken => "Action Taken",
            Decision::NoActionTaken => "No Action Taken",
            Decision::Dismissed => "Dismissed",
        }
    }
}

pub async fn insert(
    pool: &SqlitePool,
    report_id: &str,
    moderator_id: UserId,
    decision: Decision,
    note: Option<&str>,
) -> Result<()> {
    let now = crate::db::now_iso();
    sqlx::query(
        "INSERT INTO moderator_decisions (report_id, moderator_id, decision, note, decided_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(report_id)
    .bind(moderator_id.to_string())
    .bind(decision.as_db_str())
    .bind(note)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}
