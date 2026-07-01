use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serenity::model::id::UserId;

use crate::error::Result;
use crate::transcription::resample::downsample_for_storage;
use crate::voice::buffer::AudioBufferPool;

/// Per-user PCM finalized for a report: interleaved i16 stereo @ 48kHz, plus the
/// timestamp of the very first sample (needed later to convert whisper's segment-relative
/// offsets into absolute timestamps so different users' transcript lines can be merged
/// in true chronological order).
pub struct FinalizedAudio {
    pub started_at: DateTime<Utc>,
    pub samples: Vec<i16>,
}

/// Drains the rolling buffer for each given user from `since` onward, then -- after
/// `tail_duration` has elapsed -- drains whatever new audio has accumulated since and
/// appends it, capturing the "post-report tail" of immediate follow-up. This is the
/// only place in the codebase where buffered audio is read out for persistence; nothing
/// here writes to disk yet (that's `write_wavs`), so a cancelled/erased report still
/// leaves no trace beyond this in-memory snapshot.
pub async fn finalize_window(
    pool: &AudioBufferPool,
    users: &[UserId],
    since: DateTime<Utc>,
    tail_duration: Duration,
) -> HashMap<UserId, FinalizedAudio> {
    let mut accum: HashMap<UserId, FinalizedAudio> = HashMap::new();
    let mut last_seen: HashMap<UserId, DateTime<Utc>> = HashMap::new();

    for &user in users {
        let frames = pool.drain_window(user, since);
        if let Some(first_at) = frames.first().map(|f| f.0) {
            last_seen.insert(user, frames.last().map(|f| f.0).unwrap_or(first_at));
            let mut samples = Vec::new();
            for (_, mut s) in frames {
                samples.append(&mut s);
            }
            accum.insert(user, FinalizedAudio { started_at: first_at, samples });
        }
    }

    if tail_duration > Duration::zero() {
        tokio::time::sleep(tail_duration.to_std().unwrap_or_default()).await;

        for &user in users {
            let cutoff = last_seen.get(&user).copied().unwrap_or(since);
            let tail_frames = pool.drain_window(user, cutoff);
            // drain_window is inclusive of `cutoff`; skip the frame we've already counted.
            let tail_frames: Vec<_> = tail_frames.into_iter().filter(|(at, _)| *at > cutoff).collect();
            if tail_frames.is_empty() {
                continue;
            }
            let entry = accum.entry(user).or_insert_with(|| FinalizedAudio {
                started_at: tail_frames[0].0,
                samples: Vec::new(),
            });
            for (_, mut s) in tail_frames {
                entry.samples.append(&mut s);
            }
        }
    }

    accum
}

/// Writes one WAV file per user under `report_dir/<report_id>/<user_id>.wav`. Downsampled to
/// mono 16kHz (see `downsample_for_storage`) rather than kept at the native 48kHz-stereo
/// capture rate -- a raw multi-minute stereo capture is easily tens of megabytes, which
/// blew through Discord's attachment size limit and made the whole report silently fail to
/// post. 16kHz mono is already whisper's input resolution, so nothing is lost for review.
pub fn write_wavs(report_dir: &Path, audio: &HashMap<UserId, FinalizedAudio>) -> Result<HashMap<UserId, PathBuf>> {
    std::fs::create_dir_all(report_dir)?;
    let mut paths = HashMap::new();

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    for (user, finalized) in audio {
        let samples = downsample_for_storage(&finalized.samples)?;

        let path = report_dir.join(format!("{user}.wav"));
        let mut writer = hound::WavWriter::create(&path, spec)?;
        for sample in &samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;

        paths.insert(*user, path);
    }

    Ok(paths)
}
