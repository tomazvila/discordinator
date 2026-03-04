use crate::domain::types::CachedMessage;
use crate::markdown::parser;
use crate::markdown::renderer::{self, MentionResolver};
use ratatui::text::Line;

/// Render a message's content to styled lines, using the cache.
/// On first call: parses markdown, renders to Lines, caches result.
/// On subsequent calls: returns cached result directly.
/// Call `invalidate_rendered` when a message is updated.
pub fn render_message_content<'a>(
    message: &'a mut CachedMessage,
    resolver: &dyn MentionResolver,
) -> &'a [Line<'static>] {
    if message.rendered.is_none() {
        let ast = parser::parse(&message.content);
        message.rendered = Some(renderer::render(&ast, resolver));
    }
    message.rendered.as_deref().unwrap()
}

/// Invalidate cached rendered output (call on `MESSAGE_UPDATE`).
pub fn invalidate_rendered(message: &mut CachedMessage) {
    message.rendered = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::CachedMessage;
    use crate::markdown::renderer::MentionResolver;
    use std::collections::HashMap;
    use twilight_model::id::{
        marker::{ChannelMarker, RoleMarker, UserMarker},
        Id,
    };

    struct MockResolver {
        users: HashMap<u64, String>,
        channels: HashMap<u64, String>,
        roles: HashMap<u64, (String, u32)>,
    }

    impl MockResolver {
        fn new() -> Self {
            Self {
                users: HashMap::new(),
                channels: HashMap::new(),
                roles: HashMap::new(),
            }
        }
    }

    impl MentionResolver for MockResolver {
        fn resolve_user(&self, id: Id<UserMarker>) -> Option<String> {
            self.users.get(&id.get()).cloned()
        }
        fn resolve_channel(&self, id: Id<ChannelMarker>) -> Option<String> {
            self.channels.get(&id.get()).cloned()
        }
        fn resolve_role(&self, id: Id<RoleMarker>) -> Option<(String, u32)> {
            self.roles.get(&id.get()).cloned()
        }
    }

    fn make_message(content: &str) -> CachedMessage {
        CachedMessage {
            id: Id::new(1),
            channel_id: Id::new(2),
            author_id: Id::new(3),
            content: content.to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        }
    }

    #[test]
    fn first_render_parses_and_caches() {
        let mut msg = make_message("**hello** world");
        let resolver = MockResolver::new();

        // First call: rendered is None, should parse and cache
        assert!(msg.rendered.is_none());
        {
            let lines = render_message_content(&mut msg, &resolver);
            assert!(!lines.is_empty());
        }
        // Now rendered should be cached
        assert!(msg.rendered.is_some());
    }

    #[test]
    fn subsequent_render_uses_cache() {
        let mut msg = make_message("**hello** world");
        let resolver = MockResolver::new();

        // First render
        let first_len = {
            let lines = render_message_content(&mut msg, &resolver);
            lines.len()
        };

        // Second render should use cache (same result)
        let lines2 = render_message_content(&mut msg, &resolver);
        assert_eq!(lines2.len(), first_len);
    }

    #[test]
    fn invalidate_forces_reparse() {
        let mut msg = make_message("original content");
        let resolver = MockResolver::new();

        // First render
        render_message_content(&mut msg, &resolver);
        assert!(msg.rendered.is_some());

        // Simulate MESSAGE_UPDATE: change content and invalidate
        msg.content = "**updated** content".to_string();
        invalidate_rendered(&mut msg);
        assert!(msg.rendered.is_none());

        // Re-render should parse the new content
        {
            let lines = render_message_content(&mut msg, &resolver);
            assert!(!lines.is_empty());
        }
        // Verify cached and contains new content
        assert!(msg.rendered.is_some());
        let rendered = msg.rendered.as_ref().unwrap();
        assert!(rendered[0]
            .spans
            .iter()
            .any(|s| s.content.contains("updated")));
    }

    #[test]
    fn render_with_resolved_mentions() {
        let mut msg = make_message("Hello <@100> in <#200>");
        let mut resolver = MockResolver::new();
        resolver.users.insert(100, "Alice".to_string());
        resolver.channels.insert(200, "general".to_string());

        let lines = render_message_content(&mut msg, &resolver);
        let all_text: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(all_text.contains("@Alice"));
        assert!(all_text.contains("#general"));
    }

    #[test]
    fn render_plain_text_message() {
        let mut msg = make_message("just plain text");
        let resolver = MockResolver::new();

        let lines = render_message_content(&mut msg, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "just plain text");
    }

    #[test]
    fn render_empty_message() {
        let mut msg = make_message("");
        let resolver = MockResolver::new();

        let lines = render_message_content(&mut msg, &resolver);
        // Empty content produces a single line with no spans
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn render_message_with_code_block() {
        let mut msg = make_message("```rust\nfn main() {}\n```");
        let resolver = MockResolver::new();

        let lines = render_message_content(&mut msg, &resolver);
        assert!(lines.len() >= 2); // At minimum: language header + code line
    }

    #[test]
    fn cache_invalidation_on_edit_simulation() {
        let mut msg = make_message("v1");
        let resolver = MockResolver::new();

        // Render v1
        render_message_content(&mut msg, &resolver);
        let v1_content: String = msg.rendered.as_ref().unwrap()[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(v1_content.contains("v1"));

        // Simulate edit
        msg.content = "v2".to_string();
        msg.edited_timestamp = Some("2024-01-01T01:00:00Z".to_string());
        invalidate_rendered(&mut msg);

        // Render v2
        render_message_content(&mut msg, &resolver);
        let v2_content: String = msg.rendered.as_ref().unwrap()[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(v2_content.contains("v2"));
        assert!(!v2_content.contains("v1"));
    }
}
