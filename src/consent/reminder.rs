/// Brief reminder sent on every join once a user has already granted consent --
/// deliberately not a re-consent flow, just a pointer to how to opt out.
pub fn reminder_text(custom: Option<&str>) -> String {
    custom.map(str::to_string).unwrap_or_else(|| {
        "Reminder: this server records and transcribes voice channel audio for moderation \
         reports. You previously consented to this. You can opt out at any time with the \
         button below."
            .to_string()
    })
}

pub const CONSENT_PROMPT_TEXT: &str = "## Voice recording consent\n\
You've joined a voice channel on this server. To help moderators review reports of abuse, \
this bot records and transcribes voice channel audio, but **only** for members who consent.\n\n\
You've been server-muted until you respond. If you **consent**, you'll be unmuted immediately \
and your audio may be buffered briefly and used in moderation reports. If you **decline**, \
you'll be disconnected from the voice channel, and no audio of yours is captured.\n\n\
You can opt out again at any time with the button on the reminder you'll get on future joins, \
and request a copy of what's held about you with `/data request`.";
