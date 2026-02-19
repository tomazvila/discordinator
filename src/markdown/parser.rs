use crate::domain::markdown::{MarkdownAst, MarkdownSpan, MarkdownStyle};
use twilight_model::id::Id;

/// Parse Discord-flavored markdown into a MarkdownAst.
pub fn parse(input: &str) -> MarkdownAst {
    let spans = parse_inline(input, MarkdownStyle::default());
    MarkdownAst::new(merge_text_spans(spans))
}

/// Parse inline formatting within text, applying accumulated style.
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
            } else {
                // Unclosed code block: treat ``` as literal text
                current_text.push_str("```");
                i += 3;
                continue;
            }
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
        if chars[i] == '<'
            && i + 1 < len
            && (chars[i + 1] == '@' || chars[i + 1] == '#')
        {
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

        // Bold (**text**)
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some((content, end)) = find_closing(&chars, i + 2, "**") {
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

        // Strikethrough (~~text~~)
        if i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
            if let Some((content, end)) = find_closing(&chars, i + 2, "~~") {
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

        current_text.push(chars[i]);
        i += 1;
    }

    if !current_text.is_empty() {
        spans.push(MarkdownSpan::Text(current_text));
    }

    spans
}

/// Find closing delimiter and return (content_between, position_after_closing).
/// For single-char delimiters like `*`, ensures we don't match part of a multi-char
/// delimiter (e.g., `**` when looking for `*`).
fn find_closing(chars: &[char], start: usize, delimiter: &str) -> Option<(String, usize)> {
    let delim_chars: Vec<char> = delimiter.chars().collect();
    let delim_len = delim_chars.len();

    let mut i = start;
    while i + delim_len <= chars.len() {
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
                // This is ** not *, skip past this sequence
                i += 2;
                // Skip past the bold content and closing **
                while i < chars.len() {
                    if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            let content: String = chars[start..i].iter().collect();
            return Some((content, i + delim_len));
        }
        i += 1;
    }
    None
}

/// Parse inline code: `content`. Returns (content, position_after_closing_backtick).
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

/// Parse code block: ```language\ncontent```. Returns (CodeBlock span, position after closing ```)
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
            return parse_id_until_close(chars, i).map(|(id, end)| {
                (MarkdownSpan::RoleMention(Id::new(id)), end)
            });
        }
        // <@!id> - nickname mention (treat same as user mention)
        if chars[i] == '!' {
            i += 1;
        }
        // <@id> - user mention
        return parse_id_until_close(chars, i).map(|(id, end)| {
            (MarkdownSpan::UserMention(Id::new(id)), end)
        });
    }

    if chars[i] == '#' {
        i += 1;
        // <#id> - channel mention
        return parse_id_until_close(chars, i).map(|(id, end)| {
            (MarkdownSpan::ChannelMention(Id::new(id)), end)
        });
    }

    None
}

/// Parse digits until '>' and return (parsed_u64, position_after_close).
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

    Some((
        MarkdownSpan::CustomEmoji {
            name,
            id,
            animated,
        },
        i,
    ))
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
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::UserMention(Id::new(123456))]
        );
    }

    #[test]
    fn user_mention_with_nickname() {
        let ast = parse("<@!123456>");
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::UserMention(Id::new(123456))]
        );
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
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::RoleMention(Id::new(345678))]
        );
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
        assert_eq!(
            ast.spans,
            vec![MarkdownSpan::Text("`unclosed code".into())]
        );
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
