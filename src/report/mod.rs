pub mod eligibility;
pub mod pipeline;

use serenity::builder::{
    CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage, CreateLabel, CreateModal,
    CreateModalComponent, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption,
};
use serenity::gateway::client::Context;
use serenity::model::application::{CommandInteraction, InputTextStyle};
use serenity::model::id::UserId;

use crate::db::{categories, guild_config};
use crate::discord::state::AppState;
use crate::error::Result;

pub const REPORT_MODAL_PREFIX: &str = "report_modal:";
pub const CATEGORY_SELECT_ID: &str = "report_category";
pub const DETAILS_INPUT_ID: &str = "report_details";

/// Shared by both report entry points (the `/report` slash command and the "Report VC
/// Activity" user context menu) so eligibility + modal logic isn't duplicated.
pub async fn begin_report_flow(ctx: &Context, interaction: &CommandInteraction, state: &AppState, target: UserId) -> Result<()> {
    if target == interaction.user.id {
        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().ephemeral(true).content("You can't report yourself."),
                ),
            )
            .await?;
        return Ok(());
    }

    if target == state.bot_user_id {
        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().ephemeral(true).content("You can't report the bot."),
                ),
            )
            .await?;
        return Ok(());
    }

    let cfg = guild_config::get_or_init(&state.db, state.guild_id).await?;

    if cfg.reports_channel_id.is_none() {
        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .ephemeral(true)
                        .content("Witness hasn't been configured yet, ask a moderator to run `/config channel` first."),
                ),
            )
            .await?;
        return Ok(());
    }

    let eligibility = eligibility::check(&state.db, state.vc_manager.world(), target, cfg.buffer_duration_secs).await?;

    match eligibility {
        eligibility::Eligibility::NotEligible(reason) => {
            interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().ephemeral(true).content(reason),
                    ),
                )
                .await?;
        }
        eligibility::Eligibility::Eligible => {
            let cats = categories::list_active(&state.db, state.guild_id).await?;
            let options: Vec<CreateSelectMenuOption> = cats
                .iter()
                .map(|c| {
                    let opt = CreateSelectMenuOption::new(c.label.clone(), c.value.clone());
                    match &c.description {
                        Some(desc) => opt.description(desc.clone()),
                        None => opt,
                    }
                })
                .collect();

            let custom_id = format!("{REPORT_MODAL_PREFIX}{}:{target}", interaction.user.id);

            let modal = CreateModal::new(custom_id, "Report VC Activity").components(vec![
                CreateModalComponent::Label(CreateLabel::select_menu(
                    "Category",
                    CreateSelectMenu::new(CATEGORY_SELECT_ID, CreateSelectMenuKind::String { options: options.into() }),
                )),
                CreateModalComponent::Label(CreateLabel::input_text(
                    "Details",
                    CreateInputText::new(InputTextStyle::Paragraph, DETAILS_INPUT_ID)
                        .placeholder("What happened?")
                        .required(true),
                )),
            ]);

            interaction.create_response(&ctx.http, CreateInteractionResponse::Modal(modal)).await?;
        }
    }

    Ok(())
}
