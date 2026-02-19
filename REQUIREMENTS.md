# Discordinator - Requirements Document

## Project Overview

**Discordinator** is a Discord TUI (Terminal User Interface) client written in Rust. Its defining feature is a **tmux-like split pane system** that allows users to simultaneously view and interact with multiple channels across different servers in a single terminal window.

## Goal

Build a fully functional Discord TUI client in Rust with tmux-like pane management, enabling power users to monitor and participate in multiple conversations simultaneously.

## Success Criteria

- [ ] User can authenticate (token, email+password, QR code) and connect to Discord gateway
- [ ] Server list, channel tree, and message view render correctly
- [ ] Messages can be sent, edited, and deleted
- [ ] tmux-like pane splitting works (horizontal/vertical) with independent channel views
- [ ] Vim-style keyboard navigation throughout the application
- [ ] All core features pass automated tests (unit + integration with mock server)
- [ ] Application builds and runs via `nix develop`
- [ ] Performance: <16ms render time (60 FPS), <100MB RSS memory for typical usage
- [ ] Messages persist locally via SQLite for instant history on restart

---

## Technology Stack

### Critical: Why Not twilight-gateway / twilight-http

twilight-gateway and twilight-http are **bot-only** libraries. They cannot be used for user account (selfbot) clients:

1. **twilight-gateway** hardcodes IDENTIFY `properties` to `"twilight.rs"` with no public API to override. User clients must send browser-mimicking properties to avoid detection.
2. **twilight-gateway** requires `Intents` in the IDENTIFY payload. User accounts do not use intents.
3. **twilight-gateway** expects bot-format READY payloads. User READY events include additional fields (`guild_folders`, `read_states`, `relationships`, `private_channels`, `user_settings`) that twilight ignores/discards.
4. **twilight-http** prepends `Bot ` to Authorization headers. User tokens must be sent without any prefix.

**Solution**: Build custom gateway (tokio-tungstenite) and HTTP client (reqwest) like Oxicord does. Keep **twilight-model** for shared data types only (Id<T>, Message, Channel, Guild, User, Event structs).

### Stack

| Component | Crate / Version | Rationale |
|-----------|-----------------|-----------|
| Language | Rust 2021 edition | Performance, safety, strong async ecosystem |
| TUI Framework | ratatui 0.30.0 | Best Rust TUI, modularized workspace, Layout system for split panes |
| Terminal Backend | crossterm (via ratatui-crossterm) | Cross-platform, bundled with ratatui 0.30 |
| Discord Models | twilight-model 0.17.x | Type-safe Discord data types: `Id<T>` with marker types, `Message`, `Channel`, `Guild`, `User`, `Event` |
| WebSocket | tokio-tungstenite 0.26.x | Custom gateway: full control over IDENTIFY payload, heartbeat, zstd compression |
| HTTP Client | reqwest 0.12.x | Custom REST client: full control over headers (no `Bot ` prefix), X-Super-Properties |
| Async Runtime | tokio 1.x (full features) | Runtime for gateway, HTTP, terminal events |
| Database | rusqlite 0.32.x | SQLite for message persistence, session storage (used via `spawn_blocking`) |
| Serialization | serde 1.x + serde_json 1.x | Standard Rust serialization, gateway payload parsing |
| Compression | flate2 (zlib-stream) | Gateway transport compression (Discord uses zlib-stream, not zstd for user accounts) |
| Keyring | keyring 3.6.x | Secure cross-platform token storage (apple-native, sync-secret-service) |
| Markdown | Custom parser | Discord-flavored markdown with mentions, emoji, spoilers |
| Image Display | ratatui-image 10.0.5 | Sixel/Kitty/iTerm2/halfblocks protocol support, ratatui 0.30 compatible |
| Text Input | Custom (built on ratatui) | tui-textarea incompatible with ratatui 0.30; build custom input widget |
| Logging | tracing 0.1.x + tracing-subscriber 0.3.x | Structured async-aware logging to file |
| Error Handling | color-eyre 0.6.x | Pretty error reports, backtraces, graceful terminal restore on panic |
| Config | toml 0.8.x | TOML config file parsing |
| Directories | dirs 6.x | XDG-compliant platform directory resolution |
| QR Code | qrcode 0.14.x | QR code generation for remote auth login |
| Base64 | base64 0.22.x | X-Super-Properties encoding |

---

## Architecture

### High-Level Architecture (Clean Architecture)

The application follows a **Clean Architecture** pattern with four layers. Dependencies point inward only: presentation depends on application, application depends on domain, infrastructure implements domain interfaces.

```
┌──────────────────────────────────────────────────────────────┐
│                   PRESENTATION LAYER                          │
│  ┌──────────────────────────────────────────────────────┐    │
│  │              Terminal (crossterm raw mode)             │    │
│  ├──────────────────────────────────────────────────────┤    │
│  │              UI Components (ratatui 0.30)             │    │
│  │  ┌──────────┬──────────────┬──────────────┐          │    │
│  │  │ Server/  │   Pane 1     │   Pane 2     │          │    │
│  │  │ Channel  │  (messages)  │  (messages)  │          │    │
│  │  │ Tree     ├──────────────┼──────────────┤          │    │
│  │  │(toggle)  │   Pane 3     │   Pane 4     │          │    │
│  │  │          │  (messages)  │  (messages)  │          │    │
│  │  └──────────┴──────────────┴──────────────┘          │    │
│  │  Input Handler (mode-aware key dispatch)              │    │
│  └──────────────────────────────────────────────────────┘    │
├──────────────────────────────────────────────────────────────┤
│                   APPLICATION LAYER                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │  App (tokio::select! hub)                             │    │
│  │  ├── handle_terminal_event()                          │    │
│  │  ├── handle_gateway_event()                           │    │
│  │  ├── handle_background_result()                       │    │
│  │  └── render_if_dirty()                                │    │
│  │  Action Dispatcher (command → state mutation)         │    │
│  │  Background Task Coordinator (mpsc channels)          │    │
│  └──────────────────────────────────────────────────────┘    │
├──────────────────────────────────────────────────────────────┤
│                     DOMAIN LAYER                              │
│  ┌──────────────────────────────────────────────────────┐    │
│  │  Domain Types (twilight-model re-exports + extensions)│    │
│  │  ├── Id<UserMarker>, Id<ChannelMarker>, etc.          │    │
│  │  ├── Message, Channel, Guild, User                    │    │
│  │  ├── GatewayEvent (custom enum, user-account format)  │    │
│  │  └── DiscordCache (HashMap-based, in-process)         │    │
│  │  PaneTree (binary tree layout engine)                 │    │
│  │  MarkdownAST (parsed Discord markdown)                │    │
│  └──────────────────────────────────────────────────────┘    │
├──────────────────────────────────────────────────────────────┤
│                  INFRASTRUCTURE LAYER                         │
│  ┌────────────┬───────────────┬──────────────┬──────────┐   │
│  │  Gateway   │  HTTP Client  │   SQLite DB  │  Keyring │   │
│  │(tungstenite│  (reqwest)    │  (rusqlite)  │          │   │
│  │ + zlib)    │  + anti-detect│  spawn_block │          │   │
│  └────────────┴───────────────┴──────────────┴──────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### Event Loop Architecture

Single-task `tokio::select!` hub with **biased polling** (gateway events have priority over render ticks to prevent message loss under load). **Dirty flag rendering**: only re-render when state actually changed, not on every tick.

```rust
loop {
    tokio::select! {
        biased;  // gateway events have priority

        // Discord gateway events (highest priority - never miss messages)
        Some(event) = gateway_rx.recv() => {
            app.dirty |= handle_gateway_event(event, &mut app.state);
        }

        // Background task results (HTTP responses, SQLite query results)
        Some(result) = background_rx.recv() => {
            app.dirty |= handle_background_result(result, &mut app.state);
        }

        // Terminal input (keyboard, mouse, resize)
        Some(event) = terminal_events.next() => {
            app.dirty |= handle_input(event, &mut app.state);
        }

        // Render tick at 60 FPS (~16ms) - only render if dirty
        _ = render_tick.tick() => {
            if app.dirty {
                terminal.draw(|f| ui::render(f, &app.state))?;
                app.dirty = false;
            }
        }
    }
}
```

### Async Architecture: Channel-Based Decoupling

All heavy I/O runs in background tasks, communicating results back to the main loop via `mpsc` channels. **No `Arc<Mutex<_>>`** on shared state — the main loop owns all mutable state exclusively.

```
                        ┌───────────────┐
                        │   Main Loop   │ (owns all state)
                        │ tokio::select!│
                        └───┬───┬───┬───┘
                            │   │   │
              ┌─────────────┘   │   └─────────────┐
              ▼                 ▼                   ▼
    ┌─────────────────┐ ┌────────────┐   ┌──────────────────┐
    │  Gateway Task   │ │ HTTP Actor │   │  SQLite Worker   │
    │ (WebSocket read │ │ (reqwest   │   │ (spawn_blocking  │
    │  + decompress   │ │  + headers │   │  in thread pool) │
    │  + parse JSON)  │ │  + jitter) │   │                  │
    │                 │ │            │   │                  │
    │ → gateway_tx    │ │ → bg_tx    │   │ → bg_tx          │
    └─────────────────┘ └────────────┘   └──────────────────┘
```

- **Gateway task**: Reads WebSocket frames, decompresses zlib-stream, deserializes JSON, sends parsed events to `gateway_tx`. Runs in its own tokio task.
- **HTTP actor**: Receives requests via `mpsc` channel, adds anti-detection headers, applies rate limiting + jitter, sends responses back via `bg_tx`. Single task handles all HTTP to ensure rate limit state is centralized.
- **SQLite worker**: Receives queries via `mpsc` channel, executes on `spawn_blocking` thread (rusqlite is synchronous), sends results back via `bg_tx`.

### Directory Layout (XDG)

| Purpose | Path | Contents |
|---------|------|----------|
| Config | `~/.config/discordinator/` | `config.toml`, custom themes |
| Data | `~/.local/share/discordinator/` | `messages.db` (SQLite), `sessions/` |
| Cache | `~/.cache/discordinator/` | Downloaded attachments, avatar cache |
| Logs | `~/.local/share/discordinator/logs/` | `discordinator.log` (tracing output) |

### Module Structure

```
src/
├── main.rs                  # Entry point, tokio runtime, panic handler
├── app.rs                   # App struct, tokio::select! hub, dirty flag
├── action.rs                # Action enum (all state mutations go through here)
├── auth.rs                  # Login flow: token, email+pass+2FA, QR code
├── config.rs                # Configuration (TOML file, XDG dirs)
│
├── domain/                  # DOMAIN LAYER - pure types, no I/O
│   ├── mod.rs
│   ├── types.rs             # Re-export twilight-model types + app-specific newtypes
│   ├── cache.rs             # DiscordCache: HashMap-based guild/channel/user/message store
│   ├── pane.rs              # PaneNode binary tree, PaneId, split/close/resize
│   ├── markdown.rs          # Discord markdown AST (parser output)
│   └── event.rs             # GatewayEvent enum (user-account format, not bot)
│
├── infrastructure/          # INFRASTRUCTURE LAYER - I/O, external services
│   ├── mod.rs
│   ├── gateway.rs           # Custom WebSocket gateway (tokio-tungstenite + zlib-stream)
│   ├── http_client.rs       # Custom REST client (reqwest + anti-detection headers)
│   ├── anti_detection.rs    # IDENTIFY properties, X-Super-Properties, header sets
│   ├── db.rs                # SQLite database (rusqlite, spawn_blocking)
│   └── keyring.rs           # Token storage (keyring crate wrapper)
│
├── ui/                      # PRESENTATION LAYER - rendering + input
│   ├── mod.rs
│   ├── layout.rs            # Main layout: sidebar | pane tree | status bar
│   ├── pane_renderer.rs     # Recursive pane tree → ratatui Layout
│   ├── login.rs             # TUI login screen (token/email/QR)
│   ├── widgets/
│   │   ├── mod.rs
│   │   ├── server_tree.rs   # Server/channel tree sidebar (toggleable)
│   │   ├── message_view.rs  # Message list with rendered markdown
│   │   ├── input_box.rs     # Message composition (custom widget)
│   │   ├── member_list.rs   # Member sidebar
│   │   ├── status_bar.rs    # Connection, channel, mode, unread count
│   │   └── command_palette.rs # Ctrl+P fuzzy command palette
│   └── theme.rs             # Color scheme and styling
│
├── input/                   # Input handling (part of presentation)
│   ├── mod.rs
│   ├── handler.rs           # Key event → Action dispatch (mode-aware)
│   ├── mode.rs              # InputMode state machine (Normal/Insert/Command/PanePrefix)
│   └── keybindings.rs       # Configurable key bindings
│
├── markdown/                # Markdown processing
│   ├── mod.rs
│   ├── parser.rs            # Discord-flavored markdown → AST
│   └── renderer.rs          # AST → ratatui Spans/Lines (cached per message)
│
└── utils/
    ├── mod.rs
    └── unicode.rs            # Unicode width helpers

tests/
├── mock_discord/            # Mock Discord gateway + HTTP for testing
│   ├── mod.rs
│   ├── gateway.rs           # Mock WebSocket: HELLO → IDENTIFY → READY cycle
│   └── http.rs              # Mock REST: message CRUD, channel history
├── test_pane_tree.rs        # Pane split/close/resize/focus traversal
├── test_markdown.rs         # Markdown parser edge cases
├── test_cache.rs            # Cache operations (insert, update, lookup, eviction)
├── test_config.rs           # Config parsing (defaults, overrides, invalid)
├── test_db.rs               # SQLite CRUD, schema migration
├── test_keybindings.rs      # Key binding resolution per mode
├── test_gateway.rs          # Gateway connection, heartbeat, reconnect
├── test_http_client.rs      # HTTP headers, rate limiting, anti-detection
└── test_integration.rs      # Full lifecycle with mock server
```

### Startup Flow

1. Initialize color-eyre panic handler (restores terminal on panic)
2. Load config from XDG config dir (create default if missing)
3. Initialize tracing (log to file in XDG data dir)
4. Check for existing token (keyring → env var → config file)
5. **If no token**: Show TUI login screen (email+pass / QR code / paste token)
6. **If token exists**: Attempt gateway connection
7. **If token expired/invalid**: Show login screen with error message
8. On successful connection: load last session layout or default single-pane view
9. Enter main event loop

---

## API Approach

### User API (Selfbot)

Discordinator uses the **Discord User API** (same endpoints as the official web/desktop client), **not** the Bot API. This is the standard approach used by all major Discord TUI clients (Discordo, Endcord, Oxicord, Cordless).

**Rationale**: A TUI *client* must act as the user's personal Discord client - seeing their servers, DMs, friends, and full message history. The Bot API is fundamentally different (requires server invitations, no DMs, no user presence) and would make this a bot dashboard rather than a chat client.

**Disclaimer**: Using unofficial clients with user tokens violates Discord's Terms of Service. Users assume all risk. Discordinator must display a clear ToS warning on first launch.

### DM Safety Policy

To avoid the detection pattern that got Cordless' developer banned:
- **NEVER** call `POST /users/@me/channels` to create new DM channels
- Only display DMs provided in the READY event payload
- Existing DM channels from READY are safe to send messages to
- If a user needs to open a new DM, instruct them to do so via the official client first

---

## Anti-Detection Strategy

Discord actively detects and bans unofficial clients. The following measures are **mandatory** to reduce detection risk:

### 1. IDENTIFY Payload Mimicry

The Gateway IDENTIFY (op 2) `properties` field must closely match the official web client:

```json
{
  "os": "Mac OS X",
  "browser": "Chrome",
  "device": "",
  "system_locale": "en-US",
  "browser_user_agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  "browser_version": "131.0.0.0",
  "os_version": "10.15.7",
  "referrer": "",
  "referring_domain": "",
  "referrer_current": "",
  "referring_domain_current": "",
  "release_channel": "stable",
  "client_build_number": 346892
}
```

**These values must be kept up to date** with the real Discord web client. The `client_build_number` changes frequently and is the most important field to keep current.

### 2. HTTP Headers Mimicry

All REST API requests must include headers that match the web client:
- `User-Agent` matching the browser UA string
- `X-Super-Properties` base64-encoded JSON of the IDENTIFY properties
- `X-Discord-Locale` matching system locale
- `Authorization` with the user token (no `Bot ` prefix)

### 3. Endpoint Avoidance

The following endpoints are known to trigger detection and **must be avoided or used sparingly**:
- `POST /users/@me/channels` - Creating DM channels (**BANNED** - this got the Cordless developer banned)
- Use the existing DM channel list from READY event instead
- Avoid bulk operations or rapid sequential requests to unusual endpoints

### 4. Rate Limit Compliance

- Respect all HTTP rate limit headers (`X-RateLimit-*`) in the custom reqwest-based HTTP client
- Implement per-route rate limiting (bucket tracking) since we're not using twilight-http
- Global rate limit: stay well under 50 req/s
- Add jitter (50-150ms random delay) to request timing to avoid machine-like patterns
- The HTTP actor centralizes all requests through a single task to maintain rate limit state

### 5. Connection Behavior

- Use zlib-stream transport compression (this is what user accounts actually use, not zstd)
- Implement heartbeat/ACK cycle manually (send op 1 at the interval from HELLO, track last ACK)
- Handle session invalidation gracefully (re-IDENTIFY on invalid session, don't rapid-reconnect)
- Use a single shard (user accounts don't shard)
- Implement RESUME (op 6) for seamless reconnection after brief disconnects

---

## Features

### Phase 1: Core MVP (Single-Pane Client)

Phase 1 builds a traditional single-view Discord client. The pane system comes in Phase 2.

#### P1.1 - Authentication & Connection
- [ ] Token-based authentication (environment variable `DISCORD_TOKEN`, keyring, or config file)
- [ ] TUI login form: email + password + optional 2FA code
- [ ] QR code authentication (render QR in terminal for Discord mobile scan)
- [ ] Secure token storage in OS keyring via `keyring` crate (apple-native on macOS, secret-service on Linux)
- [ ] Connect to Discord gateway via WebSocket (wss://gateway.discord.gg/?v=10&encoding=json&compress=zlib-stream)
- [ ] Handle heartbeat/ACK cycle (op 1/op 11)
- [ ] Handle READY event and populate initial state (guilds, channels, user, DMs)
- [ ] Automatic reconnection with RESUME (op 6) on disconnect
- [ ] Graceful error handling for invalid/expired tokens with re-auth prompt
- [ ] Anti-detection: mimic web client IDENTIFY properties (see Anti-Detection section)
- [ ] Anti-detection: set correct HTTP headers on all REST requests (X-Super-Properties, User-Agent)
- [ ] ToS disclaimer display on first launch

#### P1.2 - Basic UI Layout
- [ ] Toggleable sidebar: server/channel tree on the left, hidden by default on small terminals (<120 cols)
- [ ] Sidebar toggle keybinding (Ctrl+b + s)
- [ ] Server list in sidebar (guild names, sorted by position)
- [ ] Channel tree in sidebar (categories with collapsible text channels)
- [ ] DM list from READY event (never call POST /users/@me/channels)
- [ ] Message view (scrollable message history)
- [ ] Input box for composing messages (custom widget)
- [ ] Status bar (connection status, current server/channel, mode indicator, unread count)
- [ ] Focus management between sidebar, messages, and input (Tab/Shift+Tab)

#### P1.3 - Message Display
- [ ] Render message author (with role color), timestamp, content
- [ ] Core markdown: **bold**, *italic*, __underline__, ~~strikethrough~~, `code`, ```code blocks```
- [ ] User mentions (`<@id>`) resolved to display names
- [ ] Channel mentions (`<#id>`) resolved to channel names
- [ ] Role mentions (`<@&id>`) with role color
- [ ] Custom emoji display (`<:name:id>` rendered as `:name:`)
- [ ] Reply threading (show replied-to message preview above the reply)
- [ ] Edited message indicator (`(edited)` suffix)
- [ ] System messages (joins, boosts, pins)
- [ ] Attachment indicators (filename, size, type icon)

#### P1.4 - Message Interaction
- [ ] Send messages to current channel
- [ ] Edit own messages (select message, press `e`, modify in input box)
- [ ] Delete own messages (select message, press `d`, confirm with `y`)
- [ ] Reply to messages (select message, press `r`, compose reply)
- [ ] Message history scrolling with lazy loading (fetch older messages on scroll up via REST)
- [ ] Typing indicator (send TYPING_START when composing, display others' typing)

#### P1.5 - Navigation
- [ ] Vim-style navigation: j/k for messages, h/l for sidebar tree
- [ ] g/G for top/bottom of message list
- [ ] Ctrl+u/Ctrl+d for half-page scroll
- [ ] / for channel search/filter (fuzzy match)
- [ ] Server switching via sidebar or fuzzy search
- [ ] Channel switching via sidebar tree navigation
- [ ] Unread indicators (bold channel names, bullet markers)
- [ ] Mention indicators (red/highlighted channel names)

#### P1.6 - Local Storage (SQLite)
- [ ] SQLite database at `~/.local/share/discordinator/messages.db`
- [ ] Store messages (id, channel_id, author_id, content, timestamp, edited_timestamp, attachments JSON)
- [ ] Store channel metadata (id, guild_id, name, type, position, last_read_message_id)
- [ ] Store guild metadata (id, name, icon)
- [ ] On channel open: load cached messages from SQLite first, then fetch newer from REST
- [ ] On gateway MESSAGE_CREATE: insert into SQLite and update UI
- [ ] On gateway MESSAGE_UPDATE/DELETE: update/remove from SQLite
- [ ] Configurable max messages per channel in SQLite (default 10000)
- [ ] Session storage: save last viewed channels, sidebar state

### Phase 2: Pane Management (Core Differentiator)

Phase 2 retrofits the pane system onto the Phase 1 single-view client.

#### P2.1 - Pane System Architecture
- [ ] Binary tree pane layout (each node is either a split or a leaf pane)
- [ ] Each leaf pane contains an independent channel view (messages + input)
- [ ] Pane focus management (one active pane at a time, highlighted border)
- [ ] Panes track their own scroll position, channel, and input state independently
- [ ] Unlimited pane count (no artificial limit; user decides based on terminal size)
- [ ] Sidebar remains a fixed toggleable element outside the pane tree

#### P2.2 - Pane Operations (tmux-like)
- [ ] `Ctrl+b "` - Split current pane horizontally (top/bottom)
- [ ] `Ctrl+b %` - Split current pane vertically (left/right)
- [ ] `Ctrl+b x` - Close current pane (confirm if only pane)
- [ ] `Ctrl+b o` - Cycle focus to next pane
- [ ] `Ctrl+b arrow` - Move focus directionally (up/down/left/right)
- [ ] `Ctrl+b z` - Toggle zoom (maximize/restore current pane)
- [ ] `Ctrl+b {` / `Ctrl+b }` - Swap pane with previous/next
- [ ] `Ctrl+b space` - Cycle through preset layouts (even-horizontal, even-vertical, tiled)
- [ ] `Ctrl+b q` - Flash pane numbers for quick selection (then press number)
- [ ] Pane resize: `Ctrl+b Ctrl+arrow` to resize in direction

#### P2.3 - Pane Features
- [ ] Each pane can view a different server/channel independently
- [ ] Assign a channel to a pane via command palette or sidebar tree
- [ ] Pane title bar showing `server > #channel`
- [ ] Active pane border highlighted (configurable color)
- [ ] Inactive panes still receive and display new messages in real-time
- [ ] Pane layouts persist across sessions (save/restore to SQLite)

### Phase 3: Enhanced Features

#### P3.1 - Advanced Markdown
- [ ] Spoiler tags (`||hidden text||`) with reveal on keypress
- [ ] Timestamp formatting (`<t:unix:R>` relative, `<t:unix:F>` full, etc.)
- [ ] Subtext (`-# small text`)
- [ ] Embeds (title, description, fields, color bar, footer, thumbnail)

#### P3.2 - Reactions
- [ ] Display reactions on messages (emoji + count)
- [ ] Add reactions to messages (emoji picker or type emoji name)
- [ ] Remove own reactions

#### P3.3 - Threads & Forums
- [ ] Display thread indicators on messages
- [ ] Open thread in a pane
- [ ] Forum channel listing (thread list view)
- [ ] Create threads

#### P3.4 - User Presence & Profiles
- [ ] Member list sidebar (toggleable per pane)
- [ ] Online/offline/idle/DND status indicators
- [ ] User profile popup (roles, join date)
- [ ] Activity/game status display

#### P3.5 - Direct Messages
- [ ] DM channel list (from READY event only - never create DMs via API)
- [ ] Group DM support
- [ ] Open DM in a pane
- [ ] DM notifications

#### P3.6 - Search
- [ ] Message search within current channel (local SQLite first, then REST)
- [ ] Global message search (across server via REST)
- [ ] Search results displayed in dedicated pane
- [ ] Filter by author, date, has:file, has:link, etc.

#### P3.7 - File Handling
- [ ] Upload files (via TUI file picker)
- [ ] Download attachments to XDG cache dir
- [ ] Image preview (Sixel/Kitty/iTerm2 via ratatui-image)
- [ ] Open attachments in external application (`xdg-open` / `open`)

#### P3.8 - Notifications
- [ ] Desktop notifications for mentions (via `notify-rust`)
- [ ] Notification bell/counter in status bar
- [ ] Per-channel/server mute settings
- [ ] @mention highlighting in messages

#### P3.9 - Command Palette
- [ ] Ctrl+P to open command palette (fuzzy searchable)
- [ ] Quick channel switch
- [ ] Quick server switch
- [ ] Pane commands
- [ ] Settings toggle
- [ ] Recent channels

### Phase 4: Power User Features

#### P4.1 - Configuration
- [ ] TOML config file (`~/.config/discordinator/config.toml`)
- [ ] Configurable key bindings
- [ ] Configurable color themes
- [ ] Configurable pane prefix key (default Ctrl+b)
- [ ] Configurable timestamp format
- [ ] Configurable message display format

#### P4.2 - Vim Modes
- [ ] Normal mode (navigation, commands)
- [ ] Insert mode (message composition)
- [ ] Command mode (`:` commands like `:quit`, `:join #channel`, `:theme dark`)
- [ ] Visual mode (text selection in messages for copying)

#### P4.3 - Session Management
- [ ] Save pane layouts as named sessions (SQLite)
- [ ] Restore sessions on startup
- [ ] Auto-save session on exit
- [ ] Multiple named sessions (like tmux sessions)

#### P4.4 - Scripting & Extensibility
- [ ] Lua scripting for custom commands
- [ ] Custom keybinding actions
- [ ] External command execution (pipe message through shell command)

### Phase 5: Future / Nice-to-Have

#### P5.1 - Voice (Experimental)
- [ ] Voice channel status display (who's in which voice channel)
- [ ] Voice channel join/leave
- [ ] Basic voice chat (microphone input, speaker output)

#### P5.2 - Rich Media
- [ ] GIF search and send
- [ ] Sticker display
- [ ] Emoji autocomplete with preview
- [ ] Syntax-highlighted code blocks (via syntect)

#### P5.3 - Advanced Pane Features
- [ ] Pane linking (scroll multiple panes together)
- [ ] Pane filters (show only messages matching criteria)
- [ ] Pane picture-in-picture (small floating pane overlay)
- [ ] Pane history (navigate back/forward through channels viewed in pane)

---

## Ralph Tasks (Atomic, Numbered)

These are the concrete tasks for the Ralph loop. Each task should be completable in a single iteration. Tasks are roughly ordered by dependency but Ralph can pick whichever is unblocked.

### Scaffolding (Tasks 1-5)

**Task 1: Project scaffolding**
- Create `Cargo.toml` with all dependencies (see Technology Stack: ratatui, twilight-model, tokio-tungstenite, reqwest, rusqlite, serde, flate2, keyring, color-eyre, tracing, toml, dirs, base64, qrcode)
- Create `src/main.rs` with basic tokio entry point and color-eyre setup
- Create module stubs: `src/domain/mod.rs`, `src/infrastructure/mod.rs`, `src/ui/mod.rs`, `src/input/mod.rs`, `src/markdown/mod.rs`
- Verify `cargo build` succeeds in `nix develop`
- Test: `cargo test` passes (even if no tests yet)
- Files: `Cargo.toml`, `src/main.rs`, module stubs

**Task 2: Domain types**
- Create `src/domain/types.rs` with re-exports from twilight-model: `Id<UserMarker>`, `Id<ChannelMarker>`, `Id<GuildMarker>`, `Id<MessageMarker>`, `Id<RoleMarker>`, `ChannelType`
- Define app-specific types: `PaneId(u32)`, `CachedMessage`, `CachedGuild`, `CachedChannel`, `CachedUser`, `CachedRole`, `MessageAttachment`, `MessageEmbed`, `MessageReference`, `ReadState`
- Define `ConnectionState` enum (Disconnected/Connecting/Connected/Resuming)
- Define `Action` enum (all state mutations)
- Define `BackgroundResult`, `HttpRequest`, `DbRequest` enums
- Test: type construction, PaneId distinctness from Discord IDs
- Files: `src/domain/types.rs`, `src/domain/mod.rs`

**Task 3: Configuration module**
- Create `src/config.rs` with `AppConfig` struct
- XDG directory resolution using `dirs` crate
- TOML config loading with defaults
- Create default config on first run
- Anti-detection fields: `client_build_number`, `browser_version`, `browser_user_agent`
- Test: parse config, missing config creates default, invalid config errors gracefully
- Files: `src/config.rs`, `tests/test_config.rs`

**Task 4: SQLite database module**
- Create `src/infrastructure/db.rs` with schema initialization
- Tables: `messages`, `channels`, `guilds`, `sessions` (see Database Schema in Data Models)
- CRUD operations for messages (insert, batch insert, query by channel, update, delete)
- Session save/load operations
- All operations via `spawn_blocking` — never block the event loop
- Enable WAL mode for concurrent read/write
- Test: all CRUD operations, schema creation
- Files: `src/infrastructure/db.rs`, `tests/test_db.rs`

**Task 5: Error handling and logging**
- Set up color-eyre with custom panic handler
- Panic handler restores terminal state (disable raw mode, show cursor) before printing error
- Configure tracing-subscriber to log to file in XDG data dir
- Log rotation (keep last 5 log files, max 10MB each)
- RUST_LOG env var support
- Test: verify log file is created
- Files: `src/main.rs`

### Discord Infrastructure (Tasks 6-10)

**Task 6: Anti-detection module**
- Create `src/infrastructure/anti_detection.rs` with IDENTIFY properties struct
- Configurable `client_build_number`, `browser_version`, `browser_user_agent` from config
- `build_identify_properties()` → JSON object matching web client format
- `build_super_properties()` → base64-encoded JSON for X-Super-Properties header
- `build_http_headers()` → HeaderMap with User-Agent, X-Super-Properties, X-Discord-Locale, Authorization
- Test: verify IDENTIFY payload matches expected format, X-Super-Properties encoding is correct, headers are complete
- Files: `src/infrastructure/anti_detection.rs`, `src/infrastructure/mod.rs`

**Task 7: Custom gateway connection**
- Create `src/infrastructure/gateway.rs` using tokio-tungstenite (NOT twilight-gateway)
- Connect to `wss://gateway.discord.gg/?v=10&encoding=json&compress=zlib-stream`
- Receive HELLO (op 10), extract heartbeat_interval
- Send IDENTIFY (op 2) with anti-detection properties from Task 6 (NO intents field — user accounts don't use intents)
- Implement heartbeat/ACK cycle (op 1 send, op 11 receive) on tokio::interval
- zlib-stream decompression (flate2): maintain decompressor state across frames, flush on `\x00\x00\xff\xff` suffix
- Parse JSON into `GatewayEvent` enum (user-account format, see Data Models)
- Handle READY: extract `session_id`, `resume_gateway_url`, guilds, private_channels, read_states, relationships
- Send parsed events to `gateway_tx` channel
- Test: connect to mock gateway, verify HELLO→IDENTIFY→READY cycle, heartbeat timing
- Files: `src/infrastructure/gateway.rs`, `src/domain/event.rs`, `tests/mock_discord/mod.rs`, `tests/mock_discord/gateway.rs`

**Task 8: Gateway reconnection and RESUME**
- Implement RESUME (op 6) for seamless reconnection after disconnect
- On WebSocket close: attempt RESUME with stored session_id + sequence number
- If RESUME fails (invalid session): fall back to full re-IDENTIFY
- Exponential backoff on repeated failures (1s, 2s, 4s, max 30s)
- Handle op 7 (Reconnect) and op 9 (Invalid Session) from server
- ConnectionState transitions: Connected → Resuming → Connected (or → Connecting → Connected)
- Test: disconnect → RESUME → success, disconnect → RESUME fail → re-IDENTIFY
- Files: `src/infrastructure/gateway.rs`

**Task 9: Custom HTTP client**
- Create `src/infrastructure/http_client.rs` using reqwest (NOT twilight-http)
- HTTP actor pattern: runs as a single tokio task, receives requests via `mpsc` channel
- All requests include anti-detection headers from Task 6 (no `Bot ` prefix on Authorization)
- Per-route rate limiting: parse `X-RateLimit-*` headers, track bucket state, delay if needed
- Request jitter: 50-150ms random delay on each request
- Endpoints: send message, edit message, delete message, fetch message history, send typing
- **No `create_dm_channel()` method** — the method simply does not exist, preventing accidental DM creation
- Send results back via `bg_tx` channel as `BackgroundResult`
- Test: verify headers on mock HTTP server, rate limit handling, jitter timing
- Files: `src/infrastructure/http_client.rs`, `tests/mock_discord/http.rs`, `tests/test_http_client.rs`

**Task 10: In-memory cache**
- Create `src/domain/cache.rs` with `DiscordCache` struct
- HashMap-based: guilds, channels, users (O(1) lookup by ID)
- Per-channel message VecDeque with MAX_CACHED_MESSAGES_PER_CHANNEL eviction
- Guild ordering: `guild_order: Vec<Id<GuildMarker>>` for sidebar render
- Channel ordering per guild: `channel_order: Vec<Id<ChannelMarker>>` sorted by position
- channel_guild reverse lookup map
- ReadState tracking per channel
- DM channel list from READY
- Helper methods: `resolve_user_name(Id<UserMarker>)`, `resolve_channel_name(Id<ChannelMarker>)`, `resolve_role(Id<RoleMarker>)`
- Populate from READY event data
- Test: populate from mock READY, verify all lookups, message eviction at capacity
- Files: `src/domain/cache.rs`, `tests/test_cache.rs`

### Authentication (Task 11)

**Task 11: Authentication module**
- Create `src/auth.rs` with token management
- Token sources: env var `DISCORD_TOKEN` → keyring → config file
- Store valid token in keyring via `src/infrastructure/keyring.rs`
- Token validation: attempt gateway connect, check for invalid token error code
- Test: token retrieval priority, keyring storage (mocked)
- Files: `src/auth.rs`, `src/infrastructure/keyring.rs`

### UI Foundation (Tasks 12-19)

**Task 12: Terminal setup and main loop**
- Create `src/app.rs` with `App` struct holding all state (see AppState in Data Models)
- Set up crossterm raw mode, alternate screen
- Main `tokio::select!` loop with biased polling: gateway_rx → background_rx → terminal_events → render_tick
- Dirty flag rendering: only `terminal.draw()` when `app.dirty == true`
- Graceful shutdown on Ctrl+C / Ctrl+Q (restore terminal)
- Test: app starts and stops cleanly, dirty flag prevents unnecessary renders
- Files: `src/app.rs`, `src/main.rs`

**Task 13: Action dispatcher**
- Create `src/action.rs` with `Action` enum (see Data Models)
- `apply_action(action: Action, state: &mut AppState)` function
- Each action variant maps to a specific state mutation
- Actions that need I/O (SendMessage, EditMessage, etc.) send to appropriate background channel
- Test: each action produces expected state change
- Files: `src/action.rs`

**Task 14: Input mode system**
- Create `src/input/mode.rs` with `InputMode` enum (Normal, Insert, Command, PanePrefix)
- Create `src/input/handler.rs` with key dispatch: event → Action based on current mode
- Mode transitions: `i` → Insert, `Esc` → Normal, `:` → Command, `Ctrl+b` → PanePrefix
- Test: mode transitions, key routing per mode
- Files: `src/input/mod.rs`, `src/input/mode.rs`, `src/input/handler.rs`

**Task 15: Theme and styling**
- Create `src/ui/theme.rs` with color definitions
- Default theme matching Discord dark mode colors
- Theme struct with all configurable colors (background, text, borders, highlight, mention, etc.)
- Test: theme loads defaults, custom colors override
- Files: `src/ui/theme.rs`

**Task 16: Status bar widget**
- Create `src/ui/widgets/status_bar.rs`
- Display: connection status (ConnectionState → icon), current server > #channel, input mode, unread/mention counts
- Test: renders correct content for each connection state
- Files: `src/ui/widgets/status_bar.rs`, `src/ui/widgets/mod.rs`

**Task 17: Server/channel tree widget**
- Create `src/ui/widgets/server_tree.rs`
- Render guild list using `cache.guild_order` → `cache.guilds` lookups
- Collapsible categories with channels (using `cache.guilds[id].channel_order`)
- DM section from `cache.dm_channels`
- Unread/mention indicators from `cache.read_states` (bold, bullet, red)
- Arrow key / j/k navigation, Enter to select
- Toggle visibility with keybinding
- Test: renders tree correctly, navigation works, collapse/expand
- Files: `src/ui/widgets/server_tree.rs`

**Task 18: Message view widget**
- Create `src/ui/widgets/message_view.rs`
- Render messages from `cache.messages[channel_id]` VecDeque
- Read from VecDeque back (newest) with scroll offset
- Message selection highlight (for reply/edit/delete)
- Auto-scroll to bottom on new messages (ScrollState::Following), stop on manual scroll up
- Date separator between messages from different days
- Author name resolved via `cache.resolve_user_name()`
- Test: renders messages, selection, auto-scroll behavior
- Files: `src/ui/widgets/message_view.rs`

**Task 19: Input box widget**
- Create `src/ui/widgets/input_box.rs` (custom, not tui-textarea)
- Single-line by default, expand to multi-line with Shift+Enter
- Basic text editing: cursor movement (byte + display column tracking), insert, delete, home/end
- Enter produces `Action::SendMessage`, Esc produces `Action::EnterNormalMode`
- Visual feedback for reply mode (show "Replying to @user" header from `InputState.reply_to`)
- Visual feedback for edit mode (show "Editing message" header from `InputState.editing`)
- Test: text insertion, cursor movement, send action
- Files: `src/ui/widgets/input_box.rs`

**Task 20: Main layout composition**
- Create `src/ui/layout.rs`
- Compose: optional sidebar | main content | status bar
- Sidebar width configurable, toggleable with Ctrl+b s
- Main content area hosts the single pane (Phase 1)
- Test: layout renders correctly with/without sidebar
- Files: `src/ui/layout.rs`, `src/ui/mod.rs`

### Core Features (Tasks 21-27)

**Task 21: Channel switching**
- Select channel in sidebar → produces `Action::SwitchChannel(id)`
- Action handler updates focused pane's `channel_id`
- Pipeline: check cache.messages[channel_id] → if empty, send DbRequest::FetchMessages → then send HttpRequest::FetchMessages for newer
- Handle BackgroundResult::CachedMessages and BackgroundResult::MessagesFetched → insert into cache
- Update status bar with new channel info
- Test: channel switch updates view, messages load from cache and REST correctly
- Files: `src/app.rs`, `src/action.rs`

**Task 22: Message sending**
- In Insert mode, Enter produces `Action::SendMessage`
- Action handler sends `HttpRequest::SendMessage` to http_tx channel
- Clear input box immediately (optimistic)
- Gateway will echo back MESSAGE_CREATE → message appears in view
- Handle BackgroundResult::HttpError with status bar error display
- Test: send message via mock HTTP, verify gateway echo inserts into cache
- Files: `src/action.rs`, `src/app.rs`

**Task 23: Message editing**
- In Normal mode, press `e` on own message → populate InputState with message content + editing ID
- Input box shows "Editing message" header
- Enter produces `Action::EditMessage`
- Action handler sends `HttpRequest::EditMessage` to http_tx
- Gateway echoes MESSAGE_UPDATE → cache + rendered cache invalidated
- Test: edit flow via mock server
- Files: `src/action.rs`

**Task 24: Message deletion**
- In Normal mode, press `d` on own message → show confirmation prompt
- `y` confirms → produces `Action::DeleteMessage`
- Action handler sends `HttpRequest::DeleteMessage`
- Gateway echoes MESSAGE_DELETE → remove from cache VecDeque + SQLite
- Test: delete flow with confirmation via mock server
- Files: `src/action.rs`

**Task 25: Reply to message**
- In Normal mode, press `r` on any message → set InputState.reply_to, enter Insert mode
- Input box shows "Replying to @author: message preview..." header
- Enter produces `Action::SendMessage` with `reply_to` set
- HttpRequest::SendMessage includes `message_reference` field
- Test: reply flow, message_reference is set correctly
- Files: `src/action.rs`

**Task 26: Message history scrolling**
- Scroll up past cached messages → send DbRequest::FetchMessages (SQLite first)
- If SQLite exhausted → send HttpRequest::FetchMessages (REST, `before` parameter)
- BackgroundResult::MessagesFetched → VecDeque push_front (history backfill)
- Loading indicator while fetching
- Test: scroll triggers fetch, messages prepend correctly
- Files: `src/ui/widgets/message_view.rs`, `src/action.rs`

**Task 27: Gateway event handling**
- Handle MessageCreate → `cache.messages[channel_id].push_back()`, send DbRequest::InsertMessage, set dirty for all panes viewing that channel
- Handle MessageUpdate → update in cache VecDeque (find by ID), invalidate rendered cache, send DbRequest::UpdateMessage
- Handle MessageDelete → remove from cache VecDeque, send DbRequest::DeleteMessage
- Handle TypingStart → update `cache.typing[channel_id]`, expire after 10s
- Handle GuildCreate/Update → update `cache.guilds`, update `cache.guild_order`
- Handle ChannelCreate/Update/Delete → update `cache.channels`, update guild's `channel_order`
- Test: each event type via mock gateway
- Files: `src/app.rs`

### Discord Markdown (Tasks 28-30)

**Task 28: Markdown parser (core)**
- Create `src/markdown/parser.rs`
- Parse: **bold**, *italic*, __underline__, ~~strikethrough~~, `inline code`, ```code blocks```
- Parse: user mentions `<@id>`, channel mentions `<#id>`, role mentions `<@&id>`
- Parse: custom emoji `<:name:id>` and `<a:name:id>` (animated)
- Output: `MarkdownAst` — Vec of typed spans (text + style attributes + mention IDs)
- Separate type from raw String prevents rendering unparsed content
- Test: extensive parser tests for each markdown element, edge cases, nested formatting
- Files: `src/markdown/parser.rs`, `src/markdown/mod.rs`, `src/domain/markdown.rs`, `tests/test_markdown.rs`

**Task 29: Markdown renderer**
- Create `src/markdown/renderer.rs`
- Convert `MarkdownAst` spans to `Vec<ratatui::text::Line<'static>>` with appropriate styles
- Resolve mentions using cache (user ID → display name, channel ID → #channel-name)
- Role mentions use role color from `cache.guilds[guild_id].roles[role_id].color`
- Cache rendered output in `CachedMessage.rendered` field (invalidate on MESSAGE_UPDATE)
- Test: rendered output matches expected styled lines
- Files: `src/markdown/renderer.rs`

**Task 30: Markdown integration**
- Integrate markdown parser+renderer into message_view widget
- On first render of a message: parse → render → cache in `CachedMessage.rendered`
- On subsequent renders: use cached `rendered` field directly
- On MESSAGE_UPDATE: set `rendered = None` to force re-parse
- Test: full message rendering pipeline, cache invalidation on edit
- Files: `src/ui/widgets/message_view.rs`

### Pane System (Tasks 31-37)

**Task 31: Pane tree data structure**
- Create `src/domain/pane.rs` with `PaneNode` enum (Leaf/Split), `PaneId(u32)` newtype
- Binary tree operations: insert (split at leaf), remove (close leaf, collapse parent), find by ID
- Tree traversal: in-order for pane numbering, find leaf by direction for focus movement
- All operations O(log n) where n = pane count (typically 2-8)
- Test: extensive tree manipulation tests (split, close, find, traversal, edge cases)
- Files: `src/domain/pane.rs`, `tests/test_pane_tree.rs`

**Task 32: Pane manager**
- Add `PaneManager` to `src/domain/pane.rs` (or `src/domain/pane_manager.rs`)
- Split pane (horizontal/vertical) at current focus → creates new leaf
- Close pane (collapse parent split node, promote sibling)
- Move focus directionally (up/down/left/right based on rendered positions)
- Cycle focus (next pane in tree order)
- Minimum pane size check: refuse split if resulting panes would be too small
- Test: all pane operations
- Files: `src/domain/pane.rs`

**Task 33: Pane rendering**
- Create `src/ui/pane_renderer.rs`
- Recursively convert PaneNode tree → nested ratatui `Layout::split()` calls
- Each leaf pane renders its own message_view + input_box
- Active pane gets highlighted border (config color), inactive panes get dim border
- Pane title bar with `server > #channel` from cache lookup
- Test: verify layout produces correct areas for various tree shapes
- Files: `src/ui/pane_renderer.rs`, `src/ui/layout.rs`

**Task 34: Pane prefix keybindings**
- Implement PanePrefix input mode in handler
- After Ctrl+b, wait for next key → produce pane Action
- " → `Action::SplitPane(Horizontal)`, % → `Action::SplitPane(Vertical)`, x → `Action::ClosePane`, o → `Action::FocusNextPane`, z → `Action::ToggleZoom`
- Arrow keys → `Action::FocusPaneDirection(dir)`, Ctrl+Arrow → `Action::ResizePane(dir, delta)`
- Timeout (1s) or Esc cancels prefix mode → back to Normal
- Test: each pane keybinding triggers correct Action
- Files: `src/input/handler.rs`

**Task 35: Pane zoom**
- `Action::ToggleZoom` toggles zoom on focused pane
- Zoomed: pane_renderer renders only the focused pane at full size
- Unzoom: restore previous layout (PaneManager.zoom_state tracks zoomed pane)
- Status bar shows zoom indicator
- Test: zoom/unzoom preserves pane state
- Files: `src/domain/pane.rs`, `src/ui/pane_renderer.rs`

**Task 36: Pane channel assignment**
- When sidebar selection fires `Action::SwitchChannel(id)`, it assigns to the focused pane
- Gateway events route to all panes viewing the affected channel (iterate leaves, check channel_id)
- New messages appear in all panes viewing that channel
- Test: multi-pane with different channels, events route correctly
- Files: `src/action.rs`, `src/app.rs`

**Task 37: Pane session persistence**
- Save pane tree layout to SQLite on exit (serialize PaneNode tree + channel IDs as JSON)
- `DbRequest::SaveSession` / `DbRequest::LoadSession`
- Restore pane layout from SQLite on startup
- Auto-save session periodically (every 60s if dirty)
- Test: save and restore produces identical layout
- Files: `src/infrastructure/db.rs`, `src/domain/pane.rs`

### Login UI (Tasks 38-40)

**Task 38: Token paste login**
- Create `src/ui/login.rs` with login screen
- Option 1: Paste token directly
- Validate token by attempting gateway connection
- On success: store in keyring, proceed to main view
- Test: login flow with valid/invalid mock tokens
- Files: `src/ui/login.rs`

**Task 39: Email + password login**
- Login screen option 2: email + password form
- POST to Discord's login endpoint via reqwest (with anti-detection headers)
- Handle 2FA challenge (prompt for TOTP code)
- Extract token from response
- Test: login flow via mock HTTP endpoints
- Files: `src/ui/login.rs`, `src/auth.rs`

**Task 40: QR code login**
- Login screen option 3: QR code
- Connect to Discord Remote Auth WebSocket (wss://remote-auth-gateway.discord.gg)
- Generate QR code using `qrcode` crate, render in terminal
- Poll for scan completion, extract encrypted token
- Test: QR generation, WebSocket handshake, poll cycle
- Files: `src/ui/login.rs`, `src/auth.rs`

---

## Core Operations Analysis

Understanding what operations the application performs drives the choice of data types and data structures.

### Operation Frequency Table

| Operation | Frequency | Latency Budget | Data Access Pattern |
|-----------|-----------|----------------|---------------------|
| Render frame | 60/s (when dirty) | <16ms | Read all visible state |
| Gateway event receive | ~1-50/s | <1ms processing | Write to cache, read channel lookup |
| Key input handling | ~1-10/s | <1ms | Read mode, write state |
| Message display (per visible msg) | 60/s | <0.1ms each | Read message + author + cached markdown |
| Channel switch | ~0.1/s | <100ms perceived | Read SQLite, maybe REST fetch, write pane state |
| Message send | ~0.01/s | <500ms perceived | Write to HTTP actor channel |
| Pane split/close | ~0.01/s | <16ms | Tree insert/remove |
| Sidebar render | 60/s (when dirty) | <2ms | Iterate guilds, iterate channels per guild |
| Mention resolution | Per message render | <0.01ms | HashMap lookup by ID |
| Scroll | ~5/s | <16ms | Offset change, possible lazy load trigger |

### Critical Path: Message Receive → Display

```
Gateway WS frame
  → zlib decompress (infra)
  → JSON deserialize (infra)
  → gateway_tx.send(event) ──→ main loop receives
  → cache.insert_message(msg)     // O(1) HashMap + VecDeque push
  → db_tx.send(InsertMessage(msg)) // async, fire-and-forget
  → set dirty flag                 // O(1)
  → next render tick: render visible messages
    → for each visible msg:
      → read cached rendered_lines (O(1) if cached)
      → or parse markdown + cache result
```

### Critical Path: Sidebar Render

```
for guild_id in guild_order:           // Vec<Id<GuildMarker>> sorted by position
    guild = guilds.get(guild_id)       // O(1) HashMap
    for channel in guild_channels[guild_id]:  // Vec sorted by (category_pos, pos)
        render channel name + unread indicator
```

---

## Data Models

### Design Principles

1. **Type-safe IDs**: Use `twilight_model::id::Id<T>` with marker types. `Id<UserMarker>` and `Id<ChannelMarker>` are different types — the compiler prevents mixing them. These are `Copy`, `Hash`, `Eq`, backed by `NonZeroU64` — perfect as `HashMap` keys.
2. **Separate content representations**: Raw string → Parsed AST → Rendered Lines. Each stage has its own type to prevent accidentally rendering unparsed content.
3. **State machines as enums**: Connection state, input mode, scroll state — all use enums with data-carrying variants to make impossible states unrepresentable.
4. **Owned data in cache, borrowed for render**: Cache owns all data. Render borrows via `&`. No `Arc` or `Rc` needed since the main loop owns everything.

### Application State

```rust
/// Top-level application state. Owned exclusively by the main loop.
/// No Arc<Mutex<_>> — the tokio::select! hub is the single owner.
struct App {
    // State
    state: AppState,
    dirty: bool,  // Set true on any state change, cleared after render

    // Channels for background communication
    gateway_rx: mpsc::UnboundedReceiver<GatewayEvent>,
    background_rx: mpsc::Receiver<BackgroundResult>,
    http_tx: mpsc::Sender<HttpRequest>,
    db_tx: mpsc::Sender<DbRequest>,
}

struct AppState {
    // Discord state
    cache: DiscordCache,
    connection: ConnectionState,
    current_user: CurrentUser,

    // UI state
    pane_manager: PaneManager,
    sidebar: SidebarState,
    sidebar_visible: bool,
    command_palette: Option<CommandPaletteState>,

    // Input
    input_mode: InputMode,

    // Settings
    config: AppConfig,
}

/// Connection state machine — makes impossible states unrepresentable
enum ConnectionState {
    Disconnected,
    Connecting,
    Connected {
        session_id: String,
        resume_url: String,
        sequence: u64,
    },
    Resuming {
        session_id: String,
        resume_url: String,
        sequence: u64,
    },
}
```

### Discord Cache (In-Memory)

Data structure choices driven by operation patterns:

```rust
/// In-memory cache of Discord state. All lookups are O(1) by ID.
/// Optimized for the two hot paths: message receive and render.
struct DiscordCache {
    // O(1) lookup by ID — the primary access pattern for all Discord entities
    guilds: HashMap<Id<GuildMarker>, CachedGuild>,
    channels: HashMap<Id<ChannelMarker>, CachedChannel>,
    users: HashMap<Id<UserMarker>, CachedUser>,

    // Ordered guild list for sidebar rendering (maintained on READY + GUILD_* events)
    // Vec because guild order rarely changes and sequential iteration is the hot path
    guild_order: Vec<Id<GuildMarker>>,

    // Per-channel message windows — the core data structure for chat display
    // VecDeque: O(1) push_back (new messages), O(1) push_front (history backfill),
    // efficient sequential iteration for rendering visible slice
    messages: HashMap<Id<ChannelMarker>, VecDeque<CachedMessage>>,

    // Per-channel typing indicators: (user_id, started_at)
    // Small Vec per channel — typically 0-3 people typing at once
    typing: HashMap<Id<ChannelMarker>, Vec<(Id<UserMarker>, Instant)>>,

    // Channel → Guild reverse lookup (needed for "which guild is this channel in?")
    channel_guild: HashMap<Id<ChannelMarker>, Id<GuildMarker>>,

    // Unread state per channel
    read_states: HashMap<Id<ChannelMarker>, ReadState>,

    // DM channels from READY event (never mutated via API)
    dm_channels: Vec<Id<ChannelMarker>>,
}

struct CachedGuild {
    id: Id<GuildMarker>,
    name: String,
    icon: Option<String>,
    // Channels sorted by (category_position, position) for sidebar render.
    // Vec<Id> because the order only changes on CHANNEL_UPDATE which is rare.
    channel_order: Vec<Id<ChannelMarker>>,
    roles: HashMap<Id<RoleMarker>, CachedRole>,
}

struct CachedChannel {
    id: Id<ChannelMarker>,
    guild_id: Option<Id<GuildMarker>>,  // None for DMs
    name: String,
    kind: ChannelType,                   // twilight_model::channel::ChannelType
    position: i32,
    parent_id: Option<Id<ChannelMarker>>,  // category
    topic: Option<String>,
}

struct CachedUser {
    id: Id<UserMarker>,
    name: String,
    discriminator: Option<u16>,  // None for new username system
    display_name: Option<String>,
    avatar: Option<String>,
}

struct CachedRole {
    id: Id<RoleMarker>,
    name: String,
    color: u32,     // RGB color for rendering
    position: i32,
}

/// Message with pre-parsed/cached rendered output.
/// Separate types for raw content vs rendered prevent mixing.
struct CachedMessage {
    id: Id<MessageMarker>,
    channel_id: Id<ChannelMarker>,
    author_id: Id<UserMarker>,
    content: String,                        // Raw Discord markdown
    timestamp: String,                      // ISO 8601
    edited_timestamp: Option<String>,
    attachments: Vec<MessageAttachment>,
    embeds: Vec<MessageEmbed>,              // Simplified embed struct
    message_reference: Option<MessageReference>,
    mention_everyone: bool,
    mentions: Vec<Id<UserMarker>>,

    // Cached render output — invalidated on edit, lazily computed
    rendered: Option<Vec<ratatui::text::Line<'static>>>,
}

struct MessageAttachment {
    filename: String,
    size: u64,
    url: String,
    content_type: Option<String>,
}

struct MessageEmbed {
    title: Option<String>,
    description: Option<String>,
    color: Option<u32>,
    url: Option<String>,
}

struct MessageReference {
    message_id: Option<Id<MessageMarker>>,
    channel_id: Option<Id<ChannelMarker>>,
    guild_id: Option<Id<GuildMarker>>,
}

struct ReadState {
    last_message_id: Id<MessageMarker>,
    mention_count: u32,
}

/// Maximum messages kept in memory per channel.
/// Older messages are evicted from VecDeque front, available via SQLite.
const MAX_CACHED_MESSAGES_PER_CHANNEL: usize = 200;
```

### Gateway Events (User Account Format)

Custom event enum because twilight-model's `Event` is bot-format and misses user-specific fields:

```rust
/// Gateway events in user-account format.
/// Deserialized from raw JSON using serde, not twilight's built-in parser.
enum GatewayEvent {
    // Connection lifecycle
    Hello { heartbeat_interval: u64 },
    Ready(Box<ReadyEvent>),         // Boxed — largest variant (~KBs of data)
    Resumed,
    InvalidSession { resumable: bool },
    Reconnect,
    HeartbeatAck,

    // Messages
    MessageCreate(Box<Message>),     // twilight_model::channel::Message
    MessageUpdate(Box<MessageUpdate>),
    MessageDelete { id: Id<MessageMarker>, channel_id: Id<ChannelMarker> },

    // Guilds
    GuildCreate(Box<Guild>),         // twilight_model::guild::Guild
    GuildUpdate(Box<PartialGuild>),
    GuildDelete { id: Id<GuildMarker> },

    // Channels
    ChannelCreate(Box<Channel>),
    ChannelUpdate(Box<Channel>),
    ChannelDelete(Box<Channel>),

    // Typing
    TypingStart {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        timestamp: u64,
    },

    // Presence (Phase 3)
    PresenceUpdate(Box<PresenceUpdate>),

    // Reactions (Phase 3)
    ReactionAdd(Box<ReactionAdd>),
    ReactionRemove(Box<ReactionRemove>),

    // Catch-all for events we don't handle yet
    Unknown { op: u8, event_name: Option<String> },
}

/// User-account READY event. Contains fields that twilight ignores.
struct ReadyEvent {
    user: CurrentUser,
    guilds: Vec<Guild>,
    private_channels: Vec<Channel>,    // DM channels — bot READY doesn't have this
    session_id: String,
    resume_gateway_url: String,
    // User-specific fields (not in bot READY):
    read_states: Vec<ReadState>,        // Per-channel read position
    relationships: Vec<Relationship>,   // Friends list
    user_settings: Option<serde_json::Value>,  // Opaque — we only need a few fields
    guild_folders: Vec<GuildFolder>,    // Folder ordering for sidebar
}

struct GuildFolder {
    guild_ids: Vec<Id<GuildMarker>>,
    name: Option<String>,
    color: Option<u32>,
}

struct Relationship {
    id: Id<UserMarker>,
    kind: RelationshipType,  // Friend, Blocked, PendingIncoming, PendingOutgoing
    user: User,
}
```

### Pane System (Binary Tree)

```rust
/// Newtype for pane IDs — prevents mixing with Discord IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PaneId(u32);

struct PaneManager {
    root: PaneNode,
    focused_pane_id: PaneId,
    zoom_state: Option<PaneId>,   // If a pane is zoomed to fullscreen
    next_id: u32,                 // Monotonic ID counter
}

/// Binary tree for pane layout. Recursive structure.
/// Tree operations (split, close, find) are O(log n) where n = pane count.
/// For typical usage (2-8 panes), this is effectively O(1).
enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,              // 0.0-1.0, position of divider
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

enum SplitDirection {
    Horizontal,  // top/bottom
    Vertical,    // left/right
}

struct Pane {
    id: PaneId,
    channel_id: Option<Id<ChannelMarker>>,
    guild_id: Option<Id<GuildMarker>>,
    scroll: ScrollState,
    input: InputState,
}

/// Scroll state machine: Following (auto-scroll) vs Manual (user scrolled up)
enum ScrollState {
    Following,                    // At bottom, auto-scroll on new messages
    Manual { offset: usize },     // User scrolled up by N messages from bottom
}

struct InputState {
    content: String,       // Current input text
    cursor_pos: usize,     // Byte offset cursor position
    cursor_col: usize,     // Display column (may differ from byte offset for unicode)
    mode: InputMode,       // What we're doing: composing, editing, replying
    reply_to: Option<Id<MessageMarker>>,  // If replying to a message
    editing: Option<Id<MessageMarker>>,   // If editing a message
}
```

### Action Enum (Command Pattern)

All state mutations flow through a single `Action` enum. This makes the app testable (fire actions, assert state) and debuggable (log all actions).

```rust
/// Every state mutation is an Action. Input handlers produce Actions,
/// the main loop applies them. This decouples input from state mutation.
enum Action {
    // Navigation
    SwitchChannel(Id<ChannelMarker>),
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToTop,
    ScrollToBottom,  // re-enters Following mode

    // Messages
    SendMessage { channel_id: Id<ChannelMarker>, content: String, reply_to: Option<Id<MessageMarker>> },
    EditMessage { message_id: Id<MessageMarker>, content: String },
    DeleteMessage { message_id: Id<MessageMarker>, channel_id: Id<ChannelMarker> },

    // Input mode
    EnterInsertMode,
    EnterNormalMode,
    EnterCommandMode,
    EnterPanePrefix,

    // Pane operations
    SplitPane(SplitDirection),
    ClosePane,
    FocusNextPane,
    FocusPaneDirection(Direction),
    ResizePane(Direction, i16),  // +/- delta
    ToggleZoom,
    SwapPane(Direction),

    // UI toggles
    ToggleSidebar,
    ToggleCommandPalette,

    // System
    Quit,
    ForceQuit,
}
```

### Background Communication Types

```rust
/// Requests sent to the HTTP actor task
enum HttpRequest {
    SendMessage { channel_id: Id<ChannelMarker>, content: String, nonce: String, reply_to: Option<Id<MessageMarker>> },
    EditMessage { channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>, content: String },
    DeleteMessage { channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker> },
    FetchMessages { channel_id: Id<ChannelMarker>, before: Option<Id<MessageMarker>>, limit: u8 },
    SendTyping { channel_id: Id<ChannelMarker> },
}

/// Requests sent to the SQLite worker task
enum DbRequest {
    InsertMessage(CachedMessage),
    InsertMessages(Vec<CachedMessage>),
    UpdateMessage { id: Id<MessageMarker>, content: String, edited_timestamp: String },
    DeleteMessage(Id<MessageMarker>),
    FetchMessages { channel_id: Id<ChannelMarker>, before_timestamp: Option<String>, limit: u32 },
    SaveSession { name: String, layout_json: String },
    LoadSession { name: String },
}

/// Results from background tasks back to main loop
enum BackgroundResult {
    // HTTP responses
    MessagesFetched { channel_id: Id<ChannelMarker>, messages: Vec<CachedMessage> },
    HttpError { request: String, error: String },

    // SQLite results
    CachedMessages { channel_id: Id<ChannelMarker>, messages: Vec<CachedMessage> },
    SessionLoaded { name: String, layout_json: Option<String> },
    DbError { operation: String, error: String },
}
```

### Database Schema

```sql
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,       -- Discord snowflake (u64 fits in i64)
    channel_id INTEGER NOT NULL,
    author_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,       -- ISO 8601
    edited_timestamp TEXT,
    attachments TEXT,              -- JSON array
    embeds TEXT,                   -- JSON array
    message_reference TEXT,        -- JSON (for replies)
    mentions TEXT,                 -- JSON array of user IDs
    UNIQUE(id)
);
CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_messages_channel_id ON messages(channel_id, id DESC);

CREATE TABLE IF NOT EXISTS channels (
    id INTEGER PRIMARY KEY,
    guild_id INTEGER,
    name TEXT NOT NULL,
    kind INTEGER NOT NULL,         -- ChannelType as integer
    position INTEGER DEFAULT 0,
    parent_id INTEGER,             -- category
    last_read_message_id INTEGER
);

CREATE TABLE IF NOT EXISTS guilds (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    icon TEXT
);

CREATE TABLE IF NOT EXISTS sessions (
    name TEXT PRIMARY KEY,
    pane_layout TEXT NOT NULL,     -- JSON serialized pane tree
    last_used TEXT NOT NULL        -- ISO 8601
);
```

---

## Type Safety & Performance Review

### Type Safety Guarantees

| Concern | Solution | Enforcement |
|---------|----------|-------------|
| ID mixup (user ID used as channel ID) | `Id<T>` marker types from twilight-model | Compile-time error |
| Pane ID vs Discord ID | `PaneId(u32)` newtype, completely disjoint from `Id<T>` | Compile-time error |
| Rendering unparsed markdown | Separate types: `String` (raw) → `MarkdownAst` (parsed) → `Vec<Line>` (rendered) | Type system |
| Invalid state transitions | `ConnectionState`, `InputMode`, `ScrollState` as enums with data | Pattern exhaustiveness |
| Missing anti-detection headers | All HTTP goes through single `HttpClient` actor that always adds headers | Architecture |
| Accidental DM channel creation | No `create_dm_channel()` method exists on `HttpClient` | API surface |
| Concurrent mutable state | Main loop exclusively owns `AppState`, no `Arc<Mutex>` | Ownership system |

### Performance Characteristics

| Operation | Time Complexity | Space | Notes |
|-----------|----------------|-------|-------|
| Message insert (cache) | O(1) amortized | VecDeque push_back | Evicts from front at MAX_CACHED_MESSAGES_PER_CHANNEL |
| Message history backfill | O(1) amortized | VecDeque push_front | Triggered by scroll-up past cached range |
| Guild/channel/user lookup | O(1) average | HashMap | Hot path in mention resolution and render |
| Pane tree traversal | O(log n) | Recursive | n = pane count, typically 2-8 |
| Sidebar render | O(g * c) | Sequential | g = guilds, c = avg channels per guild |
| Markdown parse | O(n) | Per-character scan | n = message length, cached after first parse |
| Render frame | O(v) | Read-only borrow | v = visible messages, dirty flag skips unchanged frames |
| SQLite insert | O(log n) B-tree | spawn_blocking | Never blocks the event loop |
| Channel switch | O(1) + SQLite | Async pipeline | Cache lookup O(1), SQLite fetch async, REST fetch async |

### Potential Bottlenecks & Mitigations

| Bottleneck | When | Mitigation |
|------------|------|------------|
| Markdown parsing on large messages | Messages with heavy formatting | Cache parsed output in `CachedMessage.rendered`, invalidate only on MESSAGE_UPDATE |
| SQLite write contention | High-volume channels (>10 msg/s) | Batch inserts via `InsertMessages` variant, WAL mode for concurrent read/write |
| READY event processing | Initial connection (many guilds) | Process READY on background task, stream results to main loop |
| Terminal rendering large pane trees | >8 panes on small terminal | Minimum pane size check, refuse split if too small |
| Memory with many channels open | User opens 50+ channels | MAX_CACHED_MESSAGES_PER_CHANNEL (200) eviction, rest in SQLite |

---

## Key Bindings (Default)

### Global
| Key | Action |
|-----|--------|
| `Ctrl+b` | Pane prefix (start pane command sequence) |
| `Ctrl+p` | Open command palette |
| `Ctrl+q` | Quit application |
| `Tab` | Cycle focus: sidebar -> messages -> input |
| `Shift+Tab` | Reverse cycle focus |
| `Esc` | Return to Normal mode / close popups |

### Normal Mode (Navigation)
| Key | Action |
|-----|--------|
| `j` / `k` | Scroll messages down/up |
| `h` / `l` | Collapse/expand tree node or switch panel |
| `g` | Go to top of messages |
| `G` | Go to bottom of messages (follow mode) |
| `Ctrl+u` / `Ctrl+d` | Half-page scroll up/down |
| `i` | Enter Insert mode (focus input box) |
| `/` | Open search |
| `:` | Enter Command mode |
| `Enter` | Select/open highlighted item |
| `r` | Reply to highlighted message |
| `e` | Edit highlighted message (if own) |
| `d` | Delete highlighted message (if own) with confirmation |

### Insert Mode (Message Composition)
| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` | New line |
| `Esc` | Return to Normal mode |
| `Ctrl+a` | Attach file |
| `Up` | Edit last sent message |
| `Tab` | Autocomplete (mention, channel, emoji) |

### Pane Commands (after Ctrl+b prefix)
| Key | Action |
|-----|--------|
| `"` | Split horizontal |
| `%` | Split vertical |
| `x` | Close pane |
| `o` | Next pane |
| `z` | Toggle zoom |
| `s` | Toggle sidebar |
| `Arrow keys` | Move focus |
| `Ctrl+Arrow` | Resize pane |
| `{` / `}` | Swap pane |
| `Space` | Cycle layout |
| `q` | Show pane numbers |
| `0-9` | Select pane by number |

---

## Configuration

### Config File: `~/.config/discordinator/config.toml`

```toml
[general]
timestamp_format = "%H:%M"
message_cache_size = 10000         # max messages per channel in SQLite
show_typing_indicator = true
desktop_notifications = true
render_fps = 60

[auth]
# Token source priority: env var DISCORD_TOKEN -> keyring -> file
# On first launch, user is prompted with login form (email/pass, QR, or paste token)
# After first login, token is stored according to token_source
token_source = "keyring"
# token_file = "~/.config/discordinator/token"  # if token_source = "file"

[discord]
# Anti-detection: keep these updated to match the real Discord web client
# Check https://discord.com/app and inspect network requests for current values
client_build_number = 346892
browser_version = "131.0.0.0"
browser_user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

[appearance]
theme = "default"                   # or path to custom theme TOML
show_sidebar = true                 # initial sidebar visibility
sidebar_width = 24
message_date_separator = true       # Show date headers between messages

[pane]
prefix_key = "Ctrl+b"
border_style = "rounded"            # plain, rounded, double, thick
active_border_color = "cyan"
inactive_border_color = "gray"
show_pane_title = true

[keybindings]
# Override any default key binding
# "Ctrl+n" = "pane:split_vertical"
# "Ctrl+s" = "pane:split_horizontal"

[session]
auto_save = true
restore_on_start = true
```

---

## Environment & Development

### Prerequisites
- Nix with flakes enabled
- Rust 1.89+ (provided by nix flake via rust-overlay)
- A Discord account

### Development Commands

```bash
# Enter development environment
nix develop

# Build
cargo build

# Run
cargo run

# Run with debug logging
RUST_LOG=debug cargo run

# Run tests
cargo test

# Run tests verbose
cargo test -- --nocapture

# Clippy lint
cargo clippy -- -D warnings

# Format check
cargo fmt --check
```

### Environment Variables
| Variable | Description |
|----------|-------------|
| `DISCORD_TOKEN` | Discord user token (highest priority auth source) |
| `RUST_LOG` | Log level filter (error, warn, info, debug, trace) |
| `DISCORDINATOR_CONFIG` | Custom config file path (overrides XDG default) |

---

## Testing Strategy

### Unit Tests
- Discord markdown parser (input -> output spans, edge cases)
- Pane tree operations (split, close, resize, focus navigation, traversal)
- Cache operations (insert, update, eviction, mention resolution)
- Key binding resolution (mode-aware dispatch)
- Configuration parsing (defaults, overrides, invalid input)
- SQLite operations (CRUD, schema migration, concurrent access)

### Integration Tests (Mock Discord Server)
- Gateway connection, IDENTIFY, READY event handling
- HTTP client with anti-detection headers verification
- Message lifecycle (send -> gateway echo -> cache -> render)
- Pane layout rendering (verify Layout constraints produce correct areas)
- Channel switching (SQLite cache -> REST fetch -> merge)
- Full startup -> login -> navigate -> send message flow

### Mock Discord Server
- Lightweight WebSocket server mimicking Discord gateway
- Sends HELLO, accepts IDENTIFY, sends READY with test data
- Sends MESSAGE_CREATE/UPDATE/DELETE events on demand
- Mock HTTP endpoints for message history, send, edit, delete
- Used by both integration tests and offline development

---

## Non-Goals (Out of Scope)

- Bot account support (this is a user client using the user API)
- Discord voice/video (Phase 5 experimental only)
- Full Discord API coverage (focus on chat features)
- Mobile/touch support
- GUI/graphical rendering (TUI only)
- Plugin marketplace/distribution
- Self-hosting Discord alternative protocol support
- Automated actions / spam / abuse tooling (strictly a chat client)
- Creating new DM channels via API (safety: never call POST /users/@me/channels)

---

## Known Risks & Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Discord ToS violation (user client API) | High | Clear disclaimer on first launch, user assumes all risk |
| Account detection/ban | Medium | Full anti-detection strategy: mimic web client IDENTIFY properties, respect rate limits, avoid suspicious endpoints (especially POST /users/@me/channels) |
| API rate limiting | Medium | Custom per-route rate limiter in HTTP actor, exponential backoff, 50-150ms request jitter |
| IDENTIFY properties becoming stale | Medium | Configurable `client_build_number` and browser version in config file, periodic update reminders |
| Custom gateway bugs (vs battle-tested twilight) | Medium | Comprehensive mock gateway tests, heartbeat/RESUME edge case coverage, connection state machine prevents impossible states |
| Gateway zlib-stream decompression edge cases | Low | Use flate2 with persistent decompressor state, test with real-world payloads from mock server |
| Terminal compatibility (images, unicode) | Low | Graceful degradation, ratatui-image auto-detects protocol support |
| SQLite blocking event loop | Low | All SQLite via `spawn_blocking`, never on main task |

---

## References

- [Discord API Documentation (Unofficial)](https://discord.com/developers/docs)
- [ratatui Documentation](https://ratatui.rs/)
- [twilight-model Documentation](https://docs.rs/twilight-model/) - Used for shared data types only
- [Oxicord Source](https://github.com/linuxmobile/oxicord) - Rust TUI client, Clean Architecture reference, custom gateway/HTTP (not twilight-gateway)
- [Discordo Source](https://github.com/ayn2op/discordo) - Go TUI client (user API, 5.1k stars), O(1) lookup patterns, virtual row building
- [Endcord](https://github.com/sparklost/endcord) - Python TUI client (user API, most features)
- [tmux Key Bindings Reference](https://tmuxcheatsheet.com/)
- [Discord Gateway Reference](https://discord.com/developers/docs/events/gateway) - Opcodes, payloads, compression

---

## Notes for Claude (Ralph Loop)

When working on tasks:
1. **Work through Ralph Tasks sequentially** (Task 1, 2, 3... up to Task 40) - earlier tasks set up foundations for later ones
2. **Always write tests first** before implementing a feature (TDD)
3. **Run tests** after every change: `nix develop --command cargo test`
4. **Run clippy** to catch issues: `nix develop --command cargo clippy -- -D warnings`
5. **Each task should be completable in a single iteration** - if stuck, log what's blocking and move to next unblocked task
6. **Mark checkboxes** in the Features section when tasks are complete
7. **Do not skip tests** - every feature must have corresponding tests
8. **The pane system is the core differentiator** - ensure it works flawlessly before moving to Phase 3+
9. **Never call POST /users/@me/channels** - use DM channels from READY event only
10. **All commands run in `nix develop`** - do not install anything globally
11. **Custom gateway, not twilight-gateway** - twilight-gateway is bot-only. Use tokio-tungstenite with custom IDENTIFY
12. **Custom HTTP, not twilight-http** - twilight-http adds `Bot ` prefix. Use reqwest with anti-detection headers
13. **twilight-model is OK** - use it for shared types (`Id<T>`, `Message`, `Channel`, `Guild`, etc.)
14. **No Arc<Mutex<_>>** - main loop owns all state, background tasks communicate via mpsc channels
15. **Dirty flag rendering** - set `dirty = true` on state change, only render when dirty
