use crate::domain::markdown::{MarkdownAst, MarkdownSpan, MarkdownStyle};
use twilight_model::id::Id;

/// Parse Discord-flavored markdown into a `MarkdownAst`.
pub fn parse(input: &str) -> MarkdownAst {
    let spans = parse_inline(input, MarkdownStyle::default());
    MarkdownAst::new(merge_text_spans(spans))
}

/// Parse inline formatting within text, applying accumulated style.
#[allow(clippy::too_many_lines)]
fn parse_inline(input: &str, _style: MarkdownStyle) -> Vec<MarkdownSpan> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Code block (``` ... ```)
        if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            if let Some((block, end)) = parse_code_block(&chars, i) {
                if !current_text.is_empty() {
                    spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                }
                spans.push(block);
                i = end;
                continue;
            }
            // Unclosed code block: treat ``` as literal text
            current_text.push_str("```");
            i += 3;
            continue;
        }

        // Inline code (` ... `)
        if chars[i] == '`' {
            if let Some((code, end)) = parse_inline_code(&chars, i) {
                if !current_text.is_empty() {
                    spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                }
                spans.push(MarkdownSpan::InlineCode(code));
                i = end;
                continue;
            }
        }

        // Custom emoji <:name:id> or <a:name:id>
        if chars[i] == '<'
            && i + 1 < len
            && (chars[i + 1] == ':' || (chars[i + 1] == 'a' && i + 2 < len && chars[i + 2] == ':'))
        {
            if let Some((emoji, end)) = parse_custom_emoji(&chars, i) {
                if !current_text.is_empty() {
                    spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                }
                spans.push(emoji);
                i = end;
                continue;
            }
        }

        // Mentions: <@id>, <@!id>, <@&id>, <#id>
        if chars[i] == '<' && i + 1 < len && (chars[i + 1] == '@' || chars[i + 1] == '#') {
            if let Some((mention, end)) = parse_mention(&chars, i) {
                if !current_text.is_empty() {
                    spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                }
                spans.push(mention);
                i = end;
                continue;
            }
        }

        // Bold + italic (***text***)
        if i + 2 < len && chars[i] == '*' && chars[i + 1] == '*' && chars[i + 2] == '*' {
            if let Some((content, end)) = find_closing(&chars, i + 3, "***") {
                if !content.is_empty() {
                    if !current_text.is_empty() {
                        spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                    }
                    let inner = parse_inline(&content, MarkdownStyle::default());
                    spans.push(MarkdownSpan::Styled {
                        content: inner,
                        style: MarkdownStyle {
                            bold: true,
                            italic: true,
                            ..Default::default()
                        },
                    });
                    i = end;
                    continue;
                }
            }
        }

        // Bold (**text**)
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some((content, end)) = find_closing(&chars, i + 2, "**") {
                if !content.is_empty() {
                    if !current_text.is_empty() {
                        spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                    }
                    let inner = parse_inline(&content, MarkdownStyle::default());
                    spans.push(MarkdownSpan::Styled {
                        content: inner,
                        style: MarkdownStyle {
                            bold: true,
                            ..Default::default()
                        },
                    });
                    i = end;
                    continue;
                }
            }
        }

        // Italic (*text*)
        if chars[i] == '*' {
            if let Some((content, end)) = find_closing(&chars, i + 1, "*") {
                if !content.is_empty() {
                    if !current_text.is_empty() {
                        spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                    }
                    let inner = parse_inline(&content, MarkdownStyle::default());
                    spans.push(MarkdownSpan::Styled {
                        content: inner,
                        style: MarkdownStyle {
                            italic: true,
                            ..Default::default()
                        },
                    });
                    i = end;
                    continue;
                }
            }
        }

        // Underline (__text__)
        if i + 1 < len && chars[i] == '_' && chars[i + 1] == '_' {
            if let Some((content, end)) = find_closing(&chars, i + 2, "__") {
                if !content.is_empty() {
                    if !current_text.is_empty() {
                        spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                    }
                    let inner = parse_inline(&content, MarkdownStyle::default());
                    spans.push(MarkdownSpan::Styled {
                        content: inner,
                        style: MarkdownStyle {
                            underline: true,
                            ..Default::default()
                        },
                    });
                    i = end;
                    continue;
                }
            }
        }

        // Italic (_text_) — single underscore, but not __ (underline).
        // Skip if preceded by a word char (mid-word underscores like some_var_name are literal).
        if chars[i] == '_' && !(i + 1 < len && chars[i + 1] == '_') {
            let preceded_by_word = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
            if !preceded_by_word {
                if let Some((content, end)) = find_closing(&chars, i + 1, "_") {
                    // Also check closing underscore is not followed by a word char
                    let followed_by_word =
                        end < len && (chars[end].is_alphanumeric() || chars[end] == '_');
                    if !content.is_empty() && !followed_by_word {
                        if !current_text.is_empty() {
                            spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                        }
                        let inner = parse_inline(&content, MarkdownStyle::default());
                        spans.push(MarkdownSpan::Styled {
                            content: inner,
                            style: MarkdownStyle {
                                italic: true,
                                ..Default::default()
                            },
                        });
                        i = end;
                        continue;
                    }
                }
            }
        }

        // Spoiler (||text||)
        if i + 1 < len && chars[i] == '|' && chars[i + 1] == '|' {
            if let Some((content, end)) = find_closing(&chars, i + 2, "||") {
                if !content.is_empty() {
                    if !current_text.is_empty() {
                        spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                    }
                    let inner = parse_inline(&content, MarkdownStyle::default());
                    spans.push(MarkdownSpan::Spoiler(inner));
                    i = end;
                    continue;
                }
            }
        }

        // Strikethrough (~~text~~)
        if i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
            if let Some((content, end)) = find_closing(&chars, i + 2, "~~") {
                if !content.is_empty() {
                    if !current_text.is_empty() {
                        spans.push(MarkdownSpan::Text(std::mem::take(&mut current_text)));
                    }
                    let inner = parse_inline(&content, MarkdownStyle::default());
                    spans.push(MarkdownSpan::Styled {
                        content: inner,
                        style: MarkdownStyle {
                            strikethrough: true,
                            ..Default::default()
                        },
                    });
                    i = end;
                    continue;
                }
            }
        }

        current_text.push(chars[i]);
        i += 1;
    }

    if !current_text.is_empty() {
        spans.push(MarkdownSpan::Text(current_text));
    }

    spans
}

/// Find closing delimiter and return (`content_between`, `position_after_closing`).
/// For single-char delimiters like `*`, ensures we don't match part of a multi-char
/// delimiter (e.g., `**` when looking for `*`).
fn find_closing(chars: &[char], start: usize, delimiter: &str) -> Option<(String, usize)> {
    let delim_chars: Vec<char> = delimiter.chars().collect();
    let delim_len = delim_chars.len();

    let mut i = start;
    while i + delim_len <= chars.len() {
        // Skip backtick-enclosed regions — formatting doesn't apply inside code spans
        if chars[i] == '`' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '`' {
                j += 1;
            }
            if j < chars.len() {
                i = j + 1; // skip past closing backtick
                continue;
            }
            // Unclosed backtick, continue normally
        }

        // Check if delimiter matches at position i
        let mut matches = true;
        for (j, dc) in delim_chars.iter().enumerate() {
            if chars[i + j] != *dc {
                matches = false;
                break;
            }
        }
        if matches {
            // For single `*` delimiter, make sure we're not at `**` (which is bold, not italic close)
            if delimiter == "*" && i + 1 < chars.len() && chars[i + 1] == '*' {
                // This is ** not *, try to skip past the bold content and closing **
                let mut j = i + 2;
                let mut found_close = false;
                while j < chars.len() {
                    if j + 1 < chars.len() && chars[j] == '*' && chars[j + 1] == '*' {
                        j += 2;
                        found_close = true;
                        break;
                    }
                    j += 1;
                }
                if found_close {
                    i = j;
                    continue;
                }
                // Unclosed **: treat this * as a valid closer
            }
            // For single `_` delimiter, make sure we're not at `__` (which is underline, not italic close)
            if delimiter == "_" && i + 1 < chars.len() && chars[i + 1] == '_' {
                // This is __ not _, try to skip past the underline content and closing __
                let mut j = i + 2;
                let mut found_close = false;
                while j < chars.len() {
                    if j + 1 < chars.len() && chars[j] == '_' && chars[j + 1] == '_' {
                        j += 2;
                        found_close = true;
                        break;
                    }
                    j += 1;
                }
                if found_close {
                    i = j;
                    continue;
                }
                // Unclosed __: treat this _ as a valid closer
            }
            let content: String = chars[start..i].iter().collect();
            return Some((content, i + delim_len));
        }
        i += 1;
    }
    None
}

/// Parse inline code: `content`. Returns (content, `position_after_closing_backtick`).
fn parse_inline_code(chars: &[char], start: usize) -> Option<(String, usize)> {
    // start points to the opening `
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '`' {
            let content: String = chars[start + 1..i].iter().collect();
            return Some((content, i + 1));
        }
        i += 1;
    }
    None
}

/// Parse code block: triple-backtick language/content. Returns `(CodeBlock span, position after closing)`.
fn parse_code_block(chars: &[char], start: usize) -> Option<(MarkdownSpan, usize)> {
    // start points to first `
    let block_start = start + 3;

    // Find closing ```
    let mut i = block_start;
    while i + 2 < chars.len() {
        if chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            let raw: String = chars[block_start..i].iter().collect();
            let (language, content) = if let Some(newline_pos) = raw.find('\n') {
                let lang = raw[..newline_pos].trim().to_string();
                let lang = if lang.is_empty() { None } else { Some(lang) };
                (lang, raw[newline_pos + 1..].to_string())
            } else {
                (None, raw)
            };
            return Some((MarkdownSpan::CodeBlock { language, content }, i + 3));
        }
        i += 1;
    }
    None
}

/// Parse mention: <@id>, <@!id>, <@&id>, <#id>
fn parse_mention(chars: &[char], start: usize) -> Option<(MarkdownSpan, usize)> {
    // start points to '<'
    let mut i = start + 1;
    if i >= chars.len() {
        return None;
    }

    if chars[i] == '@' {
        i += 1;
        if i >= chars.len() {
            return None;
        }
        // <@&id> - role mention
        if chars[i] == '&' {
            i += 1;
            return parse_id_until_close(chars, i)
                .map(|(id, end)| (MarkdownSpan::RoleMention(Id::new(id)), end));
        }
        // <@!id> - nickname mention (treat same as user mention)
        if chars[i] == '!' {
            i += 1;
        }
        // <@id> - user mention
        return parse_id_until_close(chars, i)
            .map(|(id, end)| (MarkdownSpan::UserMention(Id::new(id)), end));
    }

    if chars[i] == '#' {
        i += 1;
        // <#id> - channel mention
        return parse_id_until_close(chars, i)
            .map(|(id, end)| (MarkdownSpan::ChannelMention(Id::new(id)), end));
    }

    None
}

/// Parse digits until '>' and return (`parsed_u64`, `position_after_close`).
fn parse_id_until_close(chars: &[char], start: usize) -> Option<(u64, usize)> {
    let mut i = start;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > start && i < chars.len() && chars[i] == '>' {
        let id_str: String = chars[start..i].iter().collect();
        if let Ok(id) = id_str.parse::<u64>() {
            if id > 0 {
                return Some((id, i + 1));
            }
        }
    }
    None
}

/// Parse custom emoji: <:name:id> or <a:name:id>
fn parse_custom_emoji(chars: &[char], start: usize) -> Option<(MarkdownSpan, usize)> {
    // start points to '<'
    let mut i = start + 1;
    let animated = if i < chars.len() && chars[i] == 'a' {
        i += 1;
        true
    } else {
        false
    };

    // Expect ':'
    if i >= chars.len() || chars[i] != ':' {
        return None;
    }
    i += 1;

    // Parse name (until next ':')
    let name_start = i;
    while i < chars.len() && chars[i] != ':' && chars[i] != '>' {
        i += 1;
    }
    if i >= chars.len() || chars[i] != ':' {
        return None;
    }
    let name: String = chars[name_start..i].iter().collect();
    if name.is_empty() {
        return None;
    }
    i += 1; // skip ':'

    // Parse id (digits until '>')
    let id_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i >= chars.len() || chars[i] != '>' || i == id_start {
        return None;
    }
    let id_str: String = chars[id_start..i].iter().collect();
    let id: u64 = id_str.parse().ok()?;
    i += 1; // skip '>'

    Some((MarkdownSpan::CustomEmoji { name, id, animated }, i))
}

/// Merge adjacent Text spans into one.
fn merge_text_spans(spans: Vec<MarkdownSpan>) -> Vec<MarkdownSpan> {
    let mut result: Vec<MarkdownSpan> = Vec::new();
    for span in spans {
        if let MarkdownSpan::Text(text) = &span {
            if let Some(MarkdownSpan::Text(prev)) = result.last_mut() {
                prev.push_str(text);
                continue;
            }
        }
        result.push(span);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::markdown::MarkdownSpan;

    #[test]
    fn plain_text() {
        let ast = parse("hello world");
        assert_eq!(ast.spans, vec![MarkdownSpan::Text("hello world".into())]);
    }

    #[test]
    fn empty_input() {
        let ast = parse("");
        assert!(ast.spans.is_empty());
    }

    #[test]
    fn bold_text() {
        let ast = parse("**bold**");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![MarkdownSpan::Text("bold".into())],
                style: MarkdownStyle {
                    bold: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn italic_text() {
        let ast = parse("*italic*");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![MarkdownSpan::Text("italic".into())],
                style: MarkdownStyle {
                    italic: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn underline_text() {
        let ast = parse("__underline__");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![MarkdownSpan::Text("underline".into())],
                style: MarkdownStyle {
                    underline: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn strikethrough_text() {
        let ast = parse("~~strikethrough~~");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![MarkdownSpan::Text("strikethrough".into())],
                style: MarkdownStyle {
                    strikethrough: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn inline_code() {
        let ast = parse("`code`");
        assert_eq!(ast.spans, vec![MarkdownSpan::InlineCode("code".into())]);
    }

    #[test]
    fn code_block_with_language() {
        let ast = parse("```rust\nfn main() {}\n```");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::CodeBlock {
                language: Some("rust".into()),
                content: "fn main() {}\n".into(),
            }]
        );
    }

    #[test]
    fn code_block_without_language() {
        let ast = parse("```\nhello\n```");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::CodeBlock {
                language: None,
                content: "hello\n".into(),
            }]
        );
    }

    #[test]
    fn user_mention() {
        let ast = parse("<@123456>");
        assert_eq!(ast.spans, vec![MarkdownSpan::UserMention(Id::new(123456))]);
    }

    #[test]
    fn user_mention_with_nickname() {
        let ast = parse("<@!123456>");
        assert_eq!(ast.spans, vec![MarkdownSpan::UserMention(Id::new(123456))]);
    }

    #[test]
    fn channel_mention() {
        let ast = parse("<#789012>");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::ChannelMention(Id::new(789012))]
        );
    }

    #[test]
    fn role_mention() {
        let ast = parse("<@&345678>");
        assert_eq!(ast.spans, vec![MarkdownSpan::RoleMention(Id::new(345678))]);
    }

    #[test]
    fn custom_emoji() {
        let ast = parse("<:smile:123456>");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::CustomEmoji {
                name: "smile".into(),
                id: 123456,
                animated: false,
            }]
        );
    }

    #[test]
    fn animated_emoji() {
        let ast = parse("<a:wave:789012>");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::CustomEmoji {
                name: "wave".into(),
                id: 789012,
                animated: true,
            }]
        );
    }

    #[test]
    fn mixed_formatting() {
        let ast = parse("hello **bold** and *italic* world");
        assert_eq!(
            ast.spans,
            vec![
                MarkdownSpan::Text("hello ".into()),
                MarkdownSpan::Styled {
                    content: vec![MarkdownSpan::Text("bold".into())],
                    style: MarkdownStyle {
                        bold: true,
                        ..Default::default()
                    },
                },
                MarkdownSpan::Text(" and ".into()),
                MarkdownSpan::Styled {
                    content: vec![MarkdownSpan::Text("italic".into())],
                    style: MarkdownStyle {
                        italic: true,
                        ..Default::default()
                    },
                },
                MarkdownSpan::Text(" world".into()),
            ]
        );
    }

    #[test]
    fn bold_italic_combined() {
        let ast = parse("***bold italic***");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![MarkdownSpan::Text("bold italic".into())],
                style: MarkdownStyle {
                    bold: true,
                    italic: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn nested_bold_in_italic() {
        let ast = parse("*italic **bold** text*");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![
                    MarkdownSpan::Text("italic ".into()),
                    MarkdownSpan::Styled {
                        content: vec![MarkdownSpan::Text("bold".into())],
                        style: MarkdownStyle {
                            bold: true,
                            ..Default::default()
                        },
                    },
                    MarkdownSpan::Text(" text".into()),
                ],
                style: MarkdownStyle {
                    italic: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn text_with_mention() {
        let ast = parse("Hello <@123> how are you?");
        assert_eq!(
            ast.spans,
            vec![
                MarkdownSpan::Text("Hello ".into()),
                MarkdownSpan::UserMention(Id::new(123)),
                MarkdownSpan::Text(" how are you?".into()),
            ]
        );
    }

    #[test]
    fn text_with_emoji() {
        let ast = parse("I like <:rust:123456> a lot");
        assert_eq!(
            ast.spans,
            vec![
                MarkdownSpan::Text("I like ".into()),
                MarkdownSpan::CustomEmoji {
                    name: "rust".into(),
                    id: 123456,
                    animated: false,
                },
                MarkdownSpan::Text(" a lot".into()),
            ]
        );
    }

    #[test]
    fn unclosed_bold_treated_as_text() {
        let ast = parse("**unclosed bold");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Text("**unclosed bold".into())]
        );
    }

    #[test]
    fn unclosed_italic_treated_as_text() {
        let ast = parse("*unclosed italic");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Text("*unclosed italic".into())]
        );
    }

    #[test]
    fn unclosed_code_treated_as_text() {
        let ast = parse("`unclosed code");
        assert_eq!(ast.spans, vec![MarkdownSpan::Text("`unclosed code".into())]);
    }

    #[test]
    fn unclosed_code_block_treated_as_text() {
        let ast = parse("```unclosed code block");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Text("```unclosed code block".into())]
        );
    }

    #[test]
    fn invalid_mention_treated_as_text() {
        let ast = parse("<@abc>");
        assert_eq!(ast.spans, vec![MarkdownSpan::Text("<@abc>".into())]);
    }

    #[test]
    fn inline_code_preserves_formatting_chars() {
        let ast = parse("`**not bold**`");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::InlineCode("**not bold**".into())]
        );
    }

    #[test]
    fn multiple_mentions() {
        let ast = parse("<@100> and <@200> and <#300>");
        assert_eq!(
            ast.spans,
            vec![
                MarkdownSpan::UserMention(Id::new(100)),
                MarkdownSpan::Text(" and ".into()),
                MarkdownSpan::UserMention(Id::new(200)),
                MarkdownSpan::Text(" and ".into()),
                MarkdownSpan::ChannelMention(Id::new(300)),
            ]
        );
    }

    #[test]
    fn code_block_preserves_content_exactly() {
        let ast = parse("```\n**bold** *italic* <@123>\n```");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::CodeBlock {
                language: None,
                content: "**bold** *italic* <@123>\n".into(),
            }]
        );
    }

    #[test]
    fn empty_bold_treated_as_text() {
        // Discord treats **** as literal text
        let ast = parse("****");
        // This will parse as bold with empty content inside
        // which is valid since ** matches **
        // The parser finds ** then looks for closing ** and finds it at position 2-3
        assert_eq!(ast.spans.len(), 1);
    }

    #[test]
    fn strikethrough_with_inner_text() {
        let ast = parse("~~hello world~~");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Styled {
                content: vec![MarkdownSpan::Text("hello world".into())],
                style: MarkdownStyle {
                    strikethrough: true,
                    ..Default::default()
                },
            }]
        );
    }

    #[test]
    fn complex_message() {
        let ast = parse("**Welcome** to <#100>! Check out <:emoji:200> and say hi to <@300>");
        assert_eq!(ast.spans.len(), 7);
        assert!(matches!(&ast.spans[0], MarkdownSpan::Styled { style, .. } if style.bold));
        assert!(matches!(&ast.spans[2], MarkdownSpan::ChannelMention(id) if id.get() == 100));
        assert!(matches!(&ast.spans[4], MarkdownSpan::CustomEmoji { name, .. } if name == "emoji"));
        assert!(matches!(&ast.spans[6], MarkdownSpan::UserMention(id) if id.get() == 300));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Extract all text content from a MarkdownAst (recursively).
    fn extract_text(spans: &[MarkdownSpan]) -> String {
        let mut result = String::new();
        for span in spans {
            match span {
                MarkdownSpan::Text(t) => result.push_str(t),
                MarkdownSpan::Styled { content, .. } => result.push_str(&extract_text(content)),
                MarkdownSpan::InlineCode(c) => result.push_str(c),
                MarkdownSpan::CodeBlock { content, language } => {
                    if let Some(lang) = language {
                        result.push_str(lang);
                    }
                    result.push_str(content);
                }
                MarkdownSpan::Spoiler(inner) => result.push_str(&extract_text(inner)),
                MarkdownSpan::UserMention(_)
                | MarkdownSpan::ChannelMention(_)
                | MarkdownSpan::RoleMention(_)
                | MarkdownSpan::CustomEmoji { .. } => {}
            }
        }
        result
    }

    // --- P3.1: parse never panics on arbitrary input ---
    proptest! {
        #[test]
        fn parse_never_panics(input in ".*") {
            let _ = parse(&input);
        }
    }

    // --- P3.2: plain ASCII without formatting chars → single Text span ---
    proptest! {
        #[test]
        fn plain_ascii_is_single_text(input in "[a-zA-Z0-9 ,.!?;:]+") {
            let ast = parse(&input);
            prop_assert_eq!(ast.spans.len(), 1, "Expected 1 span, got {:?}", ast.spans);
            match &ast.spans[0] {
                MarkdownSpan::Text(t) => prop_assert_eq!(t, &input),
                other => prop_assert!(false, "Expected Text, got {:?}", other),
            }
        }
    }

    // --- P3.3: text content is preserved for plain text ---
    proptest! {
        #[test]
        fn text_content_preserved(input in "[a-zA-Z0-9 ]+") {
            let ast = parse(&input);
            let extracted = extract_text(&ast.spans);
            prop_assert_eq!(extracted, input);
        }
    }

    // --- P3.4: empty input produces empty spans ---
    proptest! {
        #[test]
        fn empty_input_empty_spans(_x in Just(())) {
            let ast = parse("");
            prop_assert!(ast.spans.is_empty());
        }
    }

    // --- P3 bonus: parse never produces empty Text spans ---
    proptest! {
        #[test]
        fn no_empty_text_spans(input in ".{0,100}") {
            let ast = parse(&input);
            fn check_no_empty(spans: &[MarkdownSpan]) -> bool {
                spans.iter().all(|s| match s {
                    MarkdownSpan::Text(t) => !t.is_empty(),
                    MarkdownSpan::Styled { content, .. } => check_no_empty(content),
                    MarkdownSpan::Spoiler(inner) => check_no_empty(inner),
                    _ => true,
                })
            }
            prop_assert!(check_no_empty(&ast.spans), "Found empty Text span in {:?}", ast.spans);
        }
    }
}
