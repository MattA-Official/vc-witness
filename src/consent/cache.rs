use dashmap::DashMap;
use serenity::model::id::UserId;

use crate::db::consent::ConsentState;

/// In-memory mirror of `user_consent.state`, kept current by `ConsentEngine` so the
/// voice receive hot path (`voice::receiver`) never needs to hit the database per audio
/// tick. This is the enforcement point referenced throughout the design: a tick is only
/// ever buffered if `get(user) == Granted`.
#[derive(Default)]
pub struct ConsentCache {
    inner: DashMap<UserId, ConsentState>,
}

impl ConsentCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, user: UserId) -> ConsentState {
        self.inner.get(&user).map(|v| *v).unwrap_or(ConsentState::Unknown)
    }

    pub fn set(&self, user: UserId, state: ConsentState) {
        self.inner.insert(user, state);
    }

    pub fn clear(&self, user: UserId) {
        self.inner.remove(&user);
    }

    pub fn is_granted(&self, user: UserId) -> bool {
        matches!(self.get(user), ConsentState::Granted)
    }
}
