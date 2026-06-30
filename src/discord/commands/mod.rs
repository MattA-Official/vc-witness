pub mod config;
pub mod data_erase;
pub mod data_request;
pub mod report;
pub mod report_context_menu;

use serenity::builder::CreateCommand;
use serenity::gateway::client::Context;
use serenity::model::application::{CommandInteraction, ResolvedValue};

use crate::discord::state::AppState;

pub fn all() -> Vec<CreateCommand<'static>> {
    vec![report::register(), report_context_menu::register(), data_request::register(), config::register()]
}

pub async fn dispatch(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    match interaction.data.name.as_str() {
        report::NAME => report::handle(ctx, state, interaction).await,
        n if n == report_context_menu::NAME => report_context_menu::handle(ctx, state, interaction).await,
        data_request::NAME => {
            let sub = interaction.data.options().into_iter().next();
            match sub.map(|o| (o.name, o.value)) {
                Some(("request", ResolvedValue::SubCommand(_))) => data_request::handle_request(ctx, state, interaction).await,
                Some(("erase", ResolvedValue::SubCommand(_))) => data_erase::handle(ctx, state, interaction).await,
                _ => {}
            }
        }
        config::NAME => config::handle(ctx, state, interaction).await,
        other => tracing::warn!("unhandled command: {other}"),
    }
}
