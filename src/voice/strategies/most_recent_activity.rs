use serenity::model::id::ChannelId;

use crate::voice::strategy::{VcEvent, VcStrategy, VcWorldView};

/// Joins whichever channel most recently had a consenting join/consent event.
/// Channels active simultaneously elsewhere simply go unrecorded -- the simplest,
/// most predictable of the three policies.
pub struct MostRecentActivity;

impl VcStrategy for MostRecentActivity {
    fn decide(&self, event: &VcEvent, world: &dyn VcWorldView) -> Option<ChannelId> {
        let trigger_channel = match event {
            VcEvent::UserJoined { channel, .. } => Some(*channel),
            VcEvent::UserMoved { to, .. } => Some(*to),
            VcEvent::ConsentChanged { channel, .. } => *channel,
            VcEvent::UserLeft { channel, .. } => {
                // If the channel we were just in is now empty of consenting members,
                // fall through to whichever other active channel had the most recent activity.
                if world.consenting_count(*channel) == 0 && world.current_channel() == Some(*channel) {
                    None
                } else {
                    return world.current_channel();
                }
            }
        };

        if let Some(channel) = trigger_channel {
            if world.consenting_count(channel) > 0 {
                return Some(channel);
            }
        }

        // Fall back to the busiest-by-recency active channel with at least one consenting member.
        world
            .all_active_channels()
            .into_iter()
            .filter(|c| world.consenting_count(*c) > 0)
            .max_by_key(|c| world.last_activity(*c))
    }

    fn name(&self) -> &'static str {
        "most_recent_activity"
    }
}
