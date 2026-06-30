use serenity::model::id::ChannelId;

use crate::voice::strategy::{VcEvent, VcStrategy, VcWorldView};

/// Stays in the current channel until it has zero consenting members, then moves to
/// another active channel. Minimizes rejoin churn at the cost of leaving newly active
/// channels uncovered while the bot is "stuck" elsewhere.
pub struct StickyUntilEmpty;

impl VcStrategy for StickyUntilEmpty {
    fn decide(&self, event: &VcEvent, world: &dyn VcWorldView) -> Option<ChannelId> {
        if let Some(current) = world.current_channel() {
            if world.consenting_count(current) > 0 {
                return Some(current);
            }
        }

        // Current channel (if any) is now empty -- pick a new one to move to.
        let preferred = match event {
            VcEvent::UserJoined { channel, .. } => Some(*channel),
            VcEvent::UserMoved { to, .. } => Some(*to),
            VcEvent::ConsentChanged { channel, .. } => *channel,
            VcEvent::UserLeft { .. } => None,
        };

        if let Some(channel) = preferred {
            if world.consenting_count(channel) > 0 {
                return Some(channel);
            }
        }

        world
            .all_active_channels()
            .into_iter()
            .filter(|c| world.consenting_count(*c) > 0)
            .max_by_key(|c| world.last_activity(*c))
    }

    fn name(&self) -> &'static str {
        "sticky_until_empty"
    }
}
