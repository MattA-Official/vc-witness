use serenity::model::id::GuildId;
use std::path::PathBuf;

use crate::error::{Result, WitnessError};

/// Startup-only configuration. Anything that can change at runtime (reports channel,
/// mod role, vc strategy, buffer durations, categories...) lives in `guild_config` /
/// `report_categories` in the database instead, configured live via Discord commands.
#[derive(Debug, Clone)]
pub struct StartupConfig {
    pub discord_token: String,
    pub guild_id: GuildId,
    pub database_path: PathBuf,
    pub whisper_model_path: PathBuf,
    pub whisper_max_concurrent_jobs: usize,
}

impl StartupConfig {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let discord_token = required_env("DISCORD_TOKEN")?;
        let guild_id_raw = required_env("GUILD_ID")?;
        let guild_id: u64 = guild_id_raw
            .parse()
            .map_err(|_| WitnessError::Config(format!("GUILD_ID is not a valid u64: {guild_id_raw}")))?;

        let database_path = std::env::var("DATABASE_PATH")
            .unwrap_or_else(|_| "data/witness.sqlite3".to_string())
            .into();

        let whisper_model_path = required_env("WHISPER_MODEL_PATH")?.into();

        let whisper_max_concurrent_jobs = std::env::var("WHISPER_MAX_CONCURRENT_JOBS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| std::cmp::max(1, num_cpus::get().saturating_sub(1)));

        Ok(Self {
            discord_token,
            guild_id: GuildId::new(guild_id),
            database_path,
            whisper_model_path,
            whisper_max_concurrent_jobs,
        })
    }
}

fn required_env(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| WitnessError::Config(format!("missing required env var: {key}")))
}
