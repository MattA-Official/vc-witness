use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serenity::model::id::UserId;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};

use crate::error::{Result, WitnessError};
use crate::transcription::resample::to_whisper_input;
use crate::transcription::types::TranscriptLine;
use crate::transcription::TranscriptionService;

/// Transcribes one user's finalized WAV (48kHz stereo i16, as written by `voice::finalize`)
/// and returns absolute-timestamped lines tagged with that speaker -- since songbird already
/// demuxes audio per-user, there's no diarization step here, just speech-to-text per stream.
pub async fn transcribe_user_clip(
    service: &TranscriptionService,
    speaker: UserId,
    wav_path: &Path,
    clip_started_at: DateTime<Utc>,
) -> Result<Vec<TranscriptLine>> {
    let permit = service
        .semaphore()
        .acquire_owned()
        .await
        .map_err(|e| WitnessError::Transcription(format!("semaphore closed: {e}")))?;

    let context = service.context();
    let wav_path = wav_path.to_path_buf();

    let result = tokio::task::spawn_blocking(move || run_whisper(&context, &wav_path))
        .await
        .map_err(|e| WitnessError::Transcription(format!("transcription task panicked: {e}")))??;

    drop(permit);

    Ok(result
        .into_iter()
        .map(|(start_ms, end_ms, text)| TranscriptLine {
            speaker,
            start_ms: clip_started_at.timestamp_millis() + start_ms,
            end_ms: clip_started_at.timestamp_millis() + end_ms,
            text,
        })
        .collect())
}

/// Runs synchronously (CPU-bound whisper.cpp inference) -- always called via `spawn_blocking`.
fn run_whisper(context: &WhisperContext, wav_path: &Path) -> Result<Vec<(i64, i64, String)>> {
    let reader = hound::WavReader::open(wav_path)?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let audio = if spec.channels == 2 {
        to_whisper_input(&samples)?
    } else {
        let mut mono = vec![0.0f32; samples.len()];
        whisper_rs::convert_integer_to_float_audio(&samples, &mut mono)
            .map_err(|e| WitnessError::Transcription(format!("{e}")))?;
        mono
    };

    if audio.is_empty() {
        return Ok(Vec::new());
    }

    let mut state = context
        .create_state()
        .map_err(|e| WitnessError::Transcription(format!("failed to create whisper state: {e}")))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
    params.set_n_threads(std::cmp::min(4, num_cpus::get() as i32));
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, &audio)
        .map_err(|e| WitnessError::Transcription(format!("whisper inference failed: {e}")))?;

    Ok(state
        .as_iter()
        .filter_map(|segment| {
            let text = segment.to_string();
            if is_non_speech(&text) {
                return None;
            }
            let start_ms = segment.start_timestamp() * 10;
            let end_ms = segment.end_timestamp() * 10;
            Some((start_ms, end_ms, text))
        })
        .collect())
}

/// whisper.cpp emits bracketed/parenthesized tags like `[BLANK_AUDIO]`, `[SILENCE]`, or
/// `(wind blowing)` for non-verbal stretches instead of an empty string -- without this
/// filter those literally showed up as "**@user**: [BLANK_AUDIO]" in report transcripts,
/// which reads like a bug rather than "no speech here."
fn is_non_speech(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    (trimmed.starts_with('[') && trimmed.ends_with(']')) || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

pub async fn transcribe_all(
    service: Arc<TranscriptionService>,
    clips: Vec<(UserId, std::path::PathBuf, DateTime<Utc>)>,
) -> Vec<TranscriptLine> {
    let mut handles = Vec::new();
    for (speaker, path, started_at) in clips {
        let service = service.clone();
        handles.push(tokio::spawn(async move {
            transcribe_user_clip(&service, speaker, &path, started_at).await
        }));
    }

    let mut all = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(lines)) => all.extend(lines),
            Ok(Err(e)) => tracing::warn!("transcription failed for one participant: {e}"),
            Err(e) => tracing::warn!("transcription task join error: {e}"),
        }
    }
    all
}
