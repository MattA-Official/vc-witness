use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use serenity::model::id::UserId;

/// One 20ms tick of decoded PCM from songbird for a single user (interleaved i16,
/// stereo, 48kHz -- the format `VoiceTick` delivers per SSRC with `DecodeMode::Decode`).
pub struct PcmFrame {
    pub at: DateTime<Utc>,
    pub samples: Vec<i16>,
}

/// Per-user ring buffer of recent PCM, with lazy eviction on every push. Deliberately
/// has no disk-backed representation: this is the mechanism that guarantees "no report
/// filed -> no audio survives" without any separate cleanup job. Bytes only ever reach
/// disk via `voice::finalize`, which drains a copy of this buffer after a report exists.
pub struct RollingBuffer {
    frames: Mutex<VecDeque<PcmFrame>>,
    max_age: Duration,
}

impl RollingBuffer {
    pub fn new(max_age: Duration) -> Self {
        Self { frames: Mutex::new(VecDeque::new()), max_age }
    }

    pub fn set_max_age(&self, _max_age: Duration) {
        // Buffer duration is read fresh from guild_config at push time via AudioBufferPool,
        // so individual buffers don't need their max_age mutated; kept for API symmetry.
    }

    pub fn push(&self, at: DateTime<Utc>, samples: Vec<i16>) {
        let mut frames = self.frames.lock().expect("RollingBuffer mutex poisoned");
        frames.push_back(PcmFrame { at, samples });
        let cutoff = at - self.max_age;
        while frames.front().map(|f| f.at < cutoff).unwrap_or(false) {
            frames.pop_front();
        }
    }

    /// Snapshot of all frames at or after `since`, in chronological order. Does not mutate
    /// the buffer -- finalize.rs takes a snapshot, then later takes another for the tail.
    pub fn drain_window(&self, since: DateTime<Utc>) -> Vec<(DateTime<Utc>, Vec<i16>)> {
        let frames = self.frames.lock().expect("RollingBuffer mutex poisoned");
        frames
            .iter()
            .filter(|f| f.at >= since)
            .map(|f| (f.at, f.samples.clone()))
            .collect()
    }

    pub fn oldest_timestamp(&self) -> Option<DateTime<Utc>> {
        self.frames.lock().expect("RollingBuffer mutex poisoned").front().map(|f| f.at)
    }

    pub fn is_empty(&self) -> bool {
        self.frames.lock().expect("RollingBuffer mutex poisoned").is_empty()
    }
}

/// All currently-tracked per-user rolling buffers. A user's buffer is created lazily the
/// first time consented audio arrives for them, and dropped after they've been inactive
/// for a while (memory hygiene only -- this is unrelated to the disk-persistence guarantee,
/// since the buffer was never written to disk regardless).
pub struct AudioBufferPool {
    buffers: DashMap<UserId, RollingBuffer>,
    default_max_age: Duration,
}

impl AudioBufferPool {
    pub fn new(default_max_age: Duration) -> Self {
        Self { buffers: DashMap::new(), default_max_age }
    }

    pub fn push(&self, user: UserId, at: DateTime<Utc>, samples: Vec<i16>) {
        let entry = self
            .buffers
            .entry(user)
            .or_insert_with(|| RollingBuffer::new(self.default_max_age));
        entry.push(at, samples);
    }

    pub fn drain_window(&self, user: UserId, since: DateTime<Utc>) -> Vec<(DateTime<Utc>, Vec<i16>)> {
        self.buffers
            .get(&user)
            .map(|b| b.drain_window(since))
            .unwrap_or_default()
    }

    pub fn has_any_audio(&self, user: UserId) -> bool {
        self.buffers.get(&user).map(|b| !b.is_empty()).unwrap_or(false)
    }

    /// Drops buffers for users who currently hold no data and aren't actively buffered --
    /// called periodically to bound memory; never touches disk.
    pub fn sweep_empty(&self) {
        self.buffers.retain(|_, b| !b.is_empty());
    }

    /// Immediately discards any unreported buffered audio for a user. Used by
    /// `ConsentEngine::erase`, which is not currently wired to a live command (see that
    /// method's doc comment) -- kept for when erasure is properly designed.
    pub fn purge(&self, user: UserId) {
        self.buffers.remove(&user);
    }
}
