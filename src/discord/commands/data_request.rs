use serenity::builder::{CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::gateway::client::Context;
use serenity::model::application::CommandInteraction;

use crate::db::consent;
use crate::db::reports;
use crate::discord::state::AppState;

pub const NAME: &str = "data";

pub fn register() -> CreateCommand<'static> {
    CreateCommand::new(NAME)
        .description("Manage the data this bot holds about you")
        .add_option(
            serenity::builder::CreateCommandOption::new(
                serenity::model::application::CommandOptionType::SubCommand,
                "request",
                "Get a summary of the data held about you",
            ),
        )
        .add_option(
            serenity::builder::CreateCommandOption::new(
                serenity::model::application::CommandOptionType::SubCommand,
                "erase",
                "Erase your consent record and any unreported buffered audio",
            ),
        )
}

pub async fn handle_request(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let user_id = interaction.user.id;

    let consent_state = consent::get_state(&state.db, user_id).await.unwrap_or(consent::ConsentState::Unknown);
    let involved = reports::find_involving(&state.db, user_id).await.unwrap_or_default();

    let consent_str = match consent_state {
        consent::ConsentState::Granted => "granted",
        consent::ConsentState::Pending => "pending (awaiting your response)",
        consent::ConsentState::Unknown => "not given",
    };

    let mut body = format!("**Your consent state:** {consent_str}\n\n**Reports involving you:** {}\n", involved.len());
    for r in involved.iter().take(20) {
        body.push_str(&format!(
            "- `{}` filed {}, category: {}, status: {}\n",
            r.id, r.created_at, r.category_label_snapshot, r.status
        ));
    }
    if involved.is_empty() {
        body.push_str("_(none)_\n");
    }

    let _ = interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().ephemeral(true).content(body)),
        )
        .await;
}
