# Discordinator

A Discord TUI (Terminal User Interface) client written in Rust with tmux-like split pane support. Monitor and participate in multiple conversations simultaneously across different servers — all from a single terminal window.

> **Disclaimer**: Discordinator uses the Discord User API (the same interface as the official web/desktop client). This is against Discord's Terms of Service. Use at your own risk.

## Features

- **tmux-style pane management** — Split your terminal into multiple independent channel views (horizontal/vertical), resize, zoom, and navigate between them
- **Vim-style keybindings** — Modal editing with Normal, Insert, Command, and Pane Prefix modes
- **Full message support** — Send, edit, delete, and reply to messages with Discord-flavored markdown rendering
- **Server/channel sidebar** — Toggleable tree view with guilds, categories, channels, and DMs
- **Three login methods** — Paste a token, email + password (with 2FA), or scan a QR code
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

On first launch, you'll see a login screen with three authentication options:

1. **Paste token** — If you already have your Discord token, paste it directly
2. **Email + password** — Enter your credentials (supports 2FA)
3. **QR code** — Scan with the Discord mobile app

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
| **Command** | `:` | Execute commands |
| **Pane Prefix** | `Ctrl+b` | Pane management (next key is the pane action) |

### Navigation (Normal Mode)

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll messages down / up |
| `g` / `G` | Jump to top / bottom of messages |
| `Ctrl+u` / `Ctrl+d` | Half-page scroll up / down |
| `i` | Enter Insert mode |
| `r` | Reply to selected message |
| `e` | Edit selected message (own only) |
| `d` | Delete selected message (own only, with confirmation) |

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
| `Tab` / `Shift+Tab` | Cycle focus between sidebar, messages, and input |

## Configuration

Configuration lives at `~/.config/discordinator/config.toml`. A default is created on first run.

```toml
[general]
timestamp_format = "%H:%M"
show_typing_indicator = true
render_fps = 60

[auth]
token_source = "keyring"    # "keyring", "env", or "file"

[discord]
# Anti-detection: keep updated to match the real Discord web client.
# Check discord.com/app and inspect network requests for current values.
client_build_number = 346892
browser_version = "131.0.0.0"

[appearance]
theme = "default"
show_sidebar = true
sidebar_width = 24

[pane]
prefix_key = "Ctrl+b"
border_style = "rounded"
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
| Cache | `~/.cache/discordinator/` |

## Design Decisions

### Why not use twilight-gateway / twilight-http?

Discord's ecosystem crates like `twilight-gateway` and `twilight-http` are designed exclusively for **bot accounts**:

- `twilight-gateway` hardcodes its IDENTIFY properties to `"twilight.rs"` and requires bot `Intents` — user accounts need browser-mimicking properties and don't use intents
- `twilight-http` prepends `Bot ` to the Authorization header — user tokens must be sent without any prefix
- Bot READY payloads differ from user READY payloads (missing `private_channels`, `read_states`, `relationships`, etc.)

Discordinator uses **tokio-tungstenite** for WebSocket and **reqwest** for HTTP, with full control over headers and payloads. Only **twilight-model** is used, for its type-safe Discord data structures (`Id<T>`, `Message`, `Channel`, `Guild`, etc.).

### Clean Architecture

The codebase follows a layered architecture where dependencies only point inward:

```
Presentation (src/ui/)        — ratatui widgets, layout, input handling
Application  (src/app.rs)     — event loop, action dispatcher, state management
Domain       (src/domain/)    — pure types and business logic, no I/O
Infrastructure (src/infrastructure/) — gateway, HTTP, SQLite, keyring
```

### Single-owner state with channel-based I/O

All mutable state lives in `AppState`, owned exclusively by the main event loop. There are no `Arc<Mutex<_>>` wrappers. Background tasks (gateway, HTTP, SQLite) communicate with the main loop through `mpsc` channels. This eliminates data races by design and keeps the mental model simple.

### Action-based state mutation

Every state change goes through a single `apply_action()` function that takes an `Action` enum variant. Input handlers, gateway events, and background results all produce `Action` values — they never mutate state directly. This makes the application testable (fire actions, assert resulting state) and debuggable (log all actions).

### Binary tree pane layout

Panes are stored as a binary tree (`PaneNode::Leaf | PaneNode::Split`), the same model tmux uses. Splitting a pane replaces the leaf with a split node containing two new leaves. Closing a pane collapses the parent split and promotes the sibling. This gives O(log n) operations for typical pane counts (2-8).

### Dirty flag rendering

The UI only re-renders when something actually changed. A `dirty` flag is set on any state mutation and cleared after each render pass. Combined with biased `tokio::select!` polling (gateway events have priority over render ticks), this ensures messages are never dropped while keeping CPU usage low at idle.

### Anti-detection by default

All Discord API communication mimics the official web client:
- Gateway IDENTIFY sends browser-like properties (configurable `client_build_number`, `browser_version`, `User-Agent`)
- HTTP requests include `X-Super-Properties`, `User-Agent`, and `X-Discord-Locale` headers
- Request timing includes 50-150ms random jitter to avoid machine-like patterns
- DM channels are only read from the READY event — `POST /users/@me/channels` is never called (this specific endpoint is known to trigger bans)

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

All commands must be run inside `nix develop`. The Nix flake provides the Rust toolchain, system libraries, and all build dependencies.

### Environment Variables

| Variable | Description |
|----------|-------------|
| `DISCORD_TOKEN` | Discord user token (highest priority auth source) |
| `RUST_LOG` | Log level filter (`error`, `warn`, `info`, `debug`, `trace`) |
| `DISCORDINATOR_CONFIG` | Custom config file path (overrides XDG default) |

## License

This project is not affiliated with Discord. Use of the Discord User API may violate Discord's Terms of Service. The authors are not responsible for any consequences of using this software.
