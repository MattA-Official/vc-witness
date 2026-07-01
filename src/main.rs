mod config;
mod consent;
mod db;
mod discord;
mod error;
mod report;
mod transcription;
mod util;
mod voice;

use std::sync::Arc;

use chrono::Duration;
use serenity::gateway::client::Client;
use serenity::http::Http;
use serenity::model::gateway::GatewayIntents;
use songbird::driver::{DecodeConfig, DecodeMode};
use songbird::{Config as SongbirdConfig, Songbird};

use crate::config::StartupConfig;
use crate::consent::cache::ConsentCache;
use crate::consent::ConsentEngine;
use crate::discord::state::AppState;
use crate::discord::Handler;
use crate::transcription::TranscriptionService;
use crate::voice::buffer::AudioBufferPool;
use crate::voice::manager::VcManager;

#[tokio::main]
async fn main() {
    // Multiple dependencies pull in rustls with different default crypto-provider features
    // enabled, which leaves the process-level provider ambiguous unless set explicitly here.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    // `EnvFilter::from_default_env()` filters out *everything*, including errors, if RUST_LOG
    // isn't set -- which silently hid pipeline failures (e.g. report-posting errors) during
    // testing. Default to "info" so warnings/errors are always visible unless overridden.
    let log_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(log_filter).init();

    let startup = StartupConfig::from_env().expect("invalid startup configuration");

    let db = db::connect(&startup.database_path).await.expect("failed to connect to database");
    db::categories::ensure_defaults(&db, startup.guild_id).await.expect("failed to seed default categories");
    let cfg = db::guild_config::get_or_init(&db, startup.guild_id).await.expect("failed to load guild config");

    let transcription = Arc::new(
        TranscriptionService::load(&startup.whisper_model_path, startup.whisper_max_concurrent_jobs)
            .expect("failed to load whisper model"),
    );

    let songbird_config = SongbirdConfig::default().decode_mode(DecodeMode::Decode(DecodeConfig::default()));
    let songbird = Songbird::serenity_from_config(songbird_config);

    let consent_cache = Arc::new(ConsentCache::new());
    let audio_pool = Arc::new(AudioBufferPool::new(Duration::seconds(cfg.buffer_duration_secs)));
    let consent_engine = Arc::new(ConsentEngine::new(db.clone(), consent_cache.clone(), audio_pool.clone()));

    let vc_manager = VcManager::new(startup.guild_id, songbird.clone(), audio_pool.clone(), consent_cache.clone(), cfg.vc_strategy);

    let reports_dir = startup.database_path.parent().unwrap_or_else(|| std::path::Path::new(".")).join("reports");

    let token: serenity::secrets::Token = startup.discord_token.parse().expect("DISCORD_TOKEN is not a valid token");

    let bot_user_id = Http::new(token.clone())
        .get_current_user()
        .await
        .expect("failed to fetch bot user, check DISCORD_TOKEN")
        .id;

    let state = Arc::new(AppState {
        db,
        guild_id: startup.guild_id,
        bot_user_id,
        songbird: songbird.clone(),
        vc_manager,
        consent_engine,
        consent_cache,
        audio_pool,
        transcription,
        reports_dir,
    });

    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILD_MEMBERS;

    // We wire Songbird in manually (rather than via `songbird::serenity::SerenityInit`) since
    // that convenience shim targets an older serenity module layout than the `next` branch
    // revision pinned here -- `ClientBuilder::voice_manager` is the same underlying hook.
    let mut client = Client::builder(token, intents)
        .event_handler(Arc::new(Handler { state }))
        .voice_manager(songbird as Arc<dyn serenity::gateway::VoiceGatewayManager>)
        .await
        .expect("failed to build client");

    if let Err(e) = client.start().await {
        tracing::error!("client error: {e}");
    }
}
