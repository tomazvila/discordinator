# CODEMAPS.md — Discordinator Codebase Map

**35 source files, ~16,746 lines of Rust**

## File Index

| File | Lines | Layer | Purpose |
|------|-------|-------|---------|
| `src/main.rs` | 662 | App | Entry point, async event loop, login wiring, DB worker, side-effect dispatch |
| `src/event_handler.rs` | 856 | App | Gateway event → cache mutation, background result handling, JSON parsers |
| `src/app.rs` | 869 | App | `AppState`, `apply_action()` central state mutation |
| `src/auth.rs` | 1072 | App | Token retrieval, email/password login, QR auth, `validate_token_via_gateway` |
| `src/config.rs` | 499 | App | TOML config with `AppConfig`, `DiscordConfig`, `AppDirs` |
| `src/logging.rs` | 233 | App | File logging with rotation, panic handler |
| `src/input/mod.rs` | 2 | App | Module declaration |
| `src/input/mode.rs` | 67 | App | `InputMode` enum: Normal, Insert, Command, PanePrefix |
| `src/input/handler.rs` | 352 | App | Key event → `Action` mapping per mode |
| `src/markdown/mod.rs` | 3 | App | Module declaration |
| `src/markdown/parser.rs` | 944 | App | Discord markdown → `MarkdownAst` parser |
| `src/markdown/renderer.rs` | 389 | App | `MarkdownAst` → ratatui `Line` renderer |
| `src/markdown/integration.rs` | 212 | App | Lazy render cache, `DiscordCacheResolver` |
| `src/domain/mod.rs` | 9 | Domain | Module declaration |
| `src/domain/types.rs` | 762 | Domain | All core types: `Action` (30 variants), `CachedMessage`, `ConnectionState`, `PaneId`, `ScrollState`, `InputState`, `HttpRequest`, `DbRequest`, `BackgroundResult` |
| `src/domain/cache.rs` | 858 | Domain | `DiscordCache` — in-memory guild/channel/user/message store |
| `src/domain/event.rs` | 729 | Domain | `GatewayEvent` enum, `parse_gateway_payload()` JSON parser |
| `src/domain/pane.rs` | 1733 | Domain | `PaneNode` binary tree, `PaneManager`, session serialization |
| `src/domain/markdown.rs` | 68 | Domain | `MarkdownAst`, `MarkdownSpan`, `MarkdownStyle` data types |
| `src/infrastructure/mod.rs` | 5 | Infra | Module declaration |
| `src/infrastructure/anti_detection.rs` | 255 | Infra | Chrome-mimicking headers, `X-Super-Properties`, `IdentifyProperties` |
| `src/infrastructure/db.rs` | 525 | Infra | SQLite: messages, channels, guilds, sessions tables |
| `src/infrastructure/gateway.rs` | 1407 | Infra | WebSocket gateway: `GatewayConnection`, `ZlibDecompressor`, `GatewayManager` with reconnect |
| `src/infrastructure/http_client.rs` | 718 | Infra | `HttpActor` — REST API with per-route rate limiting |
| `src/infrastructure/keyring.rs` | 128 | Infra | `TokenStore` trait, `KeyringStore`, `MemoryTokenStore` |
| `src/ui/mod.rs` | 5 | UI | Module declaration |
| `src/ui/theme.rs` | 312 | UI | `Theme` with Discord dark mode colors, `parse_color()` |
| `src/ui/layout.rs` | 336 | UI | Top-level layout: sidebar + pane_renderer + status bar |
| `src/ui/pane_renderer.rs` | 320 | UI | Recursive pane tree renderer with zoom support |
| `src/ui/login.rs` | 1084 | UI | Login screen: 3 auth methods, form state |
| `src/ui/widgets/mod.rs` | 4 | UI | Module declaration |
| `src/ui/widgets/input_box.rs` | 569 | UI | Message input widget, Unicode cursor management |
| `src/ui/widgets/message_view.rs` | 556 | UI | Message list with scroll, date separators, attachments |
| `src/ui/widgets/server_tree.rs` | 580+ | UI | Sidebar tree: guilds → channels → DMs, navigation helpers, auto-scroll |
| `src/ui/widgets/status_bar.rs` | 345 | UI | Connection state, mode, pane count, zoom indicator |

## Architecture Layers

```
┌─────────────────────────────────────────────────┐
│  Presentation (src/ui/)                         │
│  layout.rs → pane_renderer.rs → widgets/*       │
│  login.rs (self-contained login screen)         │
├─────────────────────────────────────────────────┤
│  Application (src/app.rs, auth.rs, config.rs,   │
│    event_handler.rs, logging.rs, input/, markdown/)│
│  apply_action() = single state mutation point   │
├─────────────────────────────────────────────────┤
│  Domain (src/domain/)                           │
│  types.rs → cache.rs, event.rs, pane.rs         │
│  Pure data + business logic, no I/O             │
├─────────────────────────────────────────────────┤
│  Infrastructure (src/infrastructure/)           │
│  gateway.rs, http_client.rs, db.rs, keyring.rs  │
│  All external I/O behind channel boundaries     │
└─────────────────────────────────────────────────┘
```

**Dependency rule**: layers only depend downward. Domain has zero external I/O imports.

## Key Types & Where They Live

### State & Actions (`domain/types.rs`)
- `Action` — 37-variant enum, the command pattern for all state mutations (includes 7 sidebar nav variants)
- `ConnectionState` — `Disconnected | Connecting | Connected { session_id, resume_url, sequence } | Resuming`
- `CachedMessage` — message with lazy `rendered: Option<Vec<Line>>` for markdown cache
- `HttpRequest` / `DbRequest` / `BackgroundResult` — channel message types for async actors
- `PaneId(u32)`, `ScrollState`, `InputState`, `SplitDirection`, `Direction`

### Application State (`app.rs`)
- `AppState` — owns: `DiscordCache`, `ConnectionState`, `PaneManager` (single source of truth for all pane state), `SidebarState`, `sidebar_focused: bool`, `InputMode`, `AppConfig`, `Theme`
- `apply_action(Action, &mut AppState) -> bool` — **the only function that mutates AppState**
- `App` — wraps `AppState` with `dirty` flag and terminal setup/teardown

### Cache (`domain/cache.rs`)
- `DiscordCache` — `HashMap`-based store for guilds, channels, users, messages (VecDeque per channel, max 200)
- Name resolution: `resolve_user_name()`, `resolve_channel_name()`, `resolve_role()`

### Pane System (`domain/pane.rs`)
- `Pane` — leaf node with `channel_id`, `guild_id`, `scroll`, `input`, `selected_message`, `confirming_delete`
- `PaneNode` — `Leaf(Pane) | Split { direction, ratio, first, second }` binary tree
- `PaneManager` — tree owner with `split()`, `close_focused()`, `focus_next()`, `toggle_zoom()`, `resize_focused()`, `compute_positions()`, `assign_channel()`, session serialization

### Gateway (`infrastructure/gateway.rs`)
- `GatewayConnection` — single WebSocket session, `tokio::select!` with inline heartbeat
- `ZlibDecompressor` — persistent `flate2::Decompress` context across frames
- `GatewayManager` — reconnection loop with exponential backoff (1s..30s)

### Event Parsing (`domain/event.rs`)
- `GatewayEvent` — typed events parsed from raw JSON gateway payloads
- `parse_gateway_payload()` — dispatches on `op` field, then `t` for dispatch events

### HTTP (`infrastructure/http_client.rs`)
- `HttpActor` — async actor receiving `HttpRequest` via channel, per-route rate limiting

### Input (`input/handler.rs`)
- `handle_key_event(KeyEvent, InputMode) -> (Option<Action>, InputMode)` — pure function, no side effects

### Markdown (`markdown/`)
- `parser.rs`: `parse()` → `MarkdownAst` from Discord-flavored markdown
- `renderer.rs`: `render()` → `Vec<Line>` with `MentionResolver` trait for name lookup
- `integration.rs`: `render_message_content()` with lazy caching on `CachedMessage.rendered`

## Data Flow

```
Terminal Input → handle_key_event() → Action → apply_action() → AppState mutation
                                                    ↓
Gateway WS ──→ parse_gateway_payload() ──→ GatewayEvent ──→ Action ──→ apply_action()
                                                    ↓
HTTP Actor ←── HttpRequest channel ←── apply_action() sends requests
HTTP Actor ──→ BackgroundResult ──→ main loop handles directly
                                                    ↓
DB Actor ←── DbRequest channel ←── apply_action() sends requests
DB Actor ──→ BackgroundResult ──→ main loop handles directly
                                                    ↓
                                    AppState.dirty = true → render()
```

## Module Dependencies

```
main.rs
├── event_handler.rs (handle_gateway_event, handle_background_result)
│   ├── domain/types.rs (CachedMessage, ConnectionState, DbRequest, BackgroundResult)
│   ├── domain/cache.rs (DiscordCache)
│   └── domain/event.rs (GatewayEvent)
├── app.rs (AppState, apply_action)
│   ├── domain/types.rs (Action, all data types)
│   ├── domain/cache.rs (DiscordCache)
│   ├── domain/pane.rs (PaneManager)
│   ├── input/handler.rs (handle_key_event)
│   ├── input/mode.rs (InputMode)
│   ├── config.rs (AppConfig)
│   └── ui/theme.rs (Theme)
├── auth.rs
│   ├── config.rs (AuthConfig, DiscordConfig)
│   ├── infrastructure/anti_detection.rs
│   └── infrastructure/keyring.rs (TokenStore)
├── infrastructure/gateway.rs
│   ├── domain/event.rs (GatewayEvent, parse_gateway_payload)
│   ├── domain/types.rs (ConnectionState)
│   ├── infrastructure/anti_detection.rs (build_identify_properties)
│   └── config.rs (DiscordConfig)
├── infrastructure/http_client.rs
│   ├── domain/types.rs (HttpRequest, CachedMessage)
│   ├── infrastructure/anti_detection.rs (build_http_headers)
│   └── config.rs (DiscordConfig)
├── infrastructure/db.rs
│   └── domain/types.rs (CachedMessage, etc.)
├── ui/layout.rs
│   ├── ui/pane_renderer.rs
│   ├── ui/widgets/* (InputBox, MessageView, ServerTree, StatusBar)
│   └── app.rs (AppState)
├── markdown/parser.rs
│   └── domain/markdown.rs (MarkdownAst, MarkdownSpan)
├── markdown/renderer.rs
│   └── domain/markdown.rs
└── markdown/integration.rs
    ├── domain/cache.rs (DiscordCache)
    ├── markdown/parser.rs
    └── markdown/renderer.rs
```
