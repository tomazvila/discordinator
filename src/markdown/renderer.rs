use crate::domain::markdown::{MarkdownAst, MarkdownSpan, MarkdownStyle};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use twilight_model::id::{
    marker::{ChannelMarker, RoleMarker, UserMarker},
    Id,
};

/// Trait for resolving Discord entity names during rendering.
/// Implemented by `DiscordCache` (or a mock in tests).
pub trait MentionResolver {
    fn resolve_user(&self, id: Id<UserMarker>) -> Option<String>;
    fn resolve_channel(&self, id: Id<ChannelMarker>) -> Option<String>;
    fn resolve_role(&self, id: Id<RoleMarker>) -> Option<(String, u32)>; // (name, color)
}

/// Render a `MarkdownAst` into ratatui Lines with styles.
pub fn render(ast: &MarkdownAst, resolver: &dyn MentionResolver) -> Vec<Line<'static>> {
    let mut lines: Vec<Vec<Span<'static>>> = vec![vec![]];

    for md_span in &ast.spans {
        render_span(md_span, Style::default(), resolver, &mut lines);
    }

    lines.into_iter().map(Line::from).collect()
}

/// Recursively render a `MarkdownSpan` into ratatui Spans, handling newlines.
fn render_span(
    md_span: &MarkdownSpan,
    base_style: Style,
    resolver: &dyn MentionResolver,
    lines: &mut Vec<Vec<Span<'static>>>,
) {
    match md_span {
        MarkdownSpan::Text(text) => {
            // Split on newlines to produce multiple lines
            let parts: Vec<&str> = text.split('\n').collect();
            for (idx, part) in parts.iter().enumerate() {
                if !part.is_empty() {
                    lines
                        .last_mut()
                        .unwrap()
                        .push(Span::styled(part.to_string(), base_style));
                }
                if idx < parts.len() - 1 {
                    lines.push(vec![]);
                }
            }
        }

        MarkdownSpan::Styled { content, style } => {
            let ratatui_style = markdown_style_to_ratatui(*style, base_style);
            for inner in content {
                render_span(inner, ratatui_style, resolver, lines);
            }
        }

        MarkdownSpan::InlineCode(code) => {
            let style = base_style.fg(Color::Gray).bg(Color::DarkGray);
            lines
                .last_mut()
                .unwrap()
                .push(Span::styled(code.clone(), style));
        }

        MarkdownSpan::CodeBlock { language, content } => {
            // Code blocks get their own lines
            if let Some(lang) = language {
                lines.push(vec![Span::styled(
                    format!("  [{lang}]"),
                    Style::default().fg(Color::DarkGray),
                )]);
            }
            let code_style = Style::default().fg(Color::Gray);
            for line in content.lines() {
                lines.push(vec![Span::styled(format!("  {line}"), code_style)]);
            }
        }

        MarkdownSpan::UserMention(id) => {
            let display = resolver
                .resolve_user(*id)
                .map_or_else(|| format!("@{}", id.get()), |name| format!("@{name}"));
            let style = base_style.fg(Color::LightBlue).add_modifier(Modifier::BOLD);
            lines.last_mut().unwrap().push(Span::styled(display, style));
        }

        MarkdownSpan::ChannelMention(id) => {
            let display = resolver
                .resolve_channel(*id)
                .map_or_else(|| format!("#{}", id.get()), |name| format!("#{name}"));
            let style = base_style.fg(Color::LightBlue).add_modifier(Modifier::BOLD);
            lines.last_mut().unwrap().push(Span::styled(display, style));
        }

        MarkdownSpan::RoleMention(id) => {
            let (display, style) = if let Some((name, color)) = resolver.resolve_role(*id) {
                let r = ((color >> 16) & 0xFF) as u8;
                let g = ((color >> 8) & 0xFF) as u8;
                let b = (color & 0xFF) as u8;
                let style = base_style
                    .fg(Color::Rgb(r, g, b))
                    .add_modifier(Modifier::BOLD);
                (format!("@{name}"), style)
            } else {
                let style = base_style.fg(Color::LightBlue).add_modifier(Modifier::BOLD);
                (format!("@{}", id.get()), style)
            };
            lines.last_mut().unwrap().push(Span::styled(display, style));
        }

        MarkdownSpan::CustomEmoji { name, .. } => {
            let style = base_style.fg(Color::Yellow);
            lines
                .last_mut()
                .unwrap()
                .push(Span::styled(format!(":{name}:"), style));
        }
        MarkdownSpan::Spoiler(inner) => {
            let spoiler_style = base_style.fg(Color::DarkGray).bg(Color::DarkGray);
            for child in inner {
                render_span(child, spoiler_style, resolver, lines);
            }
        }
    }
}

/// Convert `MarkdownStyle` flags to a ratatui Style, layered on a base style.
fn markdown_style_to_ratatui(md_style: MarkdownStyle, base: Style) -> Style {
    let mut style = base;
    if md_style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if md_style.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if md_style.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if md_style.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parser;
    use std::collections::HashMap;

    /// Mock resolver for testing.
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

    #[test]
    fn render_plain_text() {
        let ast = parser::parse("hello world");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 1);
        assert_eq!(lines[0].spans[0].content, "hello world");
    }

    #[test]
    fn render_bold_text() {
        let ast = parser::parse("**bold**");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "bold");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn render_italic_text() {
        let ast = parser::parse("*italic*");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "italic");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::ITALIC));
    }

    #[test]
    fn render_underline_text() {
        let ast = parser::parse("__underline__");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "underline");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn render_strikethrough_text() {
        let ast = parser::parse("~~struck~~");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "struck");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn render_inline_code() {
        let ast = parser::parse("`code`");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "code");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Gray));
    }

    #[test]
    fn render_code_block() {
        let ast = parser::parse("```rust\nfn main() {}\n```");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        // First line is empty (before code block), then [rust] header, then code line
        assert!(lines.len() >= 2);
        // Find the language header
        let lang_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("[rust]")));
        assert!(lang_line.is_some());
    }

    #[test]
    fn render_user_mention_resolved() {
        let ast = parser::parse("Hello <@123>");
        let mut resolver = MockResolver::new();
        resolver.users.insert(123, "TestUser".to_string());
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 2);
        assert_eq!(lines[0].spans[0].content, "Hello ");
        assert_eq!(lines[0].spans[1].content, "@TestUser");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::LightBlue));
    }

    #[test]
    fn render_user_mention_unresolved() {
        let ast = parser::parse("<@999>");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines[0].spans[0].content, "@999");
    }

    #[test]
    fn render_channel_mention_resolved() {
        let ast = parser::parse("Check <#456>");
        let mut resolver = MockResolver::new();
        resolver.channels.insert(456, "general".to_string());
        let lines = render(&ast, &resolver);
        assert_eq!(lines[0].spans[1].content, "#general");
    }

    #[test]
    fn render_role_mention_with_color() {
        let ast = parser::parse("<@&789>");
        let mut resolver = MockResolver::new();
        resolver.roles.insert(789, ("Admin".to_string(), 0xFF0000));
        let lines = render(&ast, &resolver);
        assert_eq!(lines[0].spans[0].content, "@Admin");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn render_custom_emoji() {
        let ast = parser::parse("<:smile:123>");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines[0].spans[0].content, ":smile:");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn render_mixed_content() {
        let ast = parser::parse("**bold** and `code`");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 3);
        assert_eq!(lines[0].spans[0].content, "bold");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert_eq!(lines[0].spans[1].content, " and ");
        assert_eq!(lines[0].spans[2].content, "code");
    }

    #[test]
    fn render_text_with_newlines() {
        let ast = parser::parse("line1\nline2\nline3");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].spans[0].content, "line1");
        assert_eq!(lines[1].spans[0].content, "line2");
        assert_eq!(lines[2].spans[0].content, "line3");
    }

    #[test]
    fn render_empty_input() {
        let ast = parser::parse("");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        // Empty AST produces a single empty line
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.is_empty());
    }

    #[test]
    fn render_bold_italic_combined() {
        let ast = parser::parse("***bold italic***");
        let resolver = MockResolver::new();
        let lines = render(&ast, &resolver);
        assert_eq!(lines[0].spans[0].content, "bold italic");
        let style = lines[0].spans[0].style;
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }
}
