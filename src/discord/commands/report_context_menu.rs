use serenity::builder::CreateCommand;
use serenity::gateway::client::Context;
use serenity::model::application::{CommandInteraction, CommandType, ResolvedTarget};

use crate::discord::state::AppState;
use crate::report;

pub const NAME: &str = "Report VC Activity";

pub fn register() -> CreateCommand<'static> {
    CreateCommand::new(NAME).kind(CommandType::User)
}

pub async fn handle(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let target = match interaction.data.target() {
        Some(ResolvedTarget::User(user, _)) => user.id,
        _ => return,
    };

    if let Err(e) = report::begin_report_flow(ctx, interaction, state, target).await {
        tracing::warn!("report context menu failed: {e}");
    }
}
