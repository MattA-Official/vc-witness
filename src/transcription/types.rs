use serde::{Deserialize, Serialize};
use serenity::model::id::UserId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptLine {
    pub speaker: UserId,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// A full report transcript: all participants' lines merged and sorted chronologically.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    pub lines: Vec<TranscriptLine>,
}

impl Transcript {
    pub fn merge_sorted(mut per_user: Vec<Vec<TranscriptLine>>) -> Self {
        let mut lines: Vec<TranscriptLine> = per_user.drain(..).flatten().collect();
        lines.sort_by_key(|l| l.start_ms);
        Transcript { lines }
    }

    /// Plain-text rendering for the Components V2 message / report export.
    pub fn render(&self, resolve_name: impl Fn(UserId) -> String) -> String {
        if self.lines.is_empty() {
            return "*(no speech detected)*".to_string();
        }
        self.lines
            .iter()
            .map(|l| format!("**{}**: {}", resolve_name(l.speaker), l.text.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
