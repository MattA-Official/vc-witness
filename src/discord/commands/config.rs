use serenity::builder::{
    AutocompleteChoice, CreateAllowedMentions, CreateAutocompleteResponse, CreateCommand, CreateCommandOption, CreateComponent,
    CreateContainer, CreateContainerComponent, CreateInteractionResponse, CreateInteractionResponseMessage, CreateSeparator,
    CreateTextDisplay,
};
use serenity::gateway::client::Context;
use serenity::model::application::{CommandInteraction, CommandOptionType, ResolvedValue};
use serenity::model::channel::MessageFlags;
use serenity::model::colour::Colour;
use serenity::model::permissions::Permissions;

use crate::db::categories::ReportCategory;
use crate::db::{categories, guild_config};
use crate::discord::state::AppState;
use crate::voice::strategy::VcStrategyKind;

/// Category labels/values are moderator-supplied and rendered verbatim in reply text -- deny all
/// mention parsing so a label like `@everyone` can't actually ping anyone.
fn no_mentions<'a>() -> CreateAllowedMentions<'a> {
    CreateAllowedMentions::new()
}

/// Shared rendering for a category line, used by both `/config show` and `/config category list`
/// so the two views don't drift from each other.
fn format_category_line(c: &ReportCategory) -> String {
    match &c.description {
        Some(d) => format!("- **{}** (`{}`) -- {d}", c.label, c.value),
        None => format!("- **{}** (`{}`)", c.label, c.value),
    }
}

pub const NAME: &str = "config";

/// (label, seconds) presets shown in the buffer-duration picker, largest enough to cover an
/// incident unfolding over several minutes before it's reported.
const BUFFER_PRESETS: &[(&str, i64)] = &[
    ("1 minute", 60),
    ("2 minutes", 120),
    ("5 minutes", 300),
    ("10 minutes", 600),
    ("15 minutes", 900),
];

/// (label, seconds) presets shown in the tail-duration picker.
const TAIL_PRESETS: &[(&str, i64)] = &[
    ("15 seconds", 15),
    ("30 seconds", 30),
    ("1 minute", 60),
    ("2 minutes", 120),
    ("5 minutes", 300),
];

/// Renders a stored duration back to its preset label, falling back to raw seconds for values
/// that predate the current preset set (e.g. set before presets changed).
fn duration_label(presets: &[(&str, i64)], secs: i64) -> String {
    presets.iter().find(|(_, s)| *s == secs).map(|(label, _)| label.to_string()).unwrap_or_else(|| format!("{secs}s"))
}

fn int_choice_option(
    name: &'static str,
    description: &'static str,
    presets: &'static [(&'static str, i64)],
) -> CreateCommandOption<'static> {
    let mut opt = CreateCommandOption::new(CommandOptionType::Integer, name, description).required(true);
    for (label, secs) in presets {
        opt = opt.add_int_choice(*label, *secs);
    }
    opt
}

pub fn register() -> CreateCommand<'static> {
    CreateCommand::new(NAME)
        .description("Configure Witness for this server (all configuration lives here, no config files)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "show",
            "Show the current configuration for this server",
        ))
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "channel", "Set the channel reports are posted to")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Channel, "channel", "Reports channel").required(true)),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "role", "Set the role allowed to resolve reports")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Role, "role", "Moderator role").required(true)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "buffer",
                "Set how far back to keep buffered audio (default: 1 minute)",
            )
            .add_sub_option(int_choice_option("duration", "Buffer length", BUFFER_PRESETS)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "tail",
                "Set how long to keep recording after a report (default: 15 seconds)",
            )
            .add_sub_option(int_choice_option("duration", "Tail length", TAIL_PRESETS)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "strategy",
                "Set which voice channel the bot follows when several are active",
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::String, "mode", "Strategy")
                    .required(true)
                    .add_string_choice("Most recent activity", "most_recent_activity")
                    .add_string_choice("Busiest channel", "busiest")
                    .add_string_choice("Sticky until empty", "sticky_until_empty"),
            ),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommandGroup, "category", "Manage report categories")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::SubCommand, "list", "List report categories"))
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "add", "Add a report category")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "label", "Display label").required(true).max_length(100),
                        )
                        .add_sub_option(
                            CreateCommandOption::new(
                                CommandOptionType::String,
                                "description",
                                "Shown to reporters under the label when picking a category",
                            )
                            .required(false)
                            .max_length(100),
                        ),
                )
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "remove", "Remove a category")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "value", "Category to remove")
                                .required(true)
                                .set_autocomplete(true),
                        ),
                )
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "edit", "Edit a category's label or description")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "value", "Category to edit")
                                .required(true)
                                .set_autocomplete(true),
                        )
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "label", "New display label")
                                .required(true)
                                .max_length(100),
                        )
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "description", "New description (leave blank to keep the current one)")
                                .required(false)
                                .max_length(100),
                        ),
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

fn find_string(opts: &Opts<'_>, name: &str) -> Option<String> {
    match find(opts, name) {
        Some(ResolvedValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

pub async fn handle(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let options = interaction.data.options();
    let Some(top) = options.first() else { return };

    if top.name == "show" {
        send_show(ctx, state, interaction).await;
        return;
    }

    let result: crate::error::Result<String> = match (top.name, &top.value) {
        ("channel", ResolvedValue::SubCommand(args)) => match find(args, "channel") {
            Some(ResolvedValue::Channel(channel)) => {
                let channel_id = match channel {
                    serenity::model::channel::GenericInteractionChannel::Channel(c) => c.id,
                    serenity::model::channel::GenericInteractionChannel::Thread(t) => t.parent_id,
                };
                guild_config::set_reports_channel(&state.db, state.guild_id, channel_id)
                    .await
                    .map(|()| format!("Reports channel set to <#{channel_id}>."))
            }
            _ => Ok("Nothing to update.".to_string()),
        },
        ("role", ResolvedValue::SubCommand(args)) => match find(args, "role") {
            Some(ResolvedValue::Role(role)) => guild_config::set_mod_role(&state.db, state.guild_id, role.id)
                .await
                .map(|()| format!("Moderator role set to <@&{}>.", role.id)),
            _ => Ok("Nothing to update.".to_string()),
        },
        ("buffer", ResolvedValue::SubCommand(args)) => match find(args, "duration") {
            Some(ResolvedValue::Integer(secs)) => guild_config::set_buffer_duration(&state.db, state.guild_id, *secs)
                .await
                .map(|()| format!("Buffer duration set to {}.", duration_label(BUFFER_PRESETS, *secs))),
            _ => Ok("Nothing to update.".to_string()),
        },
        ("tail", ResolvedValue::SubCommand(args)) => match find(args, "duration") {
            Some(ResolvedValue::Integer(secs)) => guild_config::set_tail_duration(&state.db, state.guild_id, *secs)
                .await
                .map(|()| format!("Tail duration set to {}.", duration_label(TAIL_PRESETS, *secs))),
            _ => Ok("Nothing to update.".to_string()),
        },
        ("strategy", ResolvedValue::SubCommand(args)) => match find(args, "mode") {
            Some(ResolvedValue::String(mode)) => {
                let kind = VcStrategyKind::from_db_str(mode);
                state.vc_manager.set_strategy(kind);
                guild_config::set_vc_strategy(&state.db, state.guild_id, kind)
                    .await
                    .map(|()| format!("VC strategy set to {}.", kind.display_label()))
            }
            _ => Ok("Nothing to update.".to_string()),
        },
        ("category", ResolvedValue::SubCommandGroup(sub)) => handle_category(state, sub).await,
        _ => Ok("Nothing to update.".to_string()),
    };

    let reply = match result {
        Ok(msg) => msg,
        Err(e) => format!("Failed: {e}"),
    };

    let _ = interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().ephemeral(true).allowed_mentions(no_mentions()).content(reply),
            ),
        )
        .await;
}

async fn send_show(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let container = match build_show_container(state).await {
        Ok(container) => container,
        Err(e) => {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .allowed_mentions(no_mentions())
                            .content(format!("Failed: {e}")),
                    ),
                )
                .await;
            return;
        }
    };

    let _ = interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .allowed_mentions(no_mentions())
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .components(vec![CreateComponent::Container(container)]),
            ),
        )
        .await;
}

async fn build_show_container(state: &AppState) -> crate::error::Result<CreateContainer<'static>> {
    let (config, cats) =
        tokio::join!(guild_config::get_or_init(&state.db, state.guild_id), categories::list_active(&state.db, state.guild_id));
    let config = config?;
    let cats = cats?;

    let channel = config.reports_channel_id.map(|c| format!("<#{c}>")).unwrap_or_else(|| "not set".to_string());
    let mod_role = config.mod_role_id.map(|r| format!("<@&{r}>")).unwrap_or_else(|| "not set".to_string());

    let categories_text =
        if cats.is_empty() { "none configured".to_string() } else { cats.iter().map(format_category_line).collect::<Vec<_>>().join("\n") };

    Ok(CreateContainer::new(vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new("## Current configuration")),
        CreateContainerComponent::Separator(CreateSeparator::new()),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "**Reports channel:** {channel}\n**Moderator role:** {mod_role}\n**VC strategy:** {}",
            config.vc_strategy.display_label(),
        ))),
        CreateContainerComponent::Separator(CreateSeparator::new()),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "**Buffer duration:** {}\n**Tail duration:** {}",
            duration_label(BUFFER_PRESETS, config.buffer_duration_secs),
            duration_label(TAIL_PRESETS, config.post_report_tail_secs),
        ))),
        CreateContainerComponent::Separator(CreateSeparator::new()),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!("**Categories**\n{categories_text}"))),
    ])
    .accent_colour(Colour::BLURPLE))
}

async fn handle_category(state: &AppState, sub: &[serenity::model::application::ResolvedOption<'_>]) -> crate::error::Result<String> {
    let Some(action) = sub.first() else { return Ok("Nothing to update.".to_string()) };

    match &action.value {
        ResolvedValue::SubCommand(args) => match action.name {
            "list" => {
                let cats = categories::list_active(&state.db, state.guild_id).await?;
                if cats.is_empty() {
                    Ok("No categories configured.".to_string())
                } else {
                    let lines = cats.iter().map(format_category_line).collect::<Vec<_>>().join("\n");
                    Ok(format!("Categories:\n{lines}"))
                }
            }
            "add" => {
                let Some(label) = find_string(args, "label") else { return Ok("Nothing to update.".to_string()) };
                let description = find_string(args, "description");
                let value = categories::add_with_generated_value(&state.db, state.guild_id, &label, description.as_deref()).await?;
                Ok(format!("Added category \"{label}\" ({value})."))
            }
            "remove" => {
                let Some(value) = find_string(args, "value") else { return Ok("Nothing to update.".to_string()) };
                categories::remove(&state.db, state.guild_id, &value)
                    .await
                    .map(|()| format!("Removed category \"{value}\"."))
            }
            "edit" => {
                let (Some(value), Some(label)) = (find_string(args, "value"), find_string(args, "label")) else {
                    return Ok("Nothing to update.".to_string());
                };
                let description = find_string(args, "description");
                categories::edit_label(&state.db, state.guild_id, &value, &label, description.as_deref())
                    .await
                    .map(|()| format!("Renamed category \"{value}\" to \"{label}\"."))
            }
            _ => Ok("Nothing to update.".to_string()),
        },
        _ => Ok("Nothing to update.".to_string()),
    }
}

/// Handles autocomplete for the category `value` field on `remove`/`edit`. Suggests active
/// categories matching the partial input by label or value (case-insensitive substring).
pub async fn handle_autocomplete(ctx: &Context, state: &AppState, interaction: &CommandInteraction) {
    let Some(focused) = interaction.data.autocomplete() else { return };
    if focused.name != "value" {
        return;
    }

    let cats = match categories::list_active(&state.db, state.guild_id).await {
        Ok(cats) => cats,
        Err(e) => {
            tracing::warn!("failed to load categories for autocomplete: {e}");
            return;
        }
    };

    let partial = focused.value.to_lowercase();
    let choices: Vec<AutocompleteChoice> = cats
        .iter()
        .filter(|c| partial.is_empty() || c.label.to_lowercase().contains(&partial) || c.value.to_lowercase().contains(&partial))
        .take(25)
        .map(|c| AutocompleteChoice::new(c.label.clone(), c.value.clone()))
        .collect();

    let _ = interaction
        .create_response(&ctx.http, CreateInteractionResponse::Autocomplete(CreateAutocompleteResponse::new().set_choices(choices)))
        .await;
}
