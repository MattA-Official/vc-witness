pub mod pipeline;
pub mod resample;
pub mod types;

use std::path::Path;
use std::sync::Arc;

use tokio::sync::Semaphore;
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::error::{Result, WitnessError};

/// Holds the single whisper.cpp model (expensive to load -- done once at startup, fails
/// fast if the model file is missing) and a semaphore bounding concurrent transcription
/// jobs so whisper inference (CPU-bound) doesn't starve the live voice-receive runtime.
pub struct TranscriptionService {
    context: Arc<WhisperContext>,
    semaphore: Arc<Semaphore>,
}

impl TranscriptionService {
    pub fn load(model_path: &Path, max_concurrent_jobs: usize) -> Result<Self> {
        if !model_path.exists() {
            return Err(WitnessError::Config(format!(
                "whisper model not found at {}, see .env.example for WHISPER_MODEL_PATH",
                model_path.display()
            )));
        }

        let context = WhisperContext::new_with_params(
            model_path.to_string_lossy().into_owned(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| WitnessError::Transcription(format!("failed to load whisper model: {e}")))?;

        Ok(Self {
            context: Arc::new(context),
            semaphore: Arc::new(Semaphore::new(max_concurrent_jobs.max(1))),
        })
    }

    pub fn context(&self) -> Arc<WhisperContext> {
        self.context.clone()
    }

    pub fn semaphore(&self) -> Arc<Semaphore> {
        self.semaphore.clone()
    }
}
