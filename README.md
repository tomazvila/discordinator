# Discordinator

A Discord TUI (Terminal User Interface) client written in Rust with tmux-like split pane support. Monitor and participate in multiple conversations simultaneously across different servers — all from a single terminal window.

> **Disclaimer**: Discordinator uses the Discord User API (the same interface as the official web/desktop client). This is against Discord's Terms of Service. Use at your own risk.


<img width="2056" height="1290" alt="Screenshot 2026-03-08 at 22 22 18" src="https://github.com/user-attachments/assets/c0cdb64a-a337-498f-900a-9cfd8c238ad0" />




## Features

- **tmux-style pane management** — Split your terminal into multiple independent channel views (horizontal/vertical), resize, zoom, and navigate between them
- **Vim-style keybindings** — Modal editing with Normal, Insert, and Pane Prefix modes
- **Full message support** — Send, edit, delete, and reply to messages with Discord-flavored markdown rendering
- **Server/channel sidebar** — Toggleable tree view with guilds, categories, channels, and DMs
- **Two login methods** — Paste a token directly, or scan a QR code with the Discord mobile app
- **Local message history** — SQLite-backed persistence so channels load instantly on restart
- **Session persistence** — Pane layouts are saved and restored across sessions
- **Anti-detection** — Mimics the official Discord web client's connection fingerprint to reduce ban risk

## Prerequisites

- [Nix](https://nixos.org/download/) with flakes enabled
- A Discord account

All build dependencies (Rust toolchain, system libraries) are managed by the Nix flake — nothing needs to be installed globally.

## Getting Started

### 1. Clone and enter the dev environment

```bash
git clone https://github.com/YOUR_USERNAME/discordinator.git
cd discordinator
nix develop
```

### 2. Build

```bash
cargo build
```

### 3. Run

```bash
cargo run
```

On first launch, you'll see a login screen with two authentication options:

1. **Paste token** (F1) — If you already have your Discord token, paste it directly
2. **QR code** (F2) — Scan with the Discord mobile app

After successful authentication, your token is stored securely in the OS keyring.

You can also provide a token via environment variable:

```bash
DISCORD_TOKEN=your_token_here cargo run
```

### 4. Enable debug logging

```bash
RUST_LOG=debug cargo run
```

Logs are written to `~/.local/share/discordinator/logs/discordinator.log`.

## Usage

### Modes

Discordinator uses vim-style modal input:

| Mode | Enter with | Purpose |
|------|-----------|---------|
| **Normal** | `Esc` | Navigate messages, select items |
| **Insert** | `i` | Compose and send messages |
| **Pane Prefix** | `Ctrl+b` | Pane management (next key is the pane action) |

### Navigation (Normal Mode)

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll messages down / up |
| `J` / `K` | Select message down / up |
| `g` / `G` | Jump to top / bottom of messages |
| `Ctrl+u` / `Ctrl+d` | Half-page scroll up / down |
| `i` | Enter Insert mode |
| `r` | Reply to selected message |
| `e` | Edit selected message (own only) |
| `d` | Delete selected message (own only, with `y` to confirm) |

### Message Composition (Insert Mode)

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Esc` | Return to Normal mode |

### Pane Management (Ctrl+b prefix)

Press `Ctrl+b` first, then the action key:

| Key | Action |
|-----|--------|
| `"` | Split pane horizontally (top/bottom) |
| `%` | Split pane vertically (left/right) |
| `x` | Close current pane |
| `o` | Cycle focus to next pane |
| `z` | Toggle zoom (maximize/restore current pane) |
| `s` | Toggle sidebar |
| Arrow keys | Move focus directionally |
| `Ctrl+Arrow` | Resize current pane |

### Global

| Key | Action |
|-----|--------|
| `Ctrl+q` | Quit |

## Configuration

Configuration lives at `~/.config/discordinator/config.toml`. A default is created on first run.

```toml
[general]
timestamp_format = "%H:%M"
render_fps = 60

[auth]
token_source = "keyring"    # "keyring", "env", or "file"

[discord]
# Anti-detection: keep updated to match the real Discord web client.
# Check discord.com/app and inspect network requests for current values.
client_build_number = 346892
browser_version = "131.0.0.0"

[appearance]
show_sidebar = true
sidebar_width = 24

[pane]
active_border_color = "cyan"
inactive_border_color = "gray"

[session]
auto_save = true
restore_on_start = true
```

### File Locations (XDG)

| Purpose | Path |
|---------|------|
| Config | `~/.config/discordinator/config.toml` |
| Database | `~/.local/share/discordinator/messages.db` |
| Logs | `~/.local/share/discordinator/logs/` |

## Development

```bash
nix develop                           # Enter dev environment
cargo build                           # Build
cargo test                            # Run tests
cargo test -- --nocapture             # Run tests with output
cargo clippy -- -D warnings           # Lint (warnings are errors)
cargo fmt --check                     # Check formatting
RUST_LOG=debug cargo run              # Run with debug logging
```

All commands can be run inside `nix develop`. The Nix flake provides the Rust toolchain, system libraries, and all build dependencies.

### Environment Variables

| Variable | Description |
|----------|-------------|
| `DISCORD_TOKEN` | Discord user token (highest priority auth source) |
| `RUST_LOG` | Log level filter (`error`, `warn`, `info`, `debug`, `trace`) |

## License

This project is not affiliated with Discord. Use of the Discord User API may violate Discord's Terms of Service. The authors are not responsible for any consequences of using this software.
