pub mod cache;
pub mod reminder;

use std::sync::Arc;

use chrono::Duration;
use serenity::builder::{CreateButton, CreateContainer, CreateContainerComponent, CreateMessage, CreateTextDisplay, EditMember};
use serenity::http::Http;
use serenity::model::application::ButtonStyle;
use serenity::model::channel::MessageFlags;
use serenity::model::id::{GuildId, UserId};
use sqlx::SqlitePool;

use crate::consent::cache::ConsentCache;
use crate::db::consent::{self, ConsentState};
use crate::error::Result;
use crate::voice::buffer::AudioBufferPool;
use crate::voice::manager::VcManager;

pub const CONSENT_ACCEPT_ID: &str = "consent:accept";
pub const CONSENT_DECLINE_ID: &str = "consent:decline";

/// Drives the GDPR consent state machine. Mute/DM/unmute/kick all happen here; the
/// `ConsentCache` mirror is updated synchronously alongside the DB so the voice-receive
/// hot path (`voice::receiver`) always sees an up-to-date `Granted`/not-granted answer.
pub struct ConsentEngine {
    pool: SqlitePool,
    cache: Arc<ConsentCache>,
    audio_pool: Arc<AudioBufferPool>,
}

impl ConsentEngine {
    pub fn new(pool: SqlitePool, cache: Arc<ConsentCache>, audio_pool: Arc<AudioBufferPool>) -> Self {
        Self { pool, cache, audio_pool }
    }

    pub fn cache(&self) -> Arc<ConsentCache> {
        self.cache.clone()
    }

    /// Called on a user's first VC join with no/unknown consent state: mute, DM the
    /// consent prompt, mark pending. Defensive against members the bot can't actually
    /// mute (e.g. higher role hierarchy) -- treated as not-recordable rather than failing.
    pub async fn handle_fresh_join(&self, http: &Http, guild_id: GuildId, user_id: UserId) -> Result<()> {
        if let Err(e) = EditMember::new().mute(true).execute(http, guild_id, user_id).await {
            tracing::warn!("could not server-mute {user_id} (likely role hierarchy), treating as not-recordable: {e}");
        }

        consent::mark_pending(&self.pool, user_id).await?;
        self.cache.set(user_id, ConsentState::Pending);

        if let Ok(dm) = user_id.create_dm_channel(http).await {
            let _ = dm.id.widen().send_message(http, build_consent_prompt()).await;
        }

        Ok(())
    }

    /// Called on every join once consent is already `Granted`: unmute defensively, send
    /// a rate-limited "you can opt out anytime" reminder (not a re-consent flow).
    pub async fn handle_returning_granted(&self, http: &Http, guild_id: GuildId, user_id: UserId, reminder_text: &str) -> Result<()> {
        if let Err(e) = EditMember::new().mute(false).execute(http, guild_id, user_id).await {
            tracing::debug!("unmute on rejoin failed for {user_id} (may already be unmuted): {e}");
        }

        if consent::should_send_reminder(&self.pool, user_id, Duration::hours(12)).await? {
            match user_id.create_dm_channel(http).await {
                Ok(dm) => {
                    if let Err(e) = dm.id.widen().send_message(http, build_reminder_prompt(reminder_text)).await {
                        tracing::warn!("failed to send consent reminder DM to {user_id}: {e}");
                    }
                }
                Err(e) => tracing::warn!("could not open DM channel to {user_id} to send consent reminder: {e}"),
            }
            consent::record_reminder_sent(&self.pool, user_id).await?;
        } else {
            tracing::debug!("skipping consent reminder for {user_id}: sent one within the last rate-limit window");
        }
        Ok(())
    }

    pub async fn handle_accept(&self, http: &Http, guild_id: GuildId, user_id: UserId, vc: &VcManager) -> Result<()> {
        consent::mark_granted(&self.pool, user_id).await?;
        self.cache.set(user_id, ConsentState::Granted);

        if let Err(e) = EditMember::new().mute(false).execute(http, guild_id, user_id).await {
            tracing::warn!("could not unmute {user_id} after consent: {e}");
        }

        vc.notify_consent_changed(user_id, vc.current_channel(), ConsentState::Granted);
        Ok(())
    }

    /// Decline is transient: kick from VC, delete the consent row entirely (not stored as
    /// a terminal state) so the next join restarts the whole flow from scratch.
    pub async fn handle_decline(&self, http: &Http, guild_id: GuildId, user_id: UserId) -> Result<()> {
        // Was previously logged at `debug` (invisible under the default "info" filter), which
        // made a real failure here -- e.g. the bot's role lacking the Move Members permission
        // it needs to disconnect anyone -- indistinguishable from "user had already left".
        if let Err(e) = EditMember::new().disconnect_member().execute(http, guild_id, user_id).await {
            tracing::warn!("failed to disconnect {user_id} from voice on decline/opt-out (check the bot role has Move Members): {e}");
        }

        consent::clear(&self.pool, user_id).await?;
        self.cache.clear(user_id);
        Ok(())
    }

    /// Drops consent record + any unreported buffered audio. Filed reports are NOT touched
    /// here -- that retention exception is handled entirely in report/db queries which
    /// simply never reference `user_consent`.
    ///
    /// Not currently wired to `/data erase` (see `discord::commands::data_erase`) -- doing
    /// so on demand would let a user named in an in-flight report scrub their
    /// already-extracted audio before it's persisted. Kept here as the building block for
    /// a future moderator-mediated or report-aware erasure flow.
    #[allow(dead_code)]
    pub async fn erase(&self, user_id: UserId) -> Result<()> {
        consent::clear(&self.pool, user_id).await?;
        self.cache.clear(user_id);
        self.audio_pool.purge(user_id);
        Ok(())
    }
}

fn build_consent_prompt() -> CreateMessage<'static> {
    let container = CreateContainer::new(vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(reminder::CONSENT_PROMPT_TEXT)),
    ]);

    CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![
            serenity::builder::CreateComponent::Container(container),
            serenity::builder::CreateComponent::ActionRow(serenity::builder::CreateActionRow::buttons(vec![
                CreateButton::new(CONSENT_ACCEPT_ID).label("I consent").style(ButtonStyle::Success),
                CreateButton::new(CONSENT_DECLINE_ID).label("I decline").style(ButtonStyle::Danger),
            ])),
        ])
}

/// Same shape as the initial consent prompt, but reused for the on-rejoin reminder with a
/// single "Opt out" button -- intentionally wired to the same `CONSENT_DECLINE_ID` handler
/// as the original decline button, since opting out later has identical semantics (kick +
/// reset consent state so the full flow restarts on the next join).
fn build_reminder_prompt(text: &str) -> CreateMessage<'static> {
    let container = CreateContainer::new(vec![CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
        text.to_string(),
    ))]);

    CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![
            serenity::builder::CreateComponent::Container(container),
            serenity::builder::CreateComponent::ActionRow(serenity::builder::CreateActionRow::buttons(vec![CreateButton::new(
                CONSENT_DECLINE_ID,
            )
            .label("Opt out")
            .style(ButtonStyle::Danger)])),
        ])
}
