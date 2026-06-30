use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::gateway::client::Context;
use serenity::model::application::{CommandInteraction, CommandOptionType, ResolvedValue};

use crate::discord::state::AppState;
use crate::report;

pub const NAME: &str = "report";

pub fn register() -> CreateCommand<'static> {
    CreateCommand::new(NAME)
        .description("Report VC activity by a user currently in (or recently in) a voice channel")
        .add_option(
            CreateCommandOption::new(CommandOptionType::User, "user", "The user to report").required(true),
        )
}

pub async fn handle(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let target = interaction.data.options().into_iter().find_map(|o| match o.value {
        ResolvedValue::User(user, _) => Some(user.id),
        _ => None,
    });

    let Some(target) = target else { return };

    if let Err(e) = report::begin_report_flow(ctx, interaction, state, target).await {
        tracing::warn!("report command failed: {e}");
    }
}
