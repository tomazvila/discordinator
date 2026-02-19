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

- **Event loop**: `tokio::select!` hub over terminal events, gateway events, and 60 FPS render tick
- **Pane system**: Binary tree (PaneNode::Leaf | PaneNode::Split), unlimited panes
- **Storage**: SQLite at `~/.local/share/discordinator/messages.db` for message persistence
- **Config**: TOML at `~/.config/discordinator/config.toml` (XDG compliant)
- **Sidebar**: Toggleable fixed element (not part of pane tree), toggle with `Ctrl+b s`

## Key Libraries

| Crate | Purpose |
|-------|---------|
| ratatui 0.30.0 | TUI framework |
| twilight-gateway 0.17.1 | Discord WebSocket gateway |
| twilight-http 0.17.1 | Discord REST API |
| twilight-model 0.17.x | Discord data types |
| rusqlite | SQLite database |
| keyring 3.6.x | Secure token storage |
| color-eyre | Error handling + panic recovery |
| tracing | Structured logging to file |

## Anti-Detection (Critical)

All Discord API interactions MUST:
1. Use IDENTIFY properties mimicking the web client (configurable in config.toml)
2. Set `X-Super-Properties`, `User-Agent`, `X-Discord-Locale` headers on HTTP requests
3. Never call `POST /users/@me/channels` (use DM channels from READY event)
4. Respect rate limits (twilight-http handles this, but add request jitter)
5. Use zstd transport compression on gateway (twilight default)

## Testing

- Unit tests: markdown parser, pane tree, cache, config, SQLite, keybindings
- Integration tests: mock Discord gateway + HTTP server in `tests/mock_discord/`
- All tests run with: `cargo test`

## Environment

Uses Nix flakes for reproducible dev environment. The `flake.nix` is at project root.
Rust toolchain provided via `rust-overlay` (stable latest, currently 1.93.1).
