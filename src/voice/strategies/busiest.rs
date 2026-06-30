use serenity::model::id::ChannelId;

use crate::voice::strategy::{VcEvent, VcStrategy, VcWorldView};

/// Always moves to whichever active channel currently has the most consenting members,
/// re-evaluating on every event. Maximizes coverage of the busiest conversation but
/// causes the most rejoin churn of the three policies.
pub struct Busiest;

impl VcStrategy for Busiest {
    fn decide(&self, _event: &VcEvent, world: &dyn VcWorldView) -> Option<ChannelId> {
        world
            .all_active_channels()
            .into_iter()
            .map(|c| (c, world.consenting_count(c)))
            .filter(|(_, n)| *n > 0)
            .max_by_key(|(c, n)| (*n, world.last_activity(*c)))
            .map(|(c, _)| c)
    }

    fn name(&self) -> &'static str {
        "busiest"
    }
}
