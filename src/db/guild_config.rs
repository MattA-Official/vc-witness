use serenity::model::id::{ChannelId, GuildId, RoleId};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::voice::strategy::VcStrategyKind;

#[derive(Debug, Clone)]
pub struct GuildConfig {
    pub guild_id: GuildId,
    pub reports_channel_id: Option<ChannelId>,
    pub mod_role_id: Option<RoleId>,
    pub vc_strategy: VcStrategyKind,
    pub buffer_duration_secs: i64,
    pub post_report_tail_secs: i64,
    pub consent_reminder_text: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    guild_id: String,
    reports_channel_id: Option<String>,
    mod_role_id: Option<String>,
    vc_strategy: String,
    buffer_duration_secs: i64,
    post_report_tail_secs: i64,
    consent_reminder_text: Option<String>,
}

impl From<Row> for GuildConfig {
    fn from(r: Row) -> Self {
        GuildConfig {
            guild_id: GuildId::new(r.guild_id.parse().unwrap_or_default()),
            reports_channel_id: r.reports_channel_id.and_then(|s| s.parse().ok()).map(ChannelId::new),
            mod_role_id: r.mod_role_id.and_then(|s| s.parse().ok()).map(RoleId::new),
            vc_strategy: VcStrategyKind::from_db_str(&r.vc_strategy),
            buffer_duration_secs: r.buffer_duration_secs,
            post_report_tail_secs: r.post_report_tail_secs,
            consent_reminder_text: r.consent_reminder_text,
        }
    }
}

/// Ensures the singleton guild_config row exists, returning it (creating with defaults if absent).
pub async fn get_or_init(pool: &SqlitePool, guild_id: GuildId) -> Result<GuildConfig> {
    let gid = guild_id.to_string();
    let now = crate::db::now_iso();

    sqlx::query(
        "INSERT INTO guild_config (guild_id, updated_at) VALUES (?, ?)
         ON CONFLICT(guild_id) DO NOTHING",
    )
    .bind(&gid)
    .bind(&now)
    .execute(pool)
    .await?;

    let row: Row = sqlx::query_as(
        r#"SELECT guild_id, reports_channel_id, mod_role_id, vc_strategy,
                  buffer_duration_secs, post_report_tail_secs, consent_reminder_text
           FROM guild_config WHERE guild_id = ?"#,
    )
    .bind(&gid)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn set_reports_channel(pool: &SqlitePool, guild_id: GuildId, channel_id: ChannelId) -> Result<()> {
    let gid = guild_id.to_string();
    let cid = channel_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query("UPDATE guild_config SET reports_channel_id = ?, updated_at = ? WHERE guild_id = ?")
        .bind(cid)
        .bind(now)
        .bind(gid)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_mod_role(pool: &SqlitePool, guild_id: GuildId, role_id: RoleId) -> Result<()> {
    let gid = guild_id.to_string();
    let rid = role_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query("UPDATE guild_config SET mod_role_id = ?, updated_at = ? WHERE guild_id = ?")
        .bind(rid)
        .bind(now)
        .bind(gid)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_vc_strategy(pool: &SqlitePool, guild_id: GuildId, strategy: VcStrategyKind) -> Result<()> {
    let gid = guild_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query("UPDATE guild_config SET vc_strategy = ?, updated_at = ? WHERE guild_id = ?")
        .bind(strategy.as_db_str())
        .bind(now)
        .bind(gid)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_buffer_duration(pool: &SqlitePool, guild_id: GuildId, secs: i64) -> Result<()> {
    let gid = guild_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query("UPDATE guild_config SET buffer_duration_secs = ?, updated_at = ? WHERE guild_id = ?")
        .bind(secs)
        .bind(now)
        .bind(gid)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_tail_duration(pool: &SqlitePool, guild_id: GuildId, secs: i64) -> Result<()> {
    let gid = guild_id.to_string();
    let now = crate::db::now_iso();
    sqlx::query("UPDATE guild_config SET post_report_tail_secs = ?, updated_at = ? WHERE guild_id = ?")
        .bind(secs)
        .bind(now)
        .bind(gid)
        .execute(pool)
        .await?;
    Ok(())
}
