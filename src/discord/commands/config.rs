use serenity::builder::{CreateCommand, CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::gateway::client::Context;
use serenity::model::application::{CommandInteraction, CommandOptionType, ResolvedValue};
use serenity::model::permissions::Permissions;

use crate::db::{categories, guild_config};
use crate::discord::state::AppState;
use crate::voice::strategy::VcStrategyKind;

pub const NAME: &str = "config";

pub fn register() -> CreateCommand<'static> {
    CreateCommand::new(NAME)
        .description("Configure Witness for this server (all configuration lives here, no config files)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "reports-channel", "Set the channel reports are posted to")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Channel, "channel", "Reports channel").required(true)),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "mod-role", "Set the role allowed to resolve reports")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Role, "role", "Moderator role").required(true)),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "buffer-duration", "Set the rolling audio buffer window")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "seconds", "Seconds").required(true)),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "tail-duration", "Set the post-report recording tail")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "seconds", "Seconds").required(true)),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "vc-strategy", "Set which VC-selection policy the bot uses")
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::String, "mode", "Strategy")
                        .required(true)
                        .add_string_choice("most_recent_activity", "most_recent_activity")
                        .add_string_choice("busiest", "busiest")
                        .add_string_choice("sticky_until_empty", "sticky_until_empty"),
                ),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommandGroup, "category", "Manage report categories")
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "add", "Add (or re-enable) a category")
                        .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "label", "Display label").required(true))
                        .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "value", "Stable identifier").required(true)),
                )
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "remove", "Soft-delete a category")
                        .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "value", "Stable identifier").required(true)),
                )
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "edit", "Rename a category's label")
                        .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "value", "Stable identifier").required(true))
                        .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "label", "New display label").required(true)),
                ),
        )
}

type Opts<'a> = [serenity::model::application::ResolvedOption<'a>];

/// Finds a named sub-option's value within a `SubCommand`'s argument list. Every top-level
/// `/config <subcommand>` option arrives as `ResolvedValue::SubCommand(args)` -- its own
/// value is never the option's value directly, only its child args are.
fn find<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a ResolvedValue<'a>> {
    opts.iter().find(|o| o.name == name).map(|o| &o.value)
}

pub async fn handle(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let options = interaction.data.options();
    let Some(top) = options.first() else { return };

    let result = match (top.name, &top.value) {
        ("reports-channel", ResolvedValue::SubCommand(args)) => match find(args, "channel") {
            Some(ResolvedValue::Channel(channel)) => {
                let channel_id = match channel {
                    serenity::model::channel::GenericInteractionChannel::Channel(c) => c.id,
                    serenity::model::channel::GenericInteractionChannel::Thread(t) => t.parent_id,
                };
                guild_config::set_reports_channel(&state.db, state.guild_id, channel_id).await
            }
            _ => Ok(()),
        },
        ("mod-role", ResolvedValue::SubCommand(args)) => match find(args, "role") {
            Some(ResolvedValue::Role(role)) => guild_config::set_mod_role(&state.db, state.guild_id, role.id).await,
            _ => Ok(()),
        },
        ("buffer-duration", ResolvedValue::SubCommand(args)) => match find(args, "seconds") {
            Some(ResolvedValue::Integer(secs)) => guild_config::set_buffer_duration(&state.db, state.guild_id, *secs).await,
            _ => Ok(()),
        },
        ("tail-duration", ResolvedValue::SubCommand(args)) => match find(args, "seconds") {
            Some(ResolvedValue::Integer(secs)) => guild_config::set_tail_duration(&state.db, state.guild_id, *secs).await,
            _ => Ok(()),
        },
        ("vc-strategy", ResolvedValue::SubCommand(args)) => match find(args, "mode") {
            Some(ResolvedValue::String(mode)) => {
                let kind = VcStrategyKind::from_db_str(mode);
                state.vc_manager.set_strategy(kind);
                guild_config::set_vc_strategy(&state.db, state.guild_id, kind).await
            }
            _ => Ok(()),
        },
        ("category", ResolvedValue::SubCommandGroup(sub)) => handle_category(state, sub).await,
        _ => Ok(()),
    };

    let reply = match result {
        Ok(()) => "Updated.".to_string(),
        Err(e) => format!("Failed: {e}"),
    };

    let _ = interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().ephemeral(true).content(reply)),
        )
        .await;
}

async fn handle_category(state: &AppState, sub: &[serenity::model::application::ResolvedOption<'_>]) -> crate::error::Result<()> {
    let Some(action) = sub.first() else { return Ok(()) };

    let opt = |name: &str, opts: &[serenity::model::application::ResolvedOption<'_>]| -> Option<String> {
        opts.iter().find(|o| o.name == name).and_then(|o| match o.value {
            ResolvedValue::String(s) => Some(s.to_string()),
            _ => None,
        })
    };

    match &action.value {
        ResolvedValue::SubCommand(args) => match action.name {
            "add" => {
                let (Some(label), Some(value)) = (opt("label", args), opt("value", args)) else { return Ok(()) };
                categories::add(&state.db, state.guild_id, &label, &value).await
            }
            "remove" => {
                let Some(value) = opt("value", args) else { return Ok(()) };
                categories::remove(&state.db, state.guild_id, &value).await
            }
            "edit" => {
                let (Some(value), Some(label)) = (opt("value", args), opt("label", args)) else { return Ok(()) };
                categories::edit_label(&state.db, state.guild_id, &value, &label).await
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}
