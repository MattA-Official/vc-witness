use std::sync::Arc;

use chrono::Duration;
use serenity::http::Http;
use serenity::model::id::{ChannelId, UserId};

use crate::db::consent::ConsentState;
use crate::db::reports::{self, NewReport, ParticipantRole, Report};
use crate::db::{categories, guild_config};
use crate::discord::components::report_message;
use crate::discord::state::AppState;
use crate::error::Result;
use crate::transcription::pipeline::transcribe_all;
use crate::transcription::types::Transcript;
use crate::voice::finalize;

pub struct ReportRequest {
    pub report_id: String,
    pub reporter_id: UserId,
    pub reported_user_id: UserId,
    pub channel_id: ChannelId,
    pub category_value: String,
    pub details_text: String,
}

/// End-to-end: persist the report row, immediately post a placeholder Components V2 card
/// (so moderators see the report right away instead of waiting out the buffer/tail/
/// transcription pipeline, which can take minutes), then finalize+transcribe audio for
/// every consenting member of the channel (not just the reported user, to preserve
/// context) and edit that same card in place once ready. Run as a spawned background task
/// so the reporter's ephemeral ack isn't blocked on any of this.
pub async fn run(http: Arc<Http>, state: Arc<AppState>, req: ReportRequest) {
    if let Err(e) = run_inner(&http, &state, &req).await {
        tracing::error!("report pipeline failed for report {}: {e}", req.report_id);
    }
}

async fn run_inner(http: &Http, state: &AppState, req: &ReportRequest) -> Result<()> {
    let cats = categories::list_active(&state.db, state.guild_id).await?;
    let category = cats.iter().find(|c| c.value == req.category_value);
    let category_label = category.map(|c| c.label.clone()).unwrap_or_else(|| req.category_value.clone());

    reports::create(
        &state.db,
        &NewReport {
            id: req.report_id.clone(),
            reporter_id: req.reporter_id,
            reported_user_id: req.reported_user_id,
            channel_id: req.channel_id,
            category_id: category.map(|c| c.id),
            category_label_snapshot: category_label,
            details_text: req.details_text.clone(),
        },
    )
    .await?;

    let cfg = guild_config::get_or_init(&state.db, state.guild_id).await?;

    let posted = match cfg.reports_channel_id {
        Some(reports_channel) => {
            let pending = fetch_report(state, &req.report_id).await?;
            let message = report_message::build(
                &pending,
                &format!("<@{}>", req.reporter_id),
                &format!("<@{}>", req.reported_user_id),
                &format!("<#{}>", req.channel_id),
            );
            let sent = reports_channel.widen().send_message(http, message).await?;
            reports::set_message_id(&state.db, &req.report_id, sent.id).await?;
            Some((reports_channel, sent.id))
        }
        None => {
            tracing::warn!("no reports channel configured — report {} filed but not posted", req.report_id);
            None
        }
    };

    let consenting_users: Vec<UserId> = state
        .vc_manager
        .world()
        .members_in(req.channel_id)
        .into_iter()
        .filter(|(_, c)| matches!(c, ConsentState::Granted))
        .map(|(u, _)| u)
        .collect();

    for &user in &consenting_users {
        if user != req.reporter_id && user != req.reported_user_id {
            reports::add_participant(&state.db, &req.report_id, user, ParticipantRole::BystanderRecorded).await?;
        }
    }

    if consenting_users.is_empty() {
        reports::finalize_without_audio(&state.db, &req.report_id).await?;
    } else {
        let since = chrono::Utc::now() - Duration::seconds(cfg.buffer_duration_secs);
        let tail = Duration::seconds(cfg.post_report_tail_secs);
        let finalized = finalize::finalize_window(&state.audio_pool, &consenting_users, since, tail).await;

        if finalized.is_empty() {
            reports::finalize_without_audio(&state.db, &req.report_id).await?;
        } else {
            let report_dir = state.reports_dir.join(&req.report_id);
            let paths = finalize::write_wavs(&report_dir, &finalized)?;

            let clips: Vec<_> = finalized
                .iter()
                .filter_map(|(uid, audio)| paths.get(uid).map(|p| (*uid, p.clone(), audio.started_at)))
                .collect();

            let lines = transcribe_all(state.transcription.clone(), clips).await;
            let transcript = Transcript::merge_sorted(vec![lines]);

            reports::finalize_with_audio(&state.db, &req.report_id, &report_dir.to_string_lossy(), &transcript).await?;
        }
    }

    if let Some((reports_channel, message_id)) = posted {
        let finalized_report = fetch_report(state, &req.report_id).await?;
        let edit = report_message::build_edit(
            &finalized_report,
            &format!("<@{}>", req.reporter_id),
            &format!("<@{}>", req.reported_user_id),
            &format!("<#{}>", req.channel_id),
            None,
        );
        reports_channel.widen().edit_message(http, message_id, edit).await?;
    }

    Ok(())
}

async fn fetch_report(state: &AppState, report_id: &str) -> Result<Report> {
    reports::get(&state.db, report_id)
        .await?
        .ok_or_else(|| crate::error::WitnessError::Config(format!("report {report_id} vanished mid-pipeline")))
}
