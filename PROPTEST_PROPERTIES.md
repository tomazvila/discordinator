# Proptest Property Inventory

Properties identified across the codebase for property-based testing with `proptest`.

---

## 1. PaneNode / PaneManager (`src/domain/pane.rs`)

### Tree structural invariants
- **P1.1** `split` increases `leaf_count` by exactly 1
- **P1.2** `split` preserves all pre-existing pane IDs (no pane is lost)
- **P1.3** `leaves_in_order` returns exactly `leaf_count` entries, all unique
- **P1.4** `find(id)` succeeds for every ID returned by `leaves_in_order`
- **P1.5** `contains(id)` agrees with `find(id).is_some()`
- **P1.6** After `remove`, `leaf_count` decreases by 1 (when count > 1)
- **P1.7** `remove` preserves all other panes that were not removed
- **P1.8** `close_focused` always leaves at least 1 pane

### Focus cycling
- **P1.9** `focus_next` N times (N = pane_count) returns to original focus
- **P1.10** `focus_prev` is the inverse of `focus_next`

### Resize
- **P1.11** After any sequence of `resize` calls, ratio stays in [0.1, 0.9]

### split_area
- **P1.12** `split_area` vertical: `first.width + second.width == area.width`
- **P1.13** `split_area` horizontal: `first.height + second.height == area.height`
- **P1.14** `split_area` preserves origin: `first.x == area.x`, `first.y == area.y`
- **P1.15** `split_area` second follows first: vertical → `second.x == first.x + first.width`, horizontal → `second.y == first.y + first.height`

### compute_positions
- **P1.16** `compute_positions` returns exactly `leaf_count` entries
- **P1.17** All rects from `compute_positions` fit within the given area

### Session persistence
- **P1.18** Serialize → deserialize roundtrip preserves pane_count, all pane IDs, and focused_pane_id

---

## 2. DiscordCache (`src/domain/cache.rs`)

- **P2.1** After any number of `insert_message` calls, messages per channel never exceeds `MAX_CACHED_MESSAGES_PER_CHANNEL`
- **P2.2** `insert_guild` is idempotent for `guild_order` (no duplicates after re-inserting same guild)
- **P2.3** After `insert_guild`, all channels in `channel_order` have correct `channel_guild` reverse lookup
- **P2.4** After `remove_guild`, no `channel_guild` entries reference the removed guild
- **P2.5** After `remove_channel`, messages/typing/read_states for that channel are gone

---

## 3. Markdown Parser (`src/markdown/parser.rs`)

- **P3.1** `parse` never panics on arbitrary `String` input
- **P3.2** Plain ASCII text without any formatting chars produces a single `Text` span with the original text
- **P3.3** Parsing preserves text content: all non-formatting chars appear in extracted text
- **P3.4** `parse("")` produces empty spans vec

---

## 4. InputState cursor operations (`src/ui/widgets/input_box.rs`)

- **P4.1** `cursor_pos` is always a valid UTF-8 char boundary in `content`
- **P4.2** `cursor_pos <= content.len()` after any operation
- **P4.3** `cursor_col == sum of unicode_width for chars before cursor`
- **P4.4** `insert_char(c)` then `delete_char_before_cursor` returns to the original content and cursor position
- **P4.5** `move_cursor_home` sets `cursor_pos = 0, cursor_col = 0`
- **P4.6** `move_cursor_end` sets `cursor_pos = content.len()`
- **P4.7** N `move_cursor_right` from Home followed by N `move_cursor_left` returns to Home (N = char_count)

---

## 5. unicode_width (`src/ui/widgets/input_box.rs`)

- **P5.1** ASCII printable chars (0x20..0x7F) have width 1
- **P5.2** Known zero-width chars (ZWJ, ZWNJ, ZWSP, BOM, variation selectors) have width 0
- **P5.3** CJK Unified Ideographs (U+4E00..U+9FFF) have width 2
- **P5.4** Width is always 0, 1, or 2 — never anything else

---

## 6. parse_color (`src/ui/theme.rs`)

- **P6.1** Valid hex `#RRGGBB` (where R,G,B are hex digits) always returns `Some(Color::Rgb(...))`
- **P6.2** Named color parsing is case-insensitive

---

## 7. Config (`src/config.rs`)

- **P7.1** `AppConfig::default()` serializes to TOML and deserializes back to equivalent values

---

## 8. Input handler (`src/input/handler.rs`)

- **P8.1** Any key in `PanePrefix` mode always returns `InputMode::Normal`
- **P8.2** Non-Esc keys in `Insert` mode stay in `InputMode::Insert`

---

## 9. Gateway event parsing (`src/domain/event.rs`)

- **P9.1** `parse_gateway_payload` never panics on arbitrary `serde_json::Value`
- **P9.2** op 10 always produces `Hello`, op 11 → `HeartbeatAck`, op 7 → `Reconnect`, op 9 → `InvalidSession`

---

## 10. Serialization types (`src/domain/types.rs`)

- **P10.1** `MessageAttachment` JSON roundtrip: serialize then deserialize preserves all fields
- **P10.2** `MessageEmbed` JSON roundtrip
- **P10.3** `MessageReference` JSON roundtrip
- **P10.4** `PaneId` JSON roundtrip
- **P10.5** `SplitDirection` JSON roundtrip
