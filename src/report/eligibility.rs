use chrono::Duration;
use serenity::model::id::UserId;

use crate::db::voice_events;
use crate::error::Result;
use crate::voice::strategy::VcWorldView;

pub enum Eligibility {
    Eligible,
    NotEligible(String),
}

/// A report can be filed if the target is currently in a VC, or left "recently" -- where
/// "recently" is deliberately tied to the rolling audio buffer window, since that's the
/// horizon for which any audio could possibly still exist.
pub async fn check(
    pool: &sqlx::SqlitePool,
    world: &dyn VcWorldView,
    target: UserId,
    recent_window_secs: i64,
) -> Result<Eligibility> {
    if world.channel_of(target).is_some() {
        return Ok(Eligibility::Eligible);
    }

    let recent_window = Duration::seconds(recent_window_secs);
    match voice_events::last_left_at(pool, target).await? {
        Some((_, left_at)) if chrono::Utc::now() - left_at <= recent_window => Ok(Eligibility::Eligible),
        _ => Ok(Eligibility::NotEligible(format!(
            "<@{target}> is not currently in a voice channel and did not leave one in the last {} minutes, so a report can't be filed against them.",
            recent_window_secs / 60
        ))),
    }
}
