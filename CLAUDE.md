# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Project Overview

Discordinator is a Discord TUI client written in Rust with tmux-like split pane support. It uses the Discord User API (selfbot approach) with anti-detection measures.

## Feature Implementation

**Before implementing any new feature, read `REQUIREMENTS.md` for specifications:**

- Ralph Tasks (numbered, atomic) define the implementation order
- Anti-Detection Strategy section is **mandatory** for all Discord API interactions
- DM Safety Policy: **NEVER** call `POST /users/@me/channels`
- Phase ordering: P1 (Core MVP) -> P2 (Panes) -> P3 (Enhanced) -> P4 (Power User)

When implementing a feature:
1. Find the task in `REQUIREMENTS.md` and understand its full specification
2. Write failing tests first (TDD)
3. Implement minimum code to pass tests
4. Run `cargo test` and `cargo clippy -- -D warnings`
5. Mark the checkbox in `REQUIREMENTS.md` when complete

## Test-Driven Development (Required)

**All changes must follow TDD:**
1. Write a test that fails (verifies the bug exists or the feature is missing)
2. Run the test to confirm it fails
3. Implement the minimum code to make the test pass
4. Run the test to confirm it passes
5. Refactor if needed (keeping tests green)

Do not implement code without a corresponding failing test first.

## Development Commands

Everything MUST be done in `nix develop` environment enabled by nix flake. Not a single dependency must be installed globally.

```bash
nix develop                          # Enter dev environment
cargo build                          # Build
cargo test                           # Run tests
cargo test -- --nocapture            # Run tests verbose
cargo clippy -- -D warnings          # Lint
cargo fmt --check                    # Format check
RUST_LOG=debug cargo run             # Run with debug logging
```

## Architecture

- **Clean Architecture**: domain (types, cache, pane tree) → application (app, action dispatcher) → infrastructure (gateway, HTTP, SQLite) → presentation (UI, input)
- **Event loop**: `tokio::select!` hub with biased polling (gateway priority), dirty flag rendering
- **No Arc<Mutex>**: Main loop owns all state exclusively. Background tasks communicate via mpsc channels.
- **Pane system**: Binary tree (PaneNode::Leaf | PaneNode::Split), unlimited panes, PaneId(u32) newtype
- **Storage**: SQLite at `~/.local/share/discordinator/messages.db` via `spawn_blocking` (never block event loop)
- **Config**: TOML at `~/.config/discordinator/config.toml` (XDG compliant)
- **Sidebar**: Toggleable fixed element (not part of pane tree), toggle with `Ctrl+b s`

## Key Libraries

| Crate | Purpose |
|-------|---------|
| ratatui 0.30.0 | TUI framework |
| tokio-tungstenite | Custom Discord gateway (NOT twilight-gateway — it's bot-only) |
| reqwest | Custom Discord HTTP client (NOT twilight-http — it adds `Bot ` prefix) |
| twilight-model 0.17.x | Discord data types only (Id<T>, Message, Channel, Guild, etc.) |
| flate2 | zlib-stream decompression for gateway |
| rusqlite | SQLite database (via spawn_blocking) |
| keyring 3.6.x | Secure token storage |
| color-eyre | Error handling + panic recovery |
| tracing | Structured logging to file |

## Anti-Detection (Critical)

All Discord API interactions MUST:
1. Use IDENTIFY properties mimicking the web client (configurable in config.toml) — custom gateway sends these directly
2. Set `X-Super-Properties`, `User-Agent`, `X-Discord-Locale` headers on ALL HTTP requests — custom reqwest client adds these
3. Never call `POST /users/@me/channels` (use DM channels from READY event) — the method does not exist on HttpClient
4. Respect rate limits (custom per-route rate limiter in HTTP actor, 50-150ms jitter)
5. Use zlib-stream transport compression on gateway (flate2, persistent decompressor state)

## Testing

- Unit tests: markdown parser, pane tree, cache, config, SQLite, keybindings, gateway connection, HTTP headers
- Integration tests: mock Discord gateway + HTTP server in `tests/mock_discord/`
- All tests run with: `cargo test`
- 40 atomic Ralph tasks (Tasks 1-40) in REQUIREMENTS.md

## Environment

Uses Nix flakes for reproducible dev environment. The `flake.nix` is at project root.
Rust toolchain provided via `rust-overlay` (stable latest).
