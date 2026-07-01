use std::sync::Arc;

use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::gateway::client::Context;
use serenity::model::application::{LabelComponent, ModalComponent, ModalInteraction};
use serenity::model::id::UserId;

use crate::discord::state::AppState;
use crate::report::pipeline::{self, ReportRequest};
use crate::report::{CATEGORY_SELECT_ID, DETAILS_INPUT_ID, REPORT_MODAL_PREFIX};

/// Parses `report_modal:<reporter_id>:<target_id>` back out of the modal's custom_id --
/// modals can't carry richer state, and the original command interaction token may have
/// expired by the time the user submits, so this is the only place that context survives.
fn parse_custom_id(custom_id: &str) -> Option<(UserId, UserId)> {
    let rest = custom_id.strip_prefix(REPORT_MODAL_PREFIX)?;
    let mut parts = rest.split(':');
    let reporter: u64 = parts.next()?.parse().ok()?;
    let target: u64 = parts.next()?.parse().ok()?;
    Some((UserId::new(reporter), UserId::new(target)))
}

fn extract_field(components: &[ModalComponent], custom_id: &str) -> Option<String> {
    for component in components {
        let ModalComponent::Label(label) = component else { continue };
        match &label.component {
            // `sm.options` is just the select menu's original definition echoed back
            // unchanged (none of them ever have `default: true`); the actual selection is
            // in `sm.values`, which is only populated on modal submissions. Reading
            // `options` here was why every report was silently falling back to "other".
            LabelComponent::SelectMenu(sm) if sm.custom_id.as_str() == custom_id => {
                return sm.values.first().map(|v| v.to_string());
            }
            LabelComponent::InputText(it) if it.custom_id.as_str() == custom_id => {
                return Some(it.value.to_string());
            }
            _ => {}
        }
    }
    None
}

pub async fn handle(ctx: &Context, state: Arc<AppState>, modal: &ModalInteraction) {
    let Some((reporter_id, target_id)) = parse_custom_id(modal.data.custom_id.as_str()) else { return };

    let category_value = extract_field(&modal.data.components, CATEGORY_SELECT_ID).unwrap_or_else(|| "other".to_string());
    let details_text = extract_field(&modal.data.components, DETAILS_INPUT_ID).unwrap_or_default();

    let ack = modal
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .content("Report received, processing audio and transcript, this may take a moment."),
            ),
        )
        .await;

    if let Err(e) = ack {
        tracing::warn!("failed to ack report modal submission: {e}");
        return;
    }

    let channel_id = match state.vc_manager.world().channel_of(target_id) {
        Some(c) => c,
        None => match crate::db::voice_events::last_left_at(&state.db, target_id).await {
            Ok(Some((c, _))) => c,
            _ => {
                tracing::warn!("report submitted for {target_id} but no current/recent channel found");
                return;
            }
        },
    };

    let req = ReportRequest {
        report_id: uuid::Uuid::new_v4().to_string(),
        reporter_id,
        reported_user_id: target_id,
        channel_id,
        category_value,
        details_text,
    };

    let http = ctx.http.clone();
    tokio::spawn(async move { pipeline::run(http, state, req).await });
}
