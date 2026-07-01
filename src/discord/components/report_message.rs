use serenity::builder::{
    CreateActionRow, CreateAttachment, CreateButton, CreateComponent, CreateContainer, CreateContainerComponent,
    CreateFile, CreateMessage, CreateTextDisplay, CreateUnfurledMediaItem, EditAttachments, EditMessage,
};
use serenity::model::application::ButtonStyle;
use serenity::model::channel::MessageFlags;
use serenity::model::id::{ChannelId, UserId};

use crate::db::reports::Report;
use crate::transcription::types::Transcript;

pub const ACTION_TAKEN_PREFIX: &str = "decision:action_taken:";
pub const NO_ACTION_PREFIX: &str = "decision:no_action:";
pub const DISMISS_PREFIX: &str = "decision:dismiss:";

/// Discord's per-message upload budget for a non-boosted guild is 25MB (and as low as 8MB on
/// some older/lower-tier guilds); this is kept well under either so a report never again
/// hard-fails with "Request entity too large" regardless of the guild's boost level, even
/// when several bystanders' recordings are attached to the same message.
const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 7 * 1024 * 1024;

/// Present once a moderator has resolved the report -- when set, the decision buttons are
/// rendered disabled and a resolution line is appended, but everything else about the card
/// (details, transcript, audio) is left exactly as it was so the record is preserved.
pub struct Resolution<'a> {
    pub moderator_mention: String,
    pub decision_label: &'static str,
    pub note: Option<&'a str>,
}

/// Renders the report body (text + audio + moderator-decision buttons) and gathers whatever
/// recordings fit the attachment budget. Shared by `build` (the initial post), `build_edit`
/// (updating that same post once audio/transcript finalize), and again once a moderator
/// resolves it -- all three need identical content, only the outer builder and resolution
/// state differ.
fn render(
    report: &Report,
    reporter_mention: &str,
    reported_mention: &str,
    channel_mention: &str,
    resolution: Option<&Resolution<'_>>,
) -> (Vec<CreateComponent<'static>>, Vec<CreateAttachment<'static>>) {
    let filed_at = chrono::DateTime::parse_from_rfc3339(&report.created_at)
        .map(|d| format!("<t:{}:f> (<t:{}:R>)", d.timestamp(), d.timestamp()))
        .unwrap_or_else(|_| report.created_at.clone());

    let mut lines = vec![
        format!("### New VC report — {}", report.category_label_snapshot),
        format!("**Reporter:** {reporter_mention}"),
        format!("**Reported user:** {reported_mention}"),
        format!("**Channel:** {channel_mention}"),
        format!("**Filed:** {filed_at}"),
        String::new(),
        format!("**Details:**\n{}", report.details_text),
        String::new(),
    ];

    let mut attachments = Vec::new();
    let mut audio_files = Vec::new();

    if report.finalized_at.is_none() {
        lines.push("**Transcript:** _Recording and transcript are still being processed — this card will update shortly._".to_string());
    } else if report.has_audio {
        let transcript = report
            .transcript_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<Transcript>(j).ok())
            .unwrap_or_default();
        let rendered = transcript.render(|uid: UserId| format!("<@{uid}>"));
        lines.push(format!("**Transcript:**\n{rendered}"));

        if let Some(dir) = &report.audio_dir {
            let mut skipped_files = Vec::new();
            let mut total_bytes: u64 = 0;
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("wav") {
                    continue;
                }
                let Some(filename) = path.file_name().map(|f| f.to_string_lossy().into_owned()) else { continue };
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

                if total_bytes + size > MAX_TOTAL_ATTACHMENT_BYTES {
                    tracing::warn!("omitting {filename} from report {} — would exceed the attachment budget", report.id);
                    skipped_files.push(filename);
                    continue;
                }

                if let Ok(bytes) = std::fs::read(&path) {
                    total_bytes += size;
                    audio_files.push(filename.clone());
                    attachments.push(CreateAttachment::bytes(bytes, filename));
                }
            }

            if !skipped_files.is_empty() {
                lines.push(format!(
                    "\n_{} recording(s) were too large to attach here; they're still stored on disk: {}._",
                    skipped_files.len(),
                    skipped_files.join(", ")
                ));
            }
        }
    } else {
        lines.push("**Transcript:** _No audio was captured for this report (the bot was not recording this channel)._".to_string());
    }

    if let Some(res) = resolution {
        let note_suffix = res.note.map(|n| format!(" — {n}")).unwrap_or_default();
        lines.push(format!("\n**Resolved by {}:** {}{note_suffix}", res.moderator_mention, res.decision_label));
    }

    let text = lines.join("\n");
    let report_id = report.id.clone();
    let disabled = resolution.is_some();

    let mut container_components = vec![CreateContainerComponent::TextDisplay(CreateTextDisplay::new(text))];
    // Embed each recording inline in the container (rather than as a bare message attachment)
    // via the Components V2 `File` component, which references the actual uploaded attachment
    // by its `attachment://` URL.
    for filename in &audio_files {
        container_components.push(CreateContainerComponent::File(CreateFile::new(CreateUnfurledMediaItem::new(
            format!("attachment://{filename}"),
        ))));
    }

    let components = vec![
        CreateComponent::Container(CreateContainer::new(container_components)),
        CreateComponent::ActionRow(CreateActionRow::buttons(vec![
            CreateButton::new(format!("{ACTION_TAKEN_PREFIX}{report_id}"))
                .label("Action Taken")
                .style(ButtonStyle::Success)
                .disabled(disabled),
            CreateButton::new(format!("{NO_ACTION_PREFIX}{report_id}"))
                .label("No Action Taken")
                .style(ButtonStyle::Secondary)
                .disabled(disabled),
            CreateButton::new(format!("{DISMISS_PREFIX}{report_id}"))
                .label("Dismiss")
                .style(ButtonStyle::Danger)
                .disabled(disabled),
        ])),
    ];

    (components, attachments)
}

/// Builds the Components V2 report card posted to the configured reports channel as soon
/// as the report exists — before audio/transcript are ready, so moderators see it
/// immediately instead of waiting out the buffer/tail/transcription pipeline.
pub fn build(report: &Report, reporter_mention: &str, reported_mention: &str, channel_mention: &str) -> CreateMessage<'static> {
    let (components, attachments) = render(report, reporter_mention, reported_mention, channel_mention, None);
    CreateMessage::new().flags(MessageFlags::IS_COMPONENTS_V2).components(components).add_files(attachments)
}

/// Updates the already-posted report card in place -- either once audio/transcript finalize
/// (`resolution: None`) or once a moderator resolves it (`resolution: Some(..)`, which
/// disables the decision buttons and appends the outcome while preserving everything else).
pub fn build_edit(
    report: &Report,
    reporter_mention: &str,
    reported_mention: &str,
    channel_mention: &str,
    resolution: Option<&Resolution<'_>>,
) -> EditMessage<'static> {
    let (components, attachments) = render(report, reporter_mention, reported_mention, channel_mention, resolution);
    let mut edit_attachments = EditAttachments::new();
    for attachment in attachments {
        edit_attachments = edit_attachments.add(attachment);
    }
    EditMessage::new().flags(MessageFlags::IS_COMPONENTS_V2).components(components).attachments(edit_attachments)
}

pub fn parse_decision_custom_id(custom_id: &str) -> Option<(&'static str, String)> {
    for (prefix, kind) in [
        (ACTION_TAKEN_PREFIX, "action_taken"),
        (NO_ACTION_PREFIX, "no_action"),
        (DISMISS_PREFIX, "dismiss"),
    ] {
        if let Some(id) = custom_id.strip_prefix(prefix) {
            return Some((kind, id.to_string()));
        }
    }
    None
}

#[allow(dead_code)]
pub fn channel_mention(channel: ChannelId) -> String {
    format!("<#{channel}>")
}
