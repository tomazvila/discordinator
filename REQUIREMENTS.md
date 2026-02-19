# Discordinator - Requirements Document

## Project Overview

**Discordinator** is a Discord TUI (Terminal User Interface) client written in Rust. Its defining feature is a **tmux-like split pane system** that allows users to simultaneously view and interact with multiple channels across different servers in a single terminal window.

## Goal

Build a fully functional Discord TUI client in Rust with tmux-like pane management, enabling power users to monitor and participate in multiple conversations simultaneously.

## Success Criteria

- [ ] User can authenticate and connect to Discord gateway
- [ ] Server list, channel tree, and message view render correctly
- [ ] Messages can be sent, edited, and deleted
- [ ] tmux-like pane splitting works (horizontal/vertical) with independent channel views
- [ ] Vim-style keyboard navigation throughout the application
- [ ] All core features pass automated tests
- [ ] Application builds and runs via `nix develop`
- [ ] Performance: <50ms render time, <100MB RSS memory for typical usage

---

## Technology Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust (2021 edition) | Performance, safety, strong async ecosystem |
| TUI Framework | ratatui 0.30+ | Best Rust TUI framework, excellent Layout system for split panes |
| Terminal Backend | crossterm | Cross-platform (Linux, macOS, Windows) |
| Discord Gateway | twilight-gateway | Modular, async Stream-based, supports zstd compression |
| Discord HTTP | twilight-http | Built-in rate limiting, modular |
| Discord Models | twilight-model | Type-safe Discord data structures |
| Async Runtime | tokio | Required by both ratatui and twilight |
| Serialization | serde + serde_json | Standard Rust serialization |
| Keyring | keyring crate | Secure token storage |
| Markdown | Custom parser | Discord-flavored markdown with mentions, emoji, spoilers |
| Image Display | ratatui-image | Sixel/Kitty/iTerm2 protocol support |
| Text Input | tui-textarea | Multi-line input with editing features |
| Logging | tracing + tracing-subscriber | Structured async-aware logging |

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
│  │          │   Pane 3     │   Pane 4     │         │
│  │          │  (messages)  │  (messages)  │         │
│  └──────────┴──────────────┴──────────────┘         │
├─────────────────────────────────────────────────────┤
│              Application State (AppState)            │
│  ┌────────────┬───────────┬──────────────┐          │
│  │ PaneManager│ GuildState│ MessageCache │          │
│  └────────────┴───────────┴──────────────┘          │
├─────────────────────────────────────────────────────┤
│              Discord Layer (twilight)                 │
│  ┌────────────┬───────────┬──────────────┐          │
│  │  Gateway   │   HTTP    │    Cache     │          │
│  │  (events)  │  (REST)   │  (in-memory) │          │
│  └────────────┴───────────┴──────────────┘          │
└─────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── main.rs              # Entry point, tokio runtime setup
├── app.rs               # AppState, main event loop (tokio::select!)
├── auth.rs              # Token management (keyring, env, file)
├── config.rs            # Configuration (TOML file)
├── discord/
│   ├── mod.rs
│   ├── gateway.rs       # Gateway connection, event handling
│   ├── http_client.rs   # REST API wrapper
│   ├── cache.rs         # In-memory cache for guilds, channels, users
│   └── models.rs        # App-specific model extensions
├── ui/
│   ├── mod.rs
│   ├── layout.rs        # Main layout rendering
│   ├── pane.rs          # Pane abstraction (split tree)
│   ├── pane_manager.rs  # Pane CRUD, focus management
│   ├── widgets/
│   │   ├── mod.rs
│   │   ├── server_tree.rs   # Server/channel tree sidebar
│   │   ├── message_view.rs  # Message list display
│   │   ├── input_box.rs     # Message composition
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
```

---

## Features

### Phase 1: Core MVP

#### P1.1 - Authentication & Connection
- [ ] Token-based authentication (environment variable `DISCORD_TOKEN` or keyring)
- [ ] Connect to Discord gateway via WebSocket
- [ ] Handle heartbeat/ACK cycle
- [ ] Handle READY event and populate initial state
- [ ] Automatic reconnection with RESUME on disconnect
- [ ] Graceful error handling for invalid tokens

#### P1.2 - Basic UI Layout
- [ ] Server list sidebar (guild icons/names)
- [ ] Channel tree (categories, text channels, DM list)
- [ ] Message view (scrollable message history)
- [ ] Input box for composing messages
- [ ] Status bar (connection status, current server/channel, mode indicator)
- [ ] Focus management between panels (Tab/Shift+Tab)

#### P1.3 - Message Display
- [ ] Render message author, timestamp, content
- [ ] Discord-flavored markdown rendering (bold, italic, underline, strikethrough, code, code blocks)
- [ ] User mentions (`<@id>`) resolved to display names
- [ ] Channel mentions (`<#id>`) resolved to channel names
- [ ] Role mentions (`<@&id>`) with role color
- [ ] Custom emoji display (`<:name:id>` as `:name:`)
- [ ] Timestamp formatting (`<t:unix:format>`)
- [ ] Reply threading (show replied-to message preview)
- [ ] Edited message indicator
- [ ] System messages (joins, boosts, pins)
- [ ] Embeds (title, description, fields, color bar)
- [ ] Attachment indicators (filename, size)

#### P1.4 - Message Interaction
- [ ] Send messages to current channel
- [ ] Edit own messages
- [ ] Delete own messages
- [ ] Reply to messages (with quote preview)
- [ ] Message history scrolling with lazy loading (fetch older on scroll up)
- [ ] Typing indicator (send and display)

#### P1.5 - Navigation
- [ ] Vim-style navigation: j/k for messages, h/l for panels
- [ ] g/G for top/bottom of message list
- [ ] Ctrl+u/Ctrl+d for half-page scroll
- [ ] / for channel search/filter
- [ ] Server switching via number keys or fuzzy search
- [ ] Channel switching via tree navigation or fuzzy search
- [ ] Unread indicators (bold channel names, bullet markers)
- [ ] Mention indicators (red channel names)

### Phase 2: Pane Management (Core Differentiator)

#### P2.1 - Pane System Architecture
- [ ] Binary tree pane layout (each node is either a split or a leaf pane)
- [ ] Each leaf pane contains an independent channel view (messages + input)
- [ ] Pane focus management (one active pane at a time, highlighted border)
- [ ] Panes track their own scroll position, channel, and input state independently

#### P2.2 - Pane Operations (tmux-like)
- [ ] `Ctrl+b "` - Split current pane horizontally (top/bottom)
- [ ] `Ctrl+b %` - Split current pane vertically (left/right)
- [ ] `Ctrl+b x` - Close current pane
- [ ] `Ctrl+b o` - Cycle focus to next pane
- [ ] `Ctrl+b arrow` - Move focus directionally (up/down/left/right)
- [ ] `Ctrl+b z` - Toggle zoom (maximize/restore current pane)
- [ ] `Ctrl+b {` / `Ctrl+b }` - Swap pane with previous/next
- [ ] `Ctrl+b space` - Cycle through preset layouts (even-horizontal, even-vertical, tiled)
- [ ] `Ctrl+b q` - Flash pane numbers for quick selection
- [ ] Pane resize: `Ctrl+b Ctrl+arrow` to resize in direction

#### P2.3 - Pane Features
- [ ] Each pane can view a different server/channel independently
- [ ] Assign a channel to a pane via command palette or channel tree
- [ ] Pane title bar showing server > channel
- [ ] Active pane border highlighted (configurable color)
- [ ] Inactive panes still receive and display new messages in real-time
- [ ] Pane layouts persist across sessions (save/restore from config)

### Phase 3: Enhanced Features

#### P3.1 - Reactions
- [ ] Display reactions on messages (emoji + count)
- [ ] Add reactions to messages (emoji picker or type emoji name)
- [ ] Remove own reactions

#### P3.2 - Threads & Forums
- [ ] Display thread indicators on messages
- [ ] Open thread in a pane
- [ ] Forum channel listing (thread list view)
- [ ] Create threads

#### P3.3 - User Presence & Profiles
- [ ] Member list sidebar (toggleable per pane)
- [ ] Online/offline/idle/DND status indicators
- [ ] User profile popup (roles, join date, avatar)
- [ ] Activity/game status display

#### P3.4 - Direct Messages
- [ ] DM channel list
- [ ] Group DM support
- [ ] Open DM in a pane
- [ ] DM notifications

#### P3.5 - Search
- [ ] Message search within current channel
- [ ] Global message search (across server)
- [ ] Search results displayed in dedicated pane
- [ ] Filter by author, date, has:file, has:link, etc.

#### P3.6 - File Handling
- [ ] Upload files (via TUI file picker)
- [ ] Download attachments
- [ ] Image preview (Sixel/Kitty/iTerm2 via ratatui-image)
- [ ] Open attachments in external application

#### P3.7 - Notifications
- [ ] Desktop notifications for mentions
- [ ] Notification bell/counter in status bar
- [ ] Per-channel/server mute settings
- [ ] @mention highlighting in messages

#### P3.8 - Command Palette
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
- [ ] Command mode (`:` commands)
- [ ] Visual mode (text selection in messages for copying)

#### P4.3 - Session Management
- [ ] Save pane layouts as named sessions
- [ ] Restore sessions on startup
- [ ] Auto-save session on exit
- [ ] Multiple named sessions (like tmux sessions)

#### P4.4 - Scripting & Extensibility
- [ ] Lua scripting for custom commands
- [ ] Custom keybinding actions
- [ ] Webhook integration support
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

## Data Models

### AppState
```rust
struct AppState {
    // Discord connection
    gateway: GatewayConnection,
    http: HttpClient,
    cache: DiscordCache,

    // UI state
    pane_manager: PaneManager,
    sidebar: SidebarState,
    command_palette: Option<CommandPaletteState>,

    // User
    current_user: CurrentUser,
    token: String,

    // Settings
    config: AppConfig,

    // Mode
    input_mode: InputMode,
}
```

### PaneManager (Binary Tree)
```rust
struct PaneManager {
    root: PaneNode,
    focused_pane_id: PaneId,
    zoom_state: Option<PaneId>,  // If a pane is zoomed
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
    messages: Vec<CachedMessage>,
    scroll_offset: usize,
    input: InputState,
    title: String,
}
```

### DiscordCache
```rust
struct DiscordCache {
    guilds: HashMap<GuildId, CachedGuild>,
    channels: HashMap<ChannelId, CachedChannel>,
    users: HashMap<UserId, CachedUser>,
    messages: HashMap<ChannelId, VecDeque<CachedMessage>>,
    dm_channels: Vec<ChannelId>,
    typing: HashMap<ChannelId, Vec<(UserId, Instant)>>,
    unread: HashMap<ChannelId, UnreadState>,
}
```

---

## Key Bindings (Default)

### Global
| Key | Action |
|-----|--------|
| `Ctrl+b` | Pane prefix (start pane command sequence) |
| `Ctrl+p` | Open command palette |
| `Ctrl+q` | Quit application |
| `Tab` | Cycle focus: sidebar -> pane -> input |
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
message_cache_size = 1000          # per channel
show_typing_indicator = true
desktop_notifications = true

[auth]
# Token source: "keyring", "env", "file"
token_source = "keyring"
# token_file = "~/.config/discordinator/token"  # if token_source = "file"

[appearance]
theme = "default"                   # or path to custom theme
show_member_list = false            # default off, toggle per pane
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
session_file = "~/.config/discordinator/session.json"
```

---

## Environment & Development

### Prerequisites
- Nix with flakes enabled
- Rust (provided by nix flake)
- A Discord account with a user token

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

# Format
cargo fmt --check
```

### Environment Variables
| Variable | Description |
|----------|-------------|
| `DISCORD_TOKEN` | Discord user token (alternative to keyring) |
| `RUST_LOG` | Log level (error, warn, info, debug, trace) |
| `DISCORDINATOR_CONFIG` | Custom config file path |

---

## Testing Strategy

### Unit Tests
- Discord markdown parser (input -> output spans)
- Pane tree operations (split, close, resize, focus navigation)
- Cache operations (insert, update, eviction)
- Key binding resolution
- Configuration parsing

### Integration Tests
- Gateway connection and event handling (mock WebSocket server)
- HTTP client rate limiting (mock HTTP server)
- Full message lifecycle (receive event -> update cache -> render)
- Pane layout rendering (verify Layout constraints produce correct areas)

### End-to-End Tests
- Startup with mock Discord backend
- Navigation flow: server -> channel -> messages
- Pane split/close/resize workflow
- Message send/edit/delete flow

---

## Non-Goals (Out of Scope)

- Bot account support (this is a user client)
- Discord voice/video (Phase 5 experimental only)
- Full Discord API coverage (focus on chat features)
- Mobile/touch support
- GUI/graphical rendering (TUI only)
- Plugin marketplace/distribution
- Self-hosting Discord alternative protocol support

---

## Known Risks & Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Discord ToS violation (user client API) | High | Educate users, provide clear disclaimers, use responsibly |
| Account detection/ban | Medium | Mimic web client properties in IDENTIFY payload, avoid suspicious endpoints |
| API rate limiting | Medium | Built-in rate limiter via twilight-http, exponential backoff |
| Gateway zstd compression changes | Low | twilight-gateway handles this, keep dependencies updated |
| Terminal compatibility (images, unicode) | Low | Graceful degradation, ratatui-image handles protocol detection |

---

## References

- [Discord API Documentation (Unofficial)](https://discord.com/developers/docs)
- [ratatui Documentation](https://ratatui.rs/)
- [twilight Documentation](https://twilight.rs/)
- [Discordo](https://github.com/ayn2op/discordo) - Go TUI client
- [Oxicord](https://github.com/linuxmobile/oxicord) - Rust TUI client (new)
- [Endcord](https://github.com/sparklost/endcord) - Python TUI client (most features)
- [tmux Key Bindings Reference](https://tmuxcheatsheet.com/)

---

## Notes for Claude (Ralph Loop)

When working on tasks:
1. **Priority order**: P1 (Core MVP) -> P2 (Panes) -> P3 (Enhanced) -> P4 (Power User)
2. **Always write tests first** before implementing a feature
3. **Run tests** after every change: `cargo test`
4. **Run clippy** to catch issues: `cargo clippy -- -D warnings`
5. **Each task should be completable in a single iteration** - if a task is too large, work on a sub-component
6. **Mark checkboxes** in this file when tasks are complete
7. **Do not skip tests** - every feature must have corresponding tests
8. **The pane system is the core differentiator** - ensure it works flawlessly before moving to Phase 3+
