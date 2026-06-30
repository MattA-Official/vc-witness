use std::sync::Arc;

use serenity::model::id::{GuildId, UserId};
use songbird::Songbird;
use sqlx::SqlitePool;

use crate::consent::cache::ConsentCache;
use crate::consent::ConsentEngine;
use crate::transcription::TranscriptionService;
use crate::voice::buffer::AudioBufferPool;
use crate::voice::manager::VcManager;

/// Shared across every event/command/interaction handler. Guild-configurable values
/// (reports channel, mod role, vc strategy, buffer/tail durations, categories) are
/// deliberately NOT cached here -- they're read fresh from `guild_config`/`report_categories`
/// on each use, since this is a single small guild and SQLite reads are cheap, and it avoids
/// any cache-invalidation bugs when `/config ...` commands change them.
pub struct AppState {
    pub db: SqlitePool,
    pub guild_id: GuildId,
    pub bot_user_id: UserId,
    pub songbird: Arc<Songbird>,
    pub vc_manager: Arc<VcManager>,
    pub consent_engine: Arc<ConsentEngine>,
    pub consent_cache: Arc<ConsentCache>,
    pub audio_pool: Arc<AudioBufferPool>,
    pub transcription: Arc<TranscriptionService>,
    pub reports_dir: std::path::PathBuf,
}
