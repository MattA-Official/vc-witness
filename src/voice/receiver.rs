use std::sync::Arc;

use dashmap::DashMap;
use serenity::async_trait;
use serenity::model::id::UserId as SerenityUserId;
use songbird::{Event, EventContext, EventHandler as VoiceEventHandler};

use crate::consent::cache::ConsentCache;
use crate::voice::buffer::AudioBufferPool;

fn to_serenity_user(id: songbird::model::id::UserId) -> SerenityUserId {
    SerenityUserId::new(id.0)
}

/// Registered as a songbird `EventHandler` on the single active `Call`. Maps RTP SSRCs to
/// Discord user IDs (the only place that mapping is ever known is `SpeakingStateUpdate`),
/// then on every 20ms `VoiceTick` routes each speaking user's decoded PCM into their
/// `RollingBuffer` -- but only if `ConsentCache` says they're `Granted`. Frames for anyone
/// else are dropped immediately and never buffered; this is the sole enforcement point for
/// "non-consenting audio is never captured."
#[derive(Clone)]
pub struct Receiver {
    audio_pool: Arc<AudioBufferPool>,
    consent_cache: Arc<ConsentCache>,
    known_ssrcs: Arc<DashMap<u32, SerenityUserId>>,
}

impl Receiver {
    pub fn new(audio_pool: Arc<AudioBufferPool>, consent_cache: Arc<ConsentCache>) -> Self {
        Self {
            audio_pool,
            consent_cache,
            known_ssrcs: Arc::new(DashMap::new()),
        }
    }
}

#[async_trait]
impl VoiceEventHandler for Receiver {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(speaking) => {
                if let Some(user) = speaking.user_id {
                    self.known_ssrcs.insert(speaking.ssrc, to_serenity_user(user));
                }
            }
            EventContext::VoiceTick(tick) => {
                let now = chrono::Utc::now();
                for (ssrc, data) in &tick.speaking {
                    let Some(samples) = data.decoded_voice.as_ref() else { continue };
                    let Some(user) = self.known_ssrcs.get(ssrc).map(|u| *u) else { continue };
                    if !self.consent_cache.is_granted(user) {
                        continue;
                    }
                    self.audio_pool.push(user, now, samples.clone());
                }
            }
            EventContext::ClientDisconnect(disconnect) => {
                let user = to_serenity_user(disconnect.user_id);
                self.known_ssrcs.retain(|_, v| *v != user);
            }
            _ => {}
        }
        None
    }
}
