use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::gateway::client::Context;
use serenity::model::application::CommandInteraction;

use crate::discord::state::AppState;

/// `/data erase`: deliberately a placeholder for now, not wired to `ConsentEngine::erase`.
/// Letting users erase their own consent/buffer data on demand would be a moderation
/// loophole -- a user named in an in-flight report could call this mid-pipeline to try to
/// scrub the audio already being processed against them before it's persisted (the
/// rolling buffer/finalize pipeline has no way to "unsee" data already copied out for a
/// report by the time an erase request lands). Erasure needs a proper design (e.g. only
/// honoring it once no report referencing the user is mid-flight) before going live.
pub async fn handle(ctx: &Context, _state: &AppState, interaction: &CommandInteraction) {
    let body = "Data erasure requests aren't self-service yet, we're working on this. \
                Please open a support ticket with a moderator and we'll handle your request manually.";

    let _ = interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().ephemeral(true).content(body)),
        )
        .await;
}
