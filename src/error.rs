use thiserror::Error;

#[derive(Debug, Error)]
pub enum WitnessError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("database migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("discord API error: {0}")]
    Serenity(#[from] serenity::Error),

    #[error("voice connection error: {0}")]
    Songbird(String),

    #[error("transcription error: {0}")]
    Transcription(String),

    #[error("audio I/O error: {0}")]
    Audio(#[from] hound::Error),

    #[error("missing or invalid configuration: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, WitnessError>;
