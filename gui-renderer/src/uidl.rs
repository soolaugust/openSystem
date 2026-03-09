use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// UIDL Widget — the basic building block of openSystem UI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    Text {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        style: Option<TextStyle>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Button {
        label: String,
        action: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        style: Option<ButtonStyle>,
    },
    #[serde(rename = "vstack")]
    VStack {
        #[serde(skip_serializing_if = "Option::is_none")]
        gap: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        padding: Option<u32>,
        children: Vec<Widget>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "hstack")]
    HStack {
        #[serde(skip_serializing_if = "Option::is_none")]
        gap: Option<u32>,
        children: Vec<Widget>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Input {
        #[serde(skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        on_change: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Spacer {
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TextStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<TextAlign>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ButtonStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_radius: Option<u32>,
}

/// Top-level UIDL document
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UidlDocument {
    pub layout: Widget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Theme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size_base: Option<u32>,
}

impl UidlDocument {
    /// Parse UIDL from JSON string
    pub fn parse(json: &str) -> anyhow::Result<Self> {
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("UIDL parse error: {}", e))
    }

    /// Compute SHA256 hash of the UIDL document (for caching)
    pub fn hash(&self) -> String {
        let json = serde_json::to_string(self).expect("UidlDocument is always serializable");
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Count total widgets in the document
    pub fn widget_count(&self) -> usize {
        count_widgets(&self.layout)
    }
}

fn count_widgets(widget: &Widget) -> usize {
    match widget {
        Widget::VStack { children, .. } | Widget::HStack { children, .. } => {
            1 + children.iter().map(count_widgets).sum::<usize>()
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_uidl() {
        let json = r#"{
            "layout": {
                "type": "vstack",
                "gap": 16,
                "children": [
                    {"type": "text", "content": "Hello openSystem"},
                    {"type": "button", "label": "Click me", "action": "test.click"}
                ]
            }
        }"#;
        let doc = UidlDocument::parse(json).unwrap();
        assert_eq!(doc.widget_count(), 3); // vstack + text + button
    }

    #[test]
    fn test_hash_deterministic() {
        let json = r#"{"layout": {"type": "text", "content": "hi"}}"#;
        let doc = UidlDocument::parse(json).unwrap();
        assert_eq!(doc.hash(), doc.hash());
    }

    #[test]
    fn test_hash_changes_with_content() {
        let doc1 = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "a"}}"#).unwrap();
        let doc2 = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "b"}}"#).unwrap();
        assert_ne!(doc1.hash(), doc2.hash());
    }
}
