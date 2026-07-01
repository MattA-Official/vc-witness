# Witness

Witness is a Discord bot for reviewing voice-channel activity. It keeps a rolling audio buffer for
voice channels, transcribes relevant audio with a local Whisper model, and helps moderators
review user-filed reports.

## What it does

- Captures short rolling audio windows from voice channels so the bot can later assemble the
  context around a reported incident.
- Transcribes buffered audio using a local Whisper model when a report is filed.
- Lets users report another person through either the `/report` slash command or the
  "Report VC Activity" user context menu.
- Enforces a consent flow for voice recording:
  - first-time joiners are muted and prompted in DMs to consent
  - users can decline or opt out, which disconnects them and resets their consent state
- Supports GDPR-style data-rights actions:
  - `/data request` shows what data the bot has about the requester
  - `/data erase` clears the consent record and any unreported buffered audio (future)
- Stores per-server configuration in SQLite and exposes it through `/config`.

## Docker

The easiest way to run Witness is the prebuilt image published on every
[release](https://github.com/MattA-Official/vc-witness/releases).

1. Download a Whisper model in ggml format (e.g. `ggml-small.en.bin` from the
   [whisper.cpp model repo](https://huggingface.co/ggerganov/whisper.cpp/tree/main)) and put it in
   a local `models/` folder.
2. Copy `.env.example` to `.env` and fill in `DISCORD_TOKEN`, `GUILD_ID`, and (if your model
   filename isn't `ggml-small.en.bin`) `WHISPER_MODEL_FILE`.
3. Grab `docker-compose.yml` from the latest release (or this repo) and run:

   ```sh
   docker compose up -d
   ```

Witness stores its SQLite database under `./data` and reads the model from `./models`, both bind-
mounted into the container, so upgrading is just pulling a new image tag and restarting.

## From source

### Requirements

- Rust toolchain
- CMake on your PATH (required for the native Whisper build)
- A Whisper model in ggml format, for example `models/ggml-small.en.bin`

### Setup

1. Copy `.env.example` to `.env` and fill in:
   - `DISCORD_TOKEN` - your bot token
   - `GUILD_ID` - the single guild this bot serves
   - `WHISPER_MODEL_PATH` - path to your Whisper model
   - Optional: `DATABASE_PATH` and `WHISPER_MAX_CONCURRENT_JOBS`
2. Place the Whisper model at the path specified by `WHISPER_MODEL_PATH`.
3. Build the bot with `cargo build`.
4. Start it with `cargo run`.

On first run, Witness creates the SQLite database (default: `data/witness.sqlite3`), runs the
migrations, seeds default report categories, and registers the slash commands.

## Commands

- `/report user:<user>` - start a report for the selected user
- `Report VC Activity` (user context menu) - same as above
- `/data request` - show the data Witness has about you
- `/data erase` - erase your consent record and any unreported buffered audio
- `/config show` - show the server's current configuration
- `/config channel <channel>` - set the channel where reports are posted
- `/config role <role>` - set the role that can resolve reports
- `/config buffer <duration>` - set the rolling audio buffer window
- `/config tail <duration>` - set the post-report recording tail
- `/config strategy <mode>` - choose the voice-channel selection strategy
- `/config category list|add|remove|edit ...` - manage report categories

## Notes

Most runtime configuration lives in the database and is edited live through `/config`, rather than
through extra config files.
