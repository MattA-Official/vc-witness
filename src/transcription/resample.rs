use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

use crate::error::{Result, WitnessError};

const SOURCE_RATE: usize = 48_000;
const TARGET_RATE: usize = 16_000;

/// Downmixes interleaved stereo i16 @ 48kHz to mono f32 in `[-1.0, 1.0]`, then resamples
/// to 16kHz -- the exact format `whisper-rs` expects from `WhisperState::full`.
pub fn to_whisper_input(stereo_i16_48k: &[i16]) -> Result<Vec<f32>> {
    let mono: Vec<f32> = stereo_i16_48k
        .chunks_exact(2)
        .map(|pair| (pair[0] as f32 + pair[1] as f32) / 2.0 / i16::MAX as f32)
        .collect();

    if mono.is_empty() {
        return Ok(Vec::new());
    }

    resample_48k_to_16k(&mono)
}

/// Same downmix+resample as `to_whisper_input`, but returns 16-bit PCM instead of float --
/// used for the archived/attached WAV, since a raw 48kHz-stereo capture of a multi-minute
/// buffer is far too large for a Discord attachment (interleaved stereo @48kHz is 6x the
/// bytes/second of mono @16kHz) and 16kHz mono is already what whisper transcribes from,
/// so there's no quality lost in the report review path by storing at this resolution too.
pub fn downsample_for_storage(stereo_i16_48k: &[i16]) -> Result<Vec<i16>> {
    let mono_16k = to_whisper_input(stereo_i16_48k)?;
    Ok(mono_16k.into_iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect())
}

fn resample_48k_to_16k(mono_48k: &[f32]) -> Result<Vec<f32>> {
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    let ratio = TARGET_RATE as f64 / SOURCE_RATE as f64;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, mono_48k.len(), 1)
        .map_err(|e| WitnessError::Transcription(format!("failed to build resampler: {e}")))?;

    let input = vec![mono_48k.to_vec()];
    let output = resampler
        .process(&input, None)
        .map_err(|e| WitnessError::Transcription(format!("resampling failed: {e}")))?;

    Ok(output.into_iter().next().unwrap_or_default())
}
