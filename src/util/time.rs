use chrono::{Duration, Utc};

pub fn now() -> chrono::DateTime<Utc> {
    Utc::now()
}

/// Human-readable "Xm Ys" rendering for DM/report text (e.g. buffer/tail durations).
pub fn format_duration(d: Duration) -> String {
    let secs = d.num_seconds().max(0);
    let mins = secs / 60;
    let rem = secs % 60;
    if mins > 0 {
        format!("{mins}m {rem}s")
    } else {
        format!("{rem}s")
    }
}
