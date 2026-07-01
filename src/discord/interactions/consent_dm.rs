use std::sync::Arc;

use serenity::builder::{
    CreateActionRow, CreateButton, CreateComponent, CreateContainer, CreateContainerComponent, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateTextDisplay,
};
use serenity::gateway::client::Context;
use serenity::model::application::{ButtonStyle, ComponentInteraction};
use serenity::model::channel::MessageFlags;

use crate::consent::{CONSENT_ACCEPT_ID, CONSENT_DECLINE_ID};
use crate::discord::state::AppState;

/// Handles the Consent/Decline buttons on the DM sent to a user on their first VC join, and
/// the "Opt out" button on later reminder DMs and on the post-consent confirmation itself
/// (all the same `CONSENT_DECLINE_ID` handler, since opting out always means the same thing:
/// disconnect + reset consent state).
/// DMs have no guild context, but this bot only ever serves one guild, so `state.guild_id`
/// is always the right one.
pub async fn handle(ctx: &Context, state: Arc<AppState>, interaction: &ComponentInteraction) {
    let user_id = interaction.user.id;

    let (response_text, show_opt_out, result) = if interaction.data.custom_id == CONSENT_ACCEPT_ID {
        let r = state.consent_engine.handle_accept(&ctx.http, state.guild_id, user_id, &state.vc_manager).await;
        ("Thanks — you're unmuted and can speak freely. You can opt out at any time with the button below.", true, r)
    } else if interaction.data.custom_id == CONSENT_DECLINE_ID {
        let r = state.consent_engine.handle_decline(&ctx.http, state.guild_id, user_id).await;
        ("Understood — you've been disconnected from the voice channel and no audio of yours was kept.", false, r)
    } else {
        return;
    };

    if let Err(e) = result {
        tracing::warn!("consent button handling failed for {user_id}: {e}");
    }

    // Replace the prompt in place (rather than leaving it with live buttons and posting a
    // separate reply) so the DM thread doesn't accumulate stale, already-acted-on prompts --
    // the closest approximation to "ephemeral" available for a proactively-sent DM, since
    // Discord's ephemeral flag only applies to responses within the interaction that
    // triggered them, not to messages a bot sends unprompted.
    let mut components = vec![CreateComponent::Container(CreateContainer::new(vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(response_text)),
    ]))];

    if show_opt_out {
        components.push(CreateComponent::ActionRow(CreateActionRow::buttons(vec![CreateButton::new(CONSENT_DECLINE_ID)
            .label("Opt out")
            .style(ButtonStyle::Danger)])));
    }

    let confirmation = CreateInteractionResponseMessage::new().flags(MessageFlags::IS_COMPONENTS_V2).components(components);

    let _ = interaction.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(confirmation)).await;
}
