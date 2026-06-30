pub mod commands;
pub mod components;
pub mod interactions;
pub mod state;
pub mod voice_state_listener;

use std::sync::Arc;

use serenity::async_trait;
use serenity::gateway::client::{Context, EventHandler};
use serenity::model::event::FullEvent;

use crate::discord::state::AppState;

pub struct Handler {
    pub state: Arc<AppState>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) {
        match event {
            FullEvent::Ready { data_about_bot } => {
                tracing::info!("{} is connected", data_about_bot.user.name);

                let commands = commands::all();
                if let Err(e) = self.state.guild_id.set_commands(&ctx.http, &commands).await {
                    tracing::error!("failed to register guild commands: {e}");
                }
            }
            FullEvent::VoiceStateUpdate { old, new } => {
                voice_state_listener::handle(ctx, &self.state, old.clone(), new).await;
            }
            FullEvent::InteractionCreate { interaction } => {
                interactions::dispatch(ctx.clone(), self.state.clone(), interaction.clone()).await;
            }
            _ => {}
        }
    }
}
