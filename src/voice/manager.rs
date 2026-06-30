use std::sync::{Arc, Mutex as StdMutex};

use arc_swap::ArcSwap;
use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use serenity::model::id::{ChannelId, GuildId, UserId};
use songbird::{CoreEvent, Songbird};

use crate::consent::cache::ConsentCache;
use crate::db::consent::ConsentState;
use crate::voice::buffer::AudioBufferPool;
use crate::voice::receiver::Receiver;
use crate::voice::strategy::{VcEvent, VcStrategy, VcStrategyKind, VcWorldView};

struct WorldState {
    members: DashMap<ChannelId, DashSet<UserId>>,
    last_activity: DashMap<ChannelId, DateTime<Utc>>,
    current_channel: StdMutex<Option<ChannelId>>,
    consent_cache: Arc<ConsentCache>,
}

impl VcWorldView for WorldState {
    fn members_in(&self, channel: ChannelId) -> Vec<(UserId, ConsentState)> {
        self.members
            .get(&channel)
            .map(|set| set.iter().map(|u| (*u, self.consent_cache.get(*u))).collect())
            .unwrap_or_default()
    }

    fn all_active_channels(&self) -> Vec<ChannelId> {
        self.members
            .iter()
            .filter(|e| !e.value().is_empty())
            .map(|e| *e.key())
            .collect()
    }

    fn last_activity(&self, channel: ChannelId) -> Option<DateTime<Utc>> {
        self.last_activity.get(&channel).map(|v| *v)
    }

    fn current_channel(&self) -> Option<ChannelId> {
        *self.current_channel.lock().expect("current_channel mutex poisoned")
    }

    fn channel_of(&self, user: UserId) -> Option<ChannelId> {
        self.members.iter().find(|e| e.value().contains(&user)).map(|e| *e.key())
    }
}

/// Owns the single live voice connection for the guild and the currently active
/// `VcStrategy`, hot-swappable via `/config vc-strategy` through an `ArcSwap`. All
/// join/leave decisions are funneled through a single-consumer channel so concurrent
/// gateway events can't race each other into conflicting connection changes.
pub struct VcManager {
    guild_id: GuildId,
    songbird: Arc<Songbird>,
    world: Arc<WorldState>,
    strategy: ArcSwap<Box<dyn VcStrategy>>,
    audio_pool: Arc<AudioBufferPool>,
    consent_cache: Arc<ConsentCache>,
    tx: tokio::sync::mpsc::UnboundedSender<VcEvent>,
}

impl VcManager {
    pub fn new(
        guild_id: GuildId,
        songbird: Arc<Songbird>,
        audio_pool: Arc<AudioBufferPool>,
        consent_cache: Arc<ConsentCache>,
        initial_strategy: VcStrategyKind,
    ) -> Arc<Self> {
        let world = Arc::new(WorldState {
            members: DashMap::new(),
            last_activity: DashMap::new(),
            current_channel: StdMutex::new(None),
            consent_cache: consent_cache.clone(),
        });

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let manager = Arc::new(Self {
            guild_id,
            songbird,
            world,
            strategy: ArcSwap::new(Arc::new(initial_strategy.build())),
            audio_pool,
            consent_cache,
            tx,
        });

        manager.clone().spawn_consumer(rx);
        manager
    }

    fn spawn_consumer(self: Arc<Self>, mut rx: tokio::sync::mpsc::UnboundedReceiver<VcEvent>) {
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let target = self.strategy.load().decide(&event, &*self.world);
                if target != self.world.current_channel() {
                    self.transition_to(target).await;
                }
            }
        });
    }

    pub fn set_strategy(&self, kind: VcStrategyKind) {
        self.strategy.store(Arc::new(kind.build()));
    }

    pub fn world(&self) -> &dyn VcWorldView {
        &*self.world
    }

    pub fn current_channel(&self) -> Option<ChannelId> {
        self.world.current_channel()
    }

    /// Records membership + activity, then asks the active strategy to re-decide.
    pub fn notify_joined(&self, channel: ChannelId, user: UserId) {
        self.world.members.entry(channel).or_default().insert(user);
        self.world.last_activity.insert(channel, Utc::now());
        let _ = self.tx.send(VcEvent::UserJoined { channel, user, at: Utc::now() });
    }

    pub fn notify_left(&self, channel: ChannelId, user: UserId) {
        if let Some(set) = self.world.members.get(&channel) {
            set.remove(&user);
        }
        let _ = self.tx.send(VcEvent::UserLeft { channel, user, at: Utc::now() });
    }

    pub fn notify_moved(&self, from: ChannelId, to: ChannelId, user: UserId) {
        if let Some(set) = self.world.members.get(&from) {
            set.remove(&user);
        }
        self.world.members.entry(to).or_default().insert(user);
        self.world.last_activity.insert(to, Utc::now());
        let _ = self.tx.send(VcEvent::UserMoved { from, to, user, at: Utc::now() });
    }

    pub fn notify_consent_changed(&self, user: UserId, channel: Option<ChannelId>, consent: ConsentState) {
        if consent == ConsentState::Granted {
            if let Some(c) = channel {
                self.world.last_activity.insert(c, Utc::now());
            }
        }
        let _ = self.tx.send(VcEvent::ConsentChanged { user, channel, consent });
    }

    async fn transition_to(&self, target: Option<ChannelId>) {
        let current = self.world.current_channel();
        if current == target {
            return;
        }

        if current.is_some() {
            if let Err(e) = self.songbird.remove(self.guild_id).await {
                tracing::warn!("error leaving previous voice channel: {e}");
            }
        }

        if let Some(channel) = target {
            let call_lock = self.songbird.get_or_insert(self.guild_id);
            {
                let mut call = call_lock.lock().await;
                let receiver = Receiver::new(self.audio_pool.clone(), self.consent_cache.clone());
                call.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());
                call.add_global_event(CoreEvent::VoiceTick.into(), receiver.clone());
                call.add_global_event(CoreEvent::ClientDisconnect.into(), receiver);
            }

            match self.songbird.join(self.guild_id, channel).await {
                Ok(_) => {
                    *self.world.current_channel.lock().expect("poisoned") = Some(channel);
                    tracing::info!(?channel, strategy = self.strategy.load().name(), "joined voice channel");
                }
                Err(e) => {
                    tracing::error!("failed to join voice channel {channel}: {e}");
                    let _ = self.songbird.remove(self.guild_id).await;
                    *self.world.current_channel.lock().expect("poisoned") = None;
                }
            }
        } else {
            *self.world.current_channel.lock().expect("poisoned") = None;
            tracing::info!("left voice channel (no eligible channel to record)");
        }
    }
}
