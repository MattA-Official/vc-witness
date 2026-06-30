pub mod consent_dm;
pub mod decision_buttons;
pub mod modal_report;

use std::sync::Arc;

use serenity::gateway::client::Context;
use serenity::model::application::Interaction;

use crate::discord::commands;
use crate::discord::state::AppState;
use crate::report::REPORT_MODAL_PREFIX;
use crate::discord::interactions::decision_buttons::NOTE_MODAL_PREFIX;

pub async fn dispatch(ctx: Context, state: Arc<AppState>, interaction: Interaction) {
    match interaction {
        Interaction::Command(command) => commands::dispatch(&ctx, &state, &command).await,
        Interaction::Component(component) => {
            if component.data.custom_id == crate::consent::CONSENT_ACCEPT_ID
                || component.data.custom_id == crate::consent::CONSENT_DECLINE_ID
            {
                consent_dm::handle(&ctx, state, &component).await;
            } else if crate::discord::components::report_message::parse_decision_custom_id(&component.data.custom_id).is_some() {
                decision_buttons::handle(&ctx, &state, &component).await;
            }
        }
        Interaction::Modal(modal) => {
            if modal.data.custom_id.as_str().starts_with(REPORT_MODAL_PREFIX) {
                modal_report::handle(&ctx, state, &modal).await;
            } else if modal.data.custom_id.as_str().starts_with(NOTE_MODAL_PREFIX) {
                decision_buttons::handle_note_modal(&ctx, &state, &modal).await;
            }
        }
        _ => {}
    }
}
