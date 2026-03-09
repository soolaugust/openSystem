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

/// Text styling options for a [`Widget::Text`].
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

/// Horizontal text alignment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Visual styling for a [`Widget::Button`].
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

/// Global theme applied to a [`UidlDocument`].
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

    /// Compute SHA256 hash of the UIDL document (for caching).
    /// Returns an empty string if serialization fails (should never occur in practice).
    pub fn hash(&self) -> String {
        let json = match serde_json::to_string(self) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
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

    #[test]
    fn test_parse_invalid_json() {
        let result = UidlDocument::parse("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_string() {
        let result = UidlDocument::parse("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_layout() {
        let result = UidlDocument::parse(r#"{"theme": {}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_widget_count_single() {
        let doc = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "hi"}}"#).unwrap();
        assert_eq!(doc.widget_count(), 1);
    }

    #[test]
    fn test_widget_count_nested() {
        let json = r#"{
            "layout": {
                "type": "vstack",
                "children": [
                    {"type": "text", "content": "a"},
                    {"type": "hstack", "children": [
                        {"type": "button", "label": "b", "action": "click"},
                        {"type": "input"}
                    ]},
                    {"type": "spacer"}
                ]
            }
        }"#;
        let doc = UidlDocument::parse(json).unwrap();
        // vstack(1) + text(1) + hstack(1) + button(1) + input(1) + spacer(1) = 6
        assert_eq!(doc.widget_count(), 6);
    }

    #[test]
    fn test_widget_types_serde() {
        let widgets_json = vec![
            r#"{"type": "text", "content": "hello"}"#,
            r#"{"type": "button", "label": "Ok", "action": "ok"}"#,
            r#"{"type": "input", "placeholder": "type here"}"#,
            r#"{"type": "spacer", "size": 16}"#,
            r#"{"type": "vstack", "gap": 8, "children": []}"#,
            r#"{"type": "hstack", "children": []}"#,
        ];
        for json in widgets_json {
            let widget: Widget = serde_json::from_str(json).unwrap();
            let back = serde_json::to_string(&widget).unwrap();
            let reparsed: Widget = serde_json::from_str(&back).unwrap();
            assert_eq!(widget, reparsed);
        }
    }

    #[test]
    fn test_document_with_theme() {
        let json = r##"{
            "layout": {"type": "text", "content": "themed"},
            "theme": {
                "primary_color": "#FF0000",
                "background_color": "#FFFFFF",
                "font_family": "Inter",
                "font_size_base": 14
            }
        }"##;
        let doc = UidlDocument::parse(json).unwrap();
        let theme = doc.theme.unwrap();
        assert_eq!(theme.primary_color.as_deref(), Some("#FF0000"));
        assert_eq!(theme.font_size_base, Some(14));
    }

    #[test]
    fn test_document_with_metadata() {
        let json = r#"{
            "layout": {"type": "text", "content": "meta"},
            "metadata": {"author": "test", "version": "1.0"}
        }"#;
        let doc = UidlDocument::parse(json).unwrap();
        let meta = doc.metadata.unwrap();
        assert_eq!(meta.get("author").unwrap(), "test");
    }

    #[test]
    fn test_text_style() {
        let json = r#"{
            "layout": {
                "type": "text",
                "content": "styled",
                "style": {"font_size": 24, "color": "red", "bold": true, "align": "center"}
            }
        }"#;
        let doc = UidlDocument::parse(json).unwrap();
        if let Widget::Text { style, .. } = &doc.layout {
            let s = style.as_ref().unwrap();
            assert_eq!(s.font_size, Some(24));
            assert_eq!(s.bold, Some(true));
            assert_eq!(s.align, Some(TextAlign::Center));
        } else {
            panic!("expected Text widget");
        }
    }
}
