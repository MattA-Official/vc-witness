use serenity::gateway::client::Context;
use serenity::model::voice::VoiceState;

use crate::consent::reminder;
use crate::db::consent::{self, ConsentState};
use crate::db::guild_config::{self, GuildConfig};
use crate::db::voice_events::{self, VoiceEventType};
use crate::discord::state::AppState;

/// The single entry point voice-channel reactivity flows through: every `VoiceStateUpdate`
/// from the gateway lands here, drives the consent state machine for fresh joins, and feeds
/// `VcManager` so the active `VcStrategy` can re-decide which channel to record.
pub async fn handle(ctx: &Context, state: &AppState, old: Option<VoiceState>, new: &VoiceState) {
    let user_id = new.user_id;
    if user_id == state.bot_user_id {
        return;
    }

    let cfg = match guild_config::get_or_init(&state.db, state.guild_id).await {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!("failed to load guild config, skipping voice state update for {user_id}: {e}");
            return;
        }
    };

    // Witness shouldn't mute, DM, or record anyone until a moderator has set up at least a
    // reports channel -- otherwise consent gets collected and audio gets buffered for a bot
    // that has nowhere to actually post what it captures.
    if cfg.reports_channel_id.is_none() {
        return;
    }

    let old_channel = old.and_then(|s| s.channel_id);
    let new_channel = new.channel_id;

    match (old_channel, new_channel) {
        (None, Some(channel)) => on_fresh_join(ctx, state, &cfg, channel, user_id).await,
        (Some(channel), None) => {
            let _ = voice_events::log(&state.db, channel, user_id, VoiceEventType::Leave).await;
            state.vc_manager.notify_left(channel, user_id);
        }
        (Some(from), Some(to)) if from != to => {
            let _ = voice_events::log(&state.db, to, user_id, VoiceEventType::Move).await;
            state.vc_manager.notify_moved(from, to, user_id);
        }
        _ => {}
    }
}

async fn on_fresh_join(
    ctx: &Context,
    state: &AppState,
    cfg: &GuildConfig,
    channel: serenity::model::id::ChannelId,
    user_id: serenity::model::id::UserId,
) {
    if let Err(e) = voice_events::log(&state.db, channel, user_id, VoiceEventType::Join).await {
        tracing::warn!("failed to log voice join: {e}");
    }

    match consent::get_state(&state.db, user_id).await {
        Ok(ConsentState::Unknown) => {
            if let Err(e) = state.consent_engine.handle_fresh_join(&ctx.http, state.guild_id, user_id).await {
                tracing::warn!("consent fresh-join flow failed for {user_id}: {e}");
            }
        }
        Ok(ConsentState::Pending) => {
            // Already muted, awaiting their DM response, nothing to do.
        }
        Ok(ConsentState::Granted) => {
            state.consent_cache.set(user_id, ConsentState::Granted);
            let reminder_text = reminder::reminder_text(cfg.consent_reminder_text.as_deref());
            if let Err(e) = state
                .consent_engine
                .handle_returning_granted(&ctx.http, state.guild_id, user_id, &reminder_text)
                .await
            {
                tracing::warn!("consent reminder flow failed for {user_id}: {e}");
            }
        }
        Err(e) => tracing::warn!("failed to read consent state for {user_id}: {e}"),
    }

    state.vc_manager.notify_joined(channel, user_id);
}
