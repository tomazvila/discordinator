use serde::{Deserialize, Serialize};
use twilight_model::id::{
    marker::{ChannelMarker, RoleMarker, UserMarker},
    Id,
};

/// Parsed Discord-flavored markdown. A Vec of typed spans.
/// Separating this from raw String prevents rendering unparsed content.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownAst {
    pub spans: Vec<MarkdownSpan>,
}

/// A single span of parsed markdown with style and content info.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkdownSpan {
    /// Plain text with no formatting.
    Text(String),

    /// Styled text (bold, italic, underline, strikethrough — can combine).
    Styled {
        content: Vec<MarkdownSpan>,
        style: MarkdownStyle,
    },

    /// Inline code (`code`).
    InlineCode(String),

    /// Code block (```language\ncode```).
    CodeBlock {
        language: Option<String>,
        content: String,
    },

    /// User mention (<@id> or <@!id>).
    UserMention(Id<UserMarker>),

    /// Channel mention (<#id>).
    ChannelMention(Id<ChannelMarker>),

    /// Role mention (<@&id>).
    RoleMention(Id<RoleMarker>),

    /// Custom emoji (<:name:id> or <a:name:id>).
    CustomEmoji {
        name: String,
        id: u64,
        animated: bool,
    },

    /// Spoiler (||text||).
    Spoiler(Vec<MarkdownSpan>),
}

/// Style flags for formatted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MarkdownStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl MarkdownAst {
    pub fn new(spans: Vec<MarkdownSpan>) -> Self {
        Self { spans }
    }
}
