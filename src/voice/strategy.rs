use chrono::{DateTime, Utc};
use serenity::model::id::{ChannelId, UserId};

use crate::db::consent::ConsentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VcStrategyKind {
    MostRecentActivity,
    Busiest,
    StickyUntilEmpty,
}

impl VcStrategyKind {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            VcStrategyKind::MostRecentActivity => "most_recent_activity",
            VcStrategyKind::Busiest => "busiest",
            VcStrategyKind::StickyUntilEmpty => "sticky_until_empty",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "busiest" => VcStrategyKind::Busiest,
            "sticky_until_empty" => VcStrategyKind::StickyUntilEmpty,
            _ => VcStrategyKind::MostRecentActivity,
        }
    }

    pub fn build(&self) -> Box<dyn VcStrategy> {
        match self {
            VcStrategyKind::MostRecentActivity => Box::new(super::strategies::most_recent_activity::MostRecentActivity),
            VcStrategyKind::Busiest => Box::new(super::strategies::busiest::Busiest),
            VcStrategyKind::StickyUntilEmpty => Box::new(super::strategies::sticky_until_empty::StickyUntilEmpty),
        }
    }
}

#[derive(Debug, Clone)]
pub enum VcEvent {
    UserJoined { channel: ChannelId, user: UserId, at: DateTime<Utc> },
    UserLeft { channel: ChannelId, user: UserId, at: DateTime<Utc> },
    UserMoved { from: ChannelId, to: ChannelId, user: UserId, at: DateTime<Utc> },
    ConsentChanged { user: UserId, channel: Option<ChannelId>, consent: ConsentState },
}

/// Read-only snapshot of guild voice state the strategy can query without owning gateway state.
/// Implemented by `VcManager` against its in-memory tracking + the `voice_activity_log` table.
pub trait VcWorldView: Send + Sync {
    fn members_in(&self, channel: ChannelId) -> Vec<(UserId, ConsentState)>;
    fn all_active_channels(&self) -> Vec<ChannelId>;
    fn last_activity(&self, channel: ChannelId) -> Option<DateTime<Utc>>;
    fn current_channel(&self) -> Option<ChannelId>;
    /// Which voice channel (if any) the given user is currently in, anywhere in the guild --
    /// independent of which single channel the bot itself is connected to/recording.
    fn channel_of(&self, user: UserId) -> Option<ChannelId>;
}

impl dyn VcWorldView + '_ {
    pub fn consenting_count(&self, channel: ChannelId) -> usize {
        self.members_in(channel)
            .into_iter()
            .filter(|(_, c)| matches!(c, ConsentState::Granted))
            .count()
    }
}

/// Decides which single voice channel the bot should be connected to, given the
/// guild can only hold one voice connection at a time. Implementations are pure
/// (no I/O) so they're trivially unit-testable against a mocked `VcWorldView`.
pub trait VcStrategy: Send + Sync {
    /// Returns the channel the bot should be connected to after this event (or `None` to leave).
    fn decide(&self, event: &VcEvent, world: &dyn VcWorldView) -> Option<ChannelId>;
    fn name(&self) -> &'static str;
}
