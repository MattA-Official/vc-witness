use serenity::builder::{
    CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage, CreateLabel, CreateModal,
    CreateModalComponent,
};
use serenity::gateway::client::Context;
use serenity::model::application::{ComponentInteraction, InputTextStyle, LabelComponent, ModalComponent, ModalInteraction};
use serenity::model::id::{MessageId, UserId};

use crate::db::decisions::{self, Decision};
use crate::db::guild_config;
use crate::db::reports::{self, Report, ReportStatus};
use crate::discord::components::report_message::{self, Resolution};
use crate::discord::state::AppState;

pub const NOTE_MODAL_PREFIX: &str = "decision_note:";
const NOTE_INPUT_ID: &str = "decision_note_input";

fn kind_to_decision(kind: &str) -> Decision {
    match kind {
        "action_taken" => Decision::ActionTaken,
        "no_action" => Decision::NoActionTaken,
        _ => Decision::Dismissed,
    }
}

fn decision_to_status(decision: Decision) -> ReportStatus {
    match decision {
        Decision::ActionTaken => ReportStatus::ActionTaken,
        Decision::NoActionTaken => ReportStatus::NoActionTaken,
        Decision::Dismissed => ReportStatus::Dismissed,
    }
}

async fn moderator_role_ok(state: &AppState, member_roles: Option<&[serenity::model::id::RoleId]>) -> bool {
    match guild_config::get_or_init(&state.db, state.guild_id).await {
        Ok(cfg) => match cfg.mod_role_id {
            Some(role) => member_roles.map(|roles| roles.contains(&role)).unwrap_or(false),
            None => true, // no mod role configured yet — don't lock everyone out
        },
        Err(_) => false,
    }
}

/// A click on Action Taken / No Action Taken / Dismiss. Discord has no native ACL for
/// message-component buttons (only for top-level commands), so the moderator-role check
/// happens here in code against `guild_config.mod_role_id`.
pub async fn handle(ctx: &Context, state: &AppState, interaction: &ComponentInteraction) {
    let Some((kind, report_id)) = report_message::parse_decision_custom_id(&interaction.data.custom_id) else { return };

    let roles = interaction.member.as_ref().map(|m| m.roles.as_slice());
    if !moderator_role_ok(state, roles).await {
        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().ephemeral(true).content("Only moderators can resolve reports."),
                ),
            )
            .await;
        return;
    }

    if kind_to_decision(kind) == Decision::Dismissed {
        if let Err(e) = finalize_decision(ctx, state, interaction.user.id, interaction.channel_id, &report_id, Decision::Dismissed, None).await {
            tracing::warn!("failed to finalize dismiss decision: {e}");
        }
        let _ = interaction.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await;
        return;
    }

    // Action Taken / No Action Taken: collect an optional short mod note before DMing the
    // reporter, so it can be sent in the same round trip rather than a second interaction.
    let custom_id = format!("{NOTE_MODAL_PREFIX}{kind}:{report_id}");
    let modal = CreateModal::new(custom_id, "Resolve report").components(vec![CreateModalComponent::Label(
        CreateLabel::input_text("Note to reporter (optional)", CreateInputText::new(InputTextStyle::Paragraph, NOTE_INPUT_ID).required(false)),
    )]);

    let _ = interaction.create_response(&ctx.http, CreateInteractionResponse::Modal(modal)).await;
}

pub async fn handle_note_modal(ctx: &Context, state: &AppState, modal: &ModalInteraction) {
    let Some(rest) = modal.data.custom_id.strip_prefix(NOTE_MODAL_PREFIX) else { return };
    let Some((kind, report_id)) = rest.split_once(':') else { return };
    let decision = kind_to_decision(kind);

    let note = modal
        .data
        .components
        .iter()
        .find_map(|c| match c {
            ModalComponent::Label(l) => match &l.component {
                LabelComponent::InputText(it) if it.custom_id.as_str() == NOTE_INPUT_ID => Some(it.value.to_string()),
                _ => None,
            },
            _ => None,
        })
        .filter(|s| !s.is_empty());

    if let Err(e) = finalize_decision(ctx, state, modal.user.id, modal.channel_id, report_id, decision, note.as_deref()).await {
        tracing::warn!("failed to finalize decision: {e}");
    }

    let _ = modal
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().ephemeral(true).content("Recorded.")),
        )
        .await;
}

/// Records the decision, then edits the original report card in place -- disabling the
/// decision buttons and appending the outcome -- rather than posting a separate message,
/// so the full report (details, transcript, audio) stays visible alongside its resolution.
async fn finalize_decision(
    ctx: &Context,
    state: &AppState,
    moderator_id: UserId,
    channel_id: serenity::model::id::GenericChannelId,
    report_id: &str,
    decision: Decision,
    note: Option<&str>,
) -> crate::error::Result<()> {
    decisions::insert(&state.db, report_id, moderator_id, decision, note).await?;
    reports::set_status(&state.db, report_id, decision_to_status(decision)).await?;

    let Some(report) = reports::get(&state.db, report_id).await? else { return Ok(()) };

    if let Err(e) = edit_report_card(ctx, &report, channel_id, moderator_id, decision, note).await {
        tracing::warn!("failed to update report card for {report_id}: {e}");
    }

    if decision.notifies_reporter() {
        if let Ok(reporter_id) = report.reporter_id.parse::<u64>() {
            let reporter_id = UserId::new(reporter_id);
            if let Ok(dm) = reporter_id.create_dm_channel(&ctx.http).await {
                let body = match note {
                    Some(n) => format!("Your VC report has been resolved: **{}**.\n\nModerator note: {n}", decision.display_label()),
                    None => format!("Your VC report has been resolved: **{}**.", decision.display_label()),
                };
                let _ = dm.id.widen().say(&ctx.http, body).await;
            }
        }
    }

    Ok(())
}

async fn edit_report_card(
    ctx: &Context,
    report: &Report,
    channel_id: serenity::model::id::GenericChannelId,
    moderator_id: UserId,
    decision: Decision,
    note: Option<&str>,
) -> crate::error::Result<()> {
    let Some(message_id) = report.report_message_id.as_deref().and_then(|s| s.parse::<u64>().ok()) else {
        return Ok(());
    };

    let resolution = Resolution { moderator_mention: format!("<@{moderator_id}>"), decision_label: decision.display_label(), note };

    let edit = report_message::build_edit(
        report,
        &format!("<@{}>", report.reporter_id),
        &format!("<@{}>", report.reported_user_id),
        &format!("<#{}>", report.channel_id),
        Some(&resolution),
    );

    channel_id.edit_message(&ctx.http, MessageId::new(message_id), edit).await?;
    Ok(())
}
