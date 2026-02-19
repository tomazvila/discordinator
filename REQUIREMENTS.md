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

| Component | Version | Rationale |
|-----------|---------|-----------|
| Language | Rust 2021 edition | Performance, safety, strong async ecosystem |
| TUI Framework | ratatui 0.30.0 | Best Rust TUI, modularized workspace, Layout system for split panes |
| Terminal Backend | crossterm (via ratatui-crossterm) | Cross-platform (Linux, macOS, Windows), bundled with ratatui 0.30 |
| Discord Gateway | twilight-gateway 0.17.1 | Modular, async Stream-based, zstd compression default, MSRV 1.89 |
| Discord HTTP | twilight-http 0.17.1 | Built-in rate limiting, brotli decompression, MSRV 1.89 |
| Discord Models | twilight-model 0.17.x | Type-safe Discord data structures |
| Discord Cache | twilight-cache-inmemory 0.17.x | In-process cache for guilds/channels/users |
| Async Runtime | tokio 1.x | Required by both ratatui and twilight |
| Database | rusqlite 0.32.x + r2d2 | SQLite for local message persistence and session storage |
| Serialization | serde 1.x + serde_json 1.x | Standard Rust serialization |
| Keyring | keyring 3.6.x | Secure cross-platform token storage (apple-native, sync-secret-service) |
| Markdown | Custom parser | Discord-flavored markdown with mentions, emoji, spoilers |
| Image Display | ratatui-image 10.0.5 | Sixel/Kitty/iTerm2/halfblocks protocol support, ratatui 0.30 compatible |
| Text Input | Custom (built on ratatui) | tui-textarea 0.7.0 only supports ratatui 0.29; build custom input widget |
| Logging | tracing 0.1.x + tracing-subscriber 0.3.x | Structured async-aware logging to file |
| Error Handling | color-eyre 0.6.x | Pretty error reports, backtraces, graceful terminal restore on panic |
| Config | toml 0.8.x | TOML config file parsing |
| Directories | dirs 6.x | XDG-compliant platform directory resolution |

---

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Terminal (crossterm)               │
├─────────────────────────────────────────────────────┤
│                    UI Layer (ratatui)                 │
│  ┌──────────┬──────────────┬──────────────┐         │
│  │ Server/  │   Pane 1     │   Pane 2     │         │
│  │ Channel  │  (messages)  │  (messages)  │         │
│  │ Tree     ├──────────────┼──────────────┤         │
│  │(toggle)  │   Pane 3     │   Pane 4     │         │
│  │          │  (messages)  │  (messages)  │         │
│  └──────────┴──────────────┴──────────────┘         │
├─────────────────────────────────────────────────────┤
│              Application State (AppState)            │
│  ┌────────────┬───────────┬──────────────┐          │
│  │ PaneManager│ GuildState│   SQLite DB  │          │
│  └────────────┴───────────┴──────────────┘          │
├─────────────────────────────────────────────────────┤
│              Discord Layer (twilight)                 │
│  ┌────────────┬───────────┬──────────────┐          │
│  │  Gateway   │   HTTP    │    Cache     │          │
│  │  (events)  │  (REST)   │  (in-memory) │          │
│  └────────────┴───────────┴──────────────┘          │
└─────────────────────────────────────────────────────┘
```

### Event Loop Architecture

The application uses a `tokio::select!` hub pattern with three event sources:

```rust
loop {
    tokio::select! {
        // Terminal input (keyboard, mouse, resize)
        event = terminal_events.next() => {
            handle_input(event, &mut app);
        }
        // Discord gateway events (messages, presence, etc.)
        event = gateway.next() => {
            handle_discord(event, &mut app);
        }
        // Render tick at 60 FPS (~16ms)
        _ = render_tick.tick() => {
            terminal.draw(|f| ui(f, &app))?;
        }
    }
}
```

Heavy operations (HTTP requests, SQLite writes, markdown parsing) are spawned as separate tokio tasks communicating via `tokio::mpsc` channels to keep the render loop responsive.

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
├── main.rs              # Entry point, tokio runtime, panic handler
├── app.rs               # AppState, main event loop (tokio::select!)
├── auth.rs              # Login flow: token, email+pass+2FA, QR code
├── config.rs            # Configuration (TOML file, XDG dirs)
├── db.rs                # SQLite database (messages, sessions, settings)
├── discord/
│   ├── mod.rs
│   ├── gateway.rs       # Gateway connection, event handling, anti-detection
│   ├── http_client.rs   # REST API wrapper with anti-detection headers
│   ├── cache.rs         # In-memory cache for guilds, channels, users
│   └── models.rs        # App-specific model extensions
├── ui/
│   ├── mod.rs
│   ├── layout.rs        # Main layout rendering
│   ├── pane.rs          # Pane abstraction (split tree)
│   ├── pane_manager.rs  # Pane CRUD, focus management
│   ├── login.rs         # TUI login screen (token/email/QR)
│   ├── widgets/
│   │   ├── mod.rs
│   │   ├── server_tree.rs   # Server/channel tree sidebar (toggleable)
│   │   ├── message_view.rs  # Message list display
│   │   ├── input_box.rs     # Message composition (custom widget)
│   │   ├── member_list.rs   # Member sidebar
│   │   ├── status_bar.rs    # Bottom status bar
│   │   └── command_palette.rs # Ctrl+P command palette
│   └── theme.rs         # Color scheme and styling
├── input/
│   ├── mod.rs
│   ├── handler.rs       # Key event dispatch
│   ├── mode.rs          # Normal/Insert/Command modes (vim-like)
│   └── keybindings.rs   # Configurable key bindings
├── markdown/
│   ├── mod.rs
│   ├── parser.rs        # Discord-flavored markdown parser
│   └── renderer.rs      # Markdown to ratatui Spans/Lines
└── utils/
    ├── mod.rs
    └── unicode.rs        # Unicode width helpers

tests/
├── mock_discord/        # Mock Discord gateway + HTTP server for testing
│   ├── mod.rs
│   ├── gateway.rs       # Mock WebSocket gateway
│   └── http.rs          # Mock REST API
├── test_pane_tree.rs    # Pane split/close/resize/focus tests
├── test_markdown.rs     # Markdown parser tests
├── test_cache.rs        # Cache operations tests
├── test_config.rs       # Config parsing tests
├── test_db.rs           # SQLite operations tests
├── test_keybindings.rs  # Key binding resolution tests
└── test_integration.rs  # Full lifecycle tests with mock server
```

### Startup Flow

1. Initialize color-eyre panic handler (restores terminal on panic)
2. Load config from XDG config dir (create default if missing)
3. Initialize tracing (log to file in XDG data dir)
4. Check for existing token (keyring -> env var -> config file)
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

- Respect all HTTP rate limit headers (`X-RateLimit-*`)
- twilight-http handles per-route rate limiting automatically
- Global rate limit: stay well under 50 req/s
- Add jitter to request timing to avoid machine-like patterns

### 5. Connection Behavior

- Use zstd transport compression (default in twilight-gateway, matches web client)
- Maintain proper heartbeat intervals as provided by the gateway
- Handle session invalidation gracefully (re-IDENTIFY, don't rapid-reconnect)
- Use a single shard (user accounts don't shard)

---

## Features

### Phase 1: Core MVP (Single-Pane Client)

Phase 1 builds a traditional single-view Discord client. The pane system comes in Phase 2.

#### P1.1 - Authentication & Connection
- [ ] Token-based authentication (environment variable `DISCORD_TOKEN`, keyring, or config file)
- [ ] TUI login form: email + password + optional 2FA code
- [ ] QR code authentication (render QR in terminal for Discord mobile scan)
- [ ] Secure token storage in OS keyring via `keyring` crate (apple-native on macOS, secret-service on Linux)
- [ ] Connect to Discord gateway via WebSocket (wss://gateway.discord.gg/?v=10&encoding=json)
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
- Create `Cargo.toml` with all dependencies
- Create `src/main.rs` with basic tokio entry point and color-eyre setup
- Verify `cargo build` succeeds in `nix develop`
- Test: `cargo test` passes (even if no tests yet)
- Files: `Cargo.toml`, `src/main.rs`

**Task 2: Configuration module**
- Create `src/config.rs` with `AppConfig` struct
- XDG directory resolution using `dirs` crate
- TOML config loading with defaults
- Create default config on first run
- Test: parse config, missing config creates default, invalid config errors gracefully
- Files: `src/config.rs`, `tests/test_config.rs`

**Task 3: SQLite database module**
- Create `src/db.rs` with schema initialization
- Tables: `messages`, `channels`, `guilds`, `sessions`
- CRUD operations for messages (insert, query by channel, update, delete)
- Session save/load operations
- Test: all CRUD operations, schema migration
- Files: `src/db.rs`, `tests/test_db.rs`

**Task 4: Logging setup**
- Configure tracing-subscriber to log to file in XDG data dir
- Log rotation (keep last 5 log files, max 10MB each)
- RUST_LOG env var support
- Test: verify log file is created, messages are written
- Files: `src/main.rs` (logging init)

**Task 5: Error handling**
- Set up color-eyre with custom panic handler
- Panic handler restores terminal state (disable raw mode, show cursor) before printing error
- Test: verify panic handler works (can be a manual test)
- Files: `src/main.rs`

### Discord Connection (Tasks 6-10)

**Task 6: Anti-detection module**
- Create `src/discord/gateway.rs` with IDENTIFY properties struct
- Configurable `client_build_number`, `browser_version`, `browser_user_agent` from config
- X-Super-Properties base64 encoding for HTTP headers
- Test: verify IDENTIFY payload matches expected format, X-Super-Properties encoding is correct
- Files: `src/discord/gateway.rs`, `src/discord/mod.rs`

**Task 7: Gateway connection**
- Connect to Discord gateway using twilight-gateway
- Custom IDENTIFY with anti-detection properties
- Heartbeat/ACK handling
- Receive READY event and extract guilds, channels, user info, DM channels
- Test: connect to mock gateway, verify READY handling
- Files: `src/discord/gateway.rs`, `tests/mock_discord/mod.rs`, `tests/mock_discord/gateway.rs`

**Task 8: HTTP client with anti-detection**
- Create `src/discord/http_client.rs` wrapping twilight-http
- Add anti-detection headers (User-Agent, X-Super-Properties, X-Discord-Locale) to all requests
- Request jitter (small random delay) to avoid machine-like patterns
- Test: verify headers are set correctly on mock HTTP server
- Files: `src/discord/http_client.rs`, `tests/mock_discord/http.rs`

**Task 9: In-memory cache**
- Create `src/discord/cache.rs` using twilight-cache-inmemory
- Cache guilds, channels, users, presences from gateway events
- Helper methods: get guild name, get channel name, get user display name, resolve mention
- Test: populate cache from mock READY event, verify lookups
- Files: `src/discord/cache.rs`, `tests/test_cache.rs`

**Task 10: Authentication module**
- Create `src/auth.rs` with token management
- Token sources: env var `DISCORD_TOKEN` -> keyring -> config file
- Token validation (attempt gateway connect, check for invalid token error)
- Store valid token in keyring
- Test: token retrieval priority, keyring storage (mocked)
- Files: `src/auth.rs`

### UI Foundation (Tasks 11-18)

**Task 11: Terminal setup and main loop**
- Create `src/app.rs` with `App` struct holding all state
- Set up crossterm raw mode, alternate screen
- Main `tokio::select!` loop: terminal events + render tick (60 FPS)
- Graceful shutdown on Ctrl+C / Ctrl+Q (restore terminal)
- Test: app starts and stops cleanly
- Files: `src/app.rs`, `src/main.rs`

**Task 12: Input mode system**
- Create `src/input/mode.rs` with `InputMode` enum (Normal, Insert, Command, PanePrefix)
- Create `src/input/handler.rs` with key dispatch based on mode
- Mode transitions: `i` -> Insert, `Esc` -> Normal, `:` -> Command, `Ctrl+b` -> PanePrefix
- Test: mode transitions, key routing per mode
- Files: `src/input/mod.rs`, `src/input/mode.rs`, `src/input/handler.rs`

**Task 13: Theme and styling**
- Create `src/ui/theme.rs` with color definitions
- Default theme matching Discord dark mode colors
- Theme struct with all configurable colors (background, text, borders, highlight, mention, etc.)
- Test: theme loads defaults, custom colors override
- Files: `src/ui/theme.rs`

**Task 14: Status bar widget**
- Create `src/ui/widgets/status_bar.rs`
- Display: connection status (icon), current server > #channel, input mode, unread/mention counts
- Test: renders correct content for each connection state
- Files: `src/ui/widgets/status_bar.rs`, `src/ui/widgets/mod.rs`

**Task 15: Server/channel tree widget**
- Create `src/ui/widgets/server_tree.rs`
- Render guild list with collapsible categories and channels
- DM section from READY event data
- Unread/mention indicators (bold, bullet, red)
- Arrow key / j/k navigation, Enter to select
- Toggle visibility with keybinding
- Test: renders tree correctly, navigation works, collapse/expand
- Files: `src/ui/widgets/server_tree.rs`

**Task 16: Message view widget**
- Create `src/ui/widgets/message_view.rs`
- Render messages with author, timestamp, content
- Message selection highlight (for reply/edit/delete)
- Auto-scroll to bottom on new messages (follow mode), stop auto-scroll on manual scroll up
- Date separator between messages from different days
- Test: renders messages, selection, auto-scroll behavior
- Files: `src/ui/widgets/message_view.rs`

**Task 17: Input box widget**
- Create `src/ui/widgets/input_box.rs` (custom, not tui-textarea)
- Single-line by default, expand to multi-line with Shift+Enter
- Basic text editing: cursor movement, insert, delete, home/end
- Enter to send, Esc to cancel/return to Normal mode
- Visual feedback for reply mode (show "Replying to @user" header)
- Test: text insertion, cursor movement, send action
- Files: `src/ui/widgets/input_box.rs`

**Task 18: Main layout composition**
- Create `src/ui/layout.rs`
- Compose: optional sidebar | main content | status bar
- Sidebar width configurable, toggleable with Ctrl+b s
- Main content area hosts the single pane (Phase 1)
- Test: layout renders correctly with/without sidebar
- Files: `src/ui/layout.rs`, `src/ui/mod.rs`

### Core Features (Tasks 19-25)

**Task 19: Channel switching**
- Select channel in sidebar -> update current pane's channel
- Fetch message history from SQLite first, then REST (newer messages)
- Store fetched messages in SQLite
- Update status bar with new channel info
- Test: channel switch updates view, messages load correctly
- Files: `src/app.rs` (event handling)

**Task 20: Message sending**
- In Insert mode, Enter sends message content via twilight-http
- Clear input box after successful send
- Handle send failures (rate limit, permission denied) with status bar error
- Store sent message in SQLite on gateway confirmation (MESSAGE_CREATE)
- Test: send message via mock HTTP, verify gateway echo
- Files: `src/app.rs`, `src/discord/http_client.rs`

**Task 21: Message editing**
- In Normal mode, press `e` on own message -> populate input box with message content
- Input box shows "Editing message" header
- Enter sends PATCH request to update message
- Handle edit failure gracefully
- Test: edit flow via mock server
- Files: `src/app.rs`

**Task 22: Message deletion**
- In Normal mode, press `d` on own message -> show confirmation prompt
- `y` confirms deletion via DELETE request
- Remove message from view and SQLite
- Test: delete flow with confirmation via mock server
- Files: `src/app.rs`

**Task 23: Reply to message**
- In Normal mode, press `r` on any message -> enter Insert mode with reply context
- Input box shows "Replying to @author: message preview..." header
- Send message with `message_reference` field
- Test: reply flow, message_reference is set correctly
- Files: `src/app.rs`

**Task 24: Message history scrolling**
- Scroll up past cached messages -> fetch older messages from REST
- Insert fetched messages into SQLite and prepend to view
- Loading indicator while fetching
- Test: scroll triggers fetch, messages prepend correctly
- Files: `src/ui/widgets/message_view.rs`, `src/app.rs`

**Task 25: Gateway event handling**
- Handle MESSAGE_CREATE -> insert into cache, SQLite, update relevant pane(s)
- Handle MESSAGE_UPDATE -> update in cache, SQLite, re-render
- Handle MESSAGE_DELETE -> remove from cache, SQLite, re-render
- Handle TYPING_START -> show typing indicator
- Handle GUILD_CREATE/UPDATE -> update cache
- Handle CHANNEL_CREATE/UPDATE/DELETE -> update cache and sidebar
- Test: each event type via mock gateway
- Files: `src/app.rs`, `src/discord/gateway.rs`

### Discord Markdown (Tasks 26-28)

**Task 26: Markdown parser (core)**
- Create `src/markdown/parser.rs`
- Parse: **bold**, *italic*, __underline__, ~~strikethrough~~, `inline code`, ```code blocks```
- Parse: user mentions `<@id>`, channel mentions `<#id>`, role mentions `<@&id>`
- Parse: custom emoji `<:name:id>` and `<a:name:id>` (animated)
- Output: Vec of styled spans (text + style attributes)
- Test: extensive parser tests for each markdown element, edge cases, nested formatting
- Files: `src/markdown/parser.rs`, `src/markdown/mod.rs`, `tests/test_markdown.rs`

**Task 27: Markdown renderer**
- Create `src/markdown/renderer.rs`
- Convert parsed spans to ratatui `Spans`/`Line` with appropriate styles
- Resolve mentions using cache (user ID -> display name, channel ID -> #channel-name)
- Role mentions use role color from cache
- Test: rendered output matches expected styled spans
- Files: `src/markdown/renderer.rs`

**Task 28: Markdown integration**
- Integrate markdown parser+renderer into message_view widget
- Messages render with full formatting
- Test: full message rendering pipeline
- Files: `src/ui/widgets/message_view.rs`

### Pane System (Tasks 29-35)

**Task 29: Pane tree data structure**
- Create `src/ui/pane.rs` with `PaneNode` enum (Leaf/Split)
- Binary tree operations: insert (split), remove (close), find by ID
- Tree traversal: in-order for pane numbering, find by direction for focus movement
- Test: extensive tree manipulation tests
- Files: `src/ui/pane.rs`, `tests/test_pane_tree.rs`

**Task 30: Pane manager**
- Create `src/ui/pane_manager.rs` with `PaneManager`
- Split pane (horizontal/vertical) at current focus
- Close pane (collapse parent split node)
- Move focus directionally (up/down/left/right based on rendered positions)
- Cycle focus (next pane in tree order)
- Test: all pane operations
- Files: `src/ui/pane_manager.rs`

**Task 31: Pane rendering**
- Integrate pane tree into layout.rs
- Recursively render pane tree as nested ratatui Layout splits
- Each leaf pane renders its own message_view + input_box
- Active pane gets highlighted border, inactive panes get dim border
- Pane title bar with `server > #channel`
- Test: verify layout produces correct areas for various tree shapes
- Files: `src/ui/layout.rs`

**Task 32: Pane prefix keybindings**
- Implement PanePrefix input mode
- After Ctrl+b, wait for next key to determine pane operation
- " -> split horizontal, % -> split vertical, x -> close, o -> next, z -> zoom
- Arrow keys -> directional focus, Ctrl+Arrow -> resize
- Timeout or Esc cancels prefix mode
- Test: each pane keybinding triggers correct operation
- Files: `src/input/handler.rs`

**Task 33: Pane zoom**
- Ctrl+b z toggles zoom on focused pane
- Zoomed: render only the focused pane at full size (hide pane tree)
- Unzoom: restore previous layout
- Status bar shows zoom indicator
- Test: zoom/unzoom preserves pane state
- Files: `src/ui/pane_manager.rs`, `src/ui/layout.rs`

**Task 34: Pane channel assignment**
- When in a pane, channel selection from sidebar assigns channel to that pane
- Each pane subscribes to gateway events for its channel
- New messages appear in all panes viewing that channel
- Test: multi-pane with different channels, events route correctly
- Files: `src/app.rs`

**Task 35: Pane session persistence**
- Save pane tree layout to SQLite on exit (tree structure + channel IDs per pane)
- Restore pane layout from SQLite on startup
- Auto-save session periodically
- Test: save and restore produces identical layout
- Files: `src/db.rs`, `src/ui/pane_manager.rs`

### Login UI (Tasks 36-38)

**Task 36: Token paste login**
- Create `src/ui/login.rs` with login screen
- Option 1: Paste token directly
- Validate token by attempting gateway connection
- On success: store in keyring, proceed to main view
- Test: login flow with valid/invalid mock tokens
- Files: `src/ui/login.rs`

**Task 37: Email + password login**
- Login screen option 2: email + password form
- POST to Discord's login endpoint
- Handle 2FA challenge (prompt for TOTP code)
- Extract token from response
- Test: login flow via mock HTTP endpoints
- Files: `src/ui/login.rs`, `src/auth.rs`

**Task 38: QR code login**
- Login screen option 3: QR code
- Generate QR code for Discord mobile app scan
- Poll for scan completion
- Extract token from response
- Test: QR generation, poll cycle
- Files: `src/ui/login.rs`, `src/auth.rs`

---

## Data Models

### AppState
```rust
struct App {
    // Discord connection
    gateway: GatewayConnection,
    http: HttpClient,
    cache: DiscordCache,
    db: Database,

    // UI state
    pane_manager: PaneManager,
    sidebar: SidebarState,
    sidebar_visible: bool,
    command_palette: Option<CommandPaletteState>,

    // User
    current_user: CurrentUser,

    // Settings
    config: AppConfig,

    // Input
    input_mode: InputMode,
}
```

### PaneManager (Binary Tree)
```rust
struct PaneManager {
    root: PaneNode,
    focused_pane_id: PaneId,
    zoom_state: Option<PaneId>,  // If a pane is zoomed
    next_id: u32,                // ID counter
}

enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,           // 0.0-1.0, position of divider
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
    channel_id: Option<ChannelId>,
    guild_id: Option<GuildId>,
    scroll_state: ScrollState,
    input: InputState,
    follow_mode: bool,  // auto-scroll to new messages
}

enum ScrollState {
    Following,                    // At bottom, auto-scroll
    Manual { offset: usize },     // User scrolled up
}
```

### Database Schema
```sql
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,       -- Discord snowflake
    channel_id INTEGER NOT NULL,
    author_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,       -- ISO 8601
    edited_timestamp TEXT,
    attachments TEXT,              -- JSON array
    embeds TEXT,                   -- JSON array
    message_reference TEXT,        -- JSON (for replies)
    UNIQUE(id)
);
CREATE INDEX idx_messages_channel ON messages(channel_id, timestamp);

CREATE TABLE IF NOT EXISTS channels (
    id INTEGER PRIMARY KEY,
    guild_id INTEGER,
    name TEXT NOT NULL,
    type INTEGER NOT NULL,
    position INTEGER DEFAULT 0,
    parent_id INTEGER,            -- category
    last_read_message_id INTEGER
);

CREATE TABLE IF NOT EXISTS guilds (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    icon TEXT
);

CREATE TABLE IF NOT EXISTS sessions (
    name TEXT PRIMARY KEY,
    pane_layout TEXT NOT NULL,    -- JSON serialized pane tree
    last_used TEXT NOT NULL       -- ISO 8601
);
```

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
| API rate limiting | Medium | Built-in rate limiter via twilight-http, exponential backoff, request jitter |
| IDENTIFY properties becoming stale | Medium | Configurable `client_build_number` and browser version in config file, periodic update reminders |
| Gateway zstd compression changes | Low | twilight-gateway handles this natively, keep dependencies updated |
| Terminal compatibility (images, unicode) | Low | Graceful degradation, ratatui-image auto-detects protocol support |

---

## References

- [Discord API Documentation (Unofficial)](https://discord.com/developers/docs)
- [ratatui Documentation](https://ratatui.rs/)
- [twilight Documentation](https://twilight.rs/)
- [Discordo](https://github.com/ayn2op/discordo) - Go TUI client (user API, 5.1k stars)
- [Oxicord](https://github.com/linuxmobile/oxicord) - Rust TUI client (user API, ratatui)
- [Endcord](https://github.com/sparklost/endcord) - Python TUI client (user API, most features)
- [tmux Key Bindings Reference](https://tmuxcheatsheet.com/)

---

## Notes for Claude (Ralph Loop)

When working on tasks:
1. **Work through Ralph Tasks sequentially** (Task 1, 2, 3...) - earlier tasks set up foundations for later ones
2. **Always write tests first** before implementing a feature (TDD)
3. **Run tests** after every change: `nix develop --command cargo test`
4. **Run clippy** to catch issues: `nix develop --command cargo clippy -- -D warnings`
5. **Each task should be completable in a single iteration** - if stuck, log what's blocking and move to next unblocked task
6. **Mark checkboxes** in the Features section when tasks are complete
7. **Do not skip tests** - every feature must have corresponding tests
8. **The pane system is the core differentiator** - ensure it works flawlessly before moving to Phase 3+
9. **Never call POST /users/@me/channels** - use DM channels from READY event only
10. **All commands run in `nix develop`** - do not install anything globally
