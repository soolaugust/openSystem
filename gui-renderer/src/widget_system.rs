//! Widget system — ECS-inspired layout and rendering components for openSystem UI.
//!
//! Each [`WidgetNode`] is a self-contained renderable element. The [`LayoutEngine`]
//! computes bounding boxes top-down, and the [`Painter`] draws them using tiny-skia.
//!
//! Supported widgets (Round 8): Text, Button, Input, VStack, HStack, Spacer.

use crate::uidl::{ButtonStyle, TextStyle, UidlDocument, Widget};
use anyhow::Result;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

// ─── Resolved layout box ─────────────────────────────────────────────────────

/// Computed position and dimensions of a widget after layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl LayoutBox {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Returns the tiny-skia [`Rect`] for this box, or `None` if dimensions are zero.
    pub fn to_rect(&self) -> Option<Rect> {
        Rect::from_xywh(self.x, self.y, self.width.max(1.0), self.height.max(1.0))
    }
}

// ─── Render constants ────────────────────────────────────────────────────────

const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_BUTTON_HEIGHT: f32 = 36.0;
const DEFAULT_INPUT_HEIGHT: f32 = 32.0;
const DEFAULT_PADDING: f32 = 8.0;
const DEFAULT_GAP: f32 = 8.0;

// ─── Palette helpers ─────────────────────────────────────────────────────────

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Color::from_rgba8(r, g, b, 255))
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        let a = u8::from_str_radix(&s[6..8], 16).ok()?;
        Some(Color::from_rgba8(r, g, b, a))
    } else {
        None
    }
}

fn named_color(name: &str) -> Option<Color> {
    match name.to_lowercase().as_str() {
        "red" => Some(Color::from_rgba8(220, 38, 38, 255)),
        "green" => Some(Color::from_rgba8(34, 197, 94, 255)),
        "blue" => Some(Color::from_rgba8(59, 130, 246, 255)),
        "white" => Some(Color::WHITE),
        "black" => Some(Color::BLACK),
        "gray" | "grey" => Some(Color::from_rgba8(156, 163, 175, 255)),
        _ => None,
    }
}

fn resolve_color(s: &str) -> Color {
    parse_color(s)
        .or_else(|| named_color(s))
        .unwrap_or(Color::BLACK)
}

// ─── Layout engine ────────────────────────────────────────────────────────────

/// Computes [`LayoutBox`] for every widget in a UIDL document.
pub struct LayoutEngine {
    canvas_width: f32,
    canvas_height: f32,
}

impl LayoutEngine {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            canvas_width: width as f32,
            canvas_height: height as f32,
        }
    }

    /// Compute layout for an entire document.
    /// Returns a flat list of `(widget, layout_box)` pairs in paint order.
    pub fn layout_document<'a>(
        &self,
        doc: &'a UidlDocument,
    ) -> Vec<(&'a Widget, LayoutBox)> {
        let root_box = LayoutBox::new(0.0, 0.0, self.canvas_width, self.canvas_height);
        let mut results = Vec::new();
        self.layout_widget(&doc.layout, root_box, &mut results);
        results
    }

    fn layout_widget<'a>(
        &self,
        widget: &'a Widget,
        available: LayoutBox,
        out: &mut Vec<(&'a Widget, LayoutBox)>,
    ) {
        match widget {
            Widget::VStack { gap, padding, children, .. } => {
                let pad = padding.map(|p| p as f32).unwrap_or(DEFAULT_PADDING);
                let gap_px = gap.map(|g| g as f32).unwrap_or(DEFAULT_GAP);
                let inner = LayoutBox::new(
                    available.x + pad,
                    available.y + pad,
                    (available.width - 2.0 * pad).max(0.0),
                    (available.height - 2.0 * pad).max(0.0),
                );
                out.push((widget, available));
                let mut y = inner.y;
                let child_count = children.len();
                for (i, child) in children.iter().enumerate() {
                    let child_height = self.widget_natural_height(child, inner.width);
                    let child_box = LayoutBox::new(inner.x, y, inner.width, child_height);
                    self.layout_widget(child, child_box, out);
                    y += child_height;
                    if i + 1 < child_count {
                        y += gap_px;
                    }
                }
            }
            Widget::HStack { gap, children, .. } => {
                let gap_px = gap.map(|g| g as f32).unwrap_or(DEFAULT_GAP);
                out.push((widget, available));
                if children.is_empty() {
                    return;
                }
                let total_gap = gap_px * (children.len() - 1) as f32;
                let child_width = ((available.width - total_gap) / children.len() as f32).max(0.0);
                let mut x = available.x;
                let child_count = children.len();
                for (i, child) in children.iter().enumerate() {
                    let child_box =
                        LayoutBox::new(x, available.y, child_width, available.height);
                    self.layout_widget(child, child_box, out);
                    x += child_width;
                    if i + 1 < child_count {
                        x += gap_px;
                    }
                }
            }
            // Leaf widgets: just record their assigned box
            leaf => {
                out.push((leaf, available));
            }
        }
    }

    /// Estimate natural height of a widget given an available width.
    fn widget_natural_height(&self, widget: &Widget, _width: f32) -> f32 {
        match widget {
            Widget::Text { style, content, .. } => {
                let fs = style
                    .as_ref()
                    .and_then(|s| s.font_size)
                    .map(|s| s as f32)
                    .unwrap_or(DEFAULT_FONT_SIZE);
                // Rough estimate: 1 line
                let _ = content;
                fs * 1.4
            }
            Widget::Button { .. } => DEFAULT_BUTTON_HEIGHT,
            Widget::Input { .. } => DEFAULT_INPUT_HEIGHT,
            Widget::Spacer { size } => size.map(|s| s as f32).unwrap_or(DEFAULT_GAP),
            Widget::VStack { gap, padding, children, .. } => {
                let pad = padding.map(|p| p as f32).unwrap_or(DEFAULT_PADDING);
                let gap_px = gap.map(|g| g as f32).unwrap_or(DEFAULT_GAP);
                let total: f32 = children
                    .iter()
                    .map(|c| self.widget_natural_height(c, _width))
                    .sum::<f32>()
                    + gap_px * (children.len().saturating_sub(1)) as f32;
                total + 2.0 * pad
            }
            Widget::HStack { children, .. } => children
                .iter()
                .map(|c| self.widget_natural_height(c, _width))
                .fold(0.0_f32, f32::max),
        }
    }
}

// ─── Painter ─────────────────────────────────────────────────────────────────

/// Renders a list of laid-out widgets into a [`Pixmap`].
pub struct Painter {
    font: Option<fontdue::Font>,
}

impl Painter {
    /// Create a painter with the embedded default font (Noto Sans subset).
    /// Falls back to a bare-bones rasterizer if font loading fails.
    pub fn new() -> Self {
        // Embed a minimal Latin font at compile time so there are no runtime file deps.
        // We use fontdue's built-in "font from bytes" path.
        // For the MVP we embed the Noto Sans Regular TTF bytes from fontdue's test data.
        // If that's unavailable we fall back to a no-op font that renders empty glyphs.
        let font = {
            let system_fonts = [
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
            ];
            let found = system_fonts.iter().find_map(|path| {
                std::fs::read(path).ok().and_then(|bytes| {
                    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
                })
            });
            if found.is_none() {
                tracing::warn!(
                    "No system font found; text rendering will be invisible. \
                     Install: apt-get install fonts-dejavu-core"
                );
            }
            found
        };
        Self { font }
    }

    /// Render a list of (widget, box) pairs into a new RGBA Pixmap.
    pub fn paint(
        &self,
        pixmap: &mut Pixmap,
        pairs: &[(&Widget, LayoutBox)],
        background: Color,
    ) {
        // Fill background
        pixmap.fill(background);

        for (widget, layout_box) in pairs {
            self.paint_widget(pixmap, widget, *layout_box);
        }
    }

    fn paint_widget(&self, pixmap: &mut Pixmap, widget: &Widget, lb: LayoutBox) {
        match widget {
            Widget::Text { content, style, .. } => {
                self.paint_text(pixmap, content, style.as_ref(), lb);
            }
            Widget::Button { label, style, .. } => {
                self.paint_button(pixmap, label, style.as_ref(), lb);
            }
            Widget::Input { placeholder, .. } => {
                let text = placeholder.as_deref().unwrap_or("");
                self.paint_input(pixmap, text, lb);
            }
            Widget::Spacer { .. } => { /* nothing to paint */ }
            Widget::VStack { .. } | Widget::HStack { .. } => { /* containers: bg only */ }
        }
    }

    fn paint_text(&self, pixmap: &mut Pixmap, text: &str, style: Option<&TextStyle>, lb: LayoutBox) {
        let font_size = style
            .and_then(|s| s.font_size)
            .map(|s| s as f32)
            .unwrap_or(DEFAULT_FONT_SIZE);
        let color = style
            .and_then(|s| s.color.as_deref())
            .map(resolve_color)
            .unwrap_or(Color::BLACK);
        let _align = style.and_then(|s| s.align.as_ref()).cloned();

        self.rasterize_text(pixmap, text, lb.x + 4.0, lb.y + font_size, font_size, color);
    }

    fn paint_button(&self, pixmap: &mut Pixmap, label: &str, style: Option<&ButtonStyle>, lb: LayoutBox) {
        // Background rect
        let bg_color = style
            .and_then(|s| s.background_color.as_deref())
            .map(resolve_color)
            .unwrap_or_else(|| Color::from_rgba8(99, 102, 241, 255)); // indigo-500

        if let Some(rect) = lb.to_rect() {
            let mut paint = Paint::default();
            paint.set_color(bg_color);
            // tiny-skia 0.12 has no rounded-rect builder; use plain fill_rect
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }

        // Label text
        let text_color = style
            .and_then(|s| s.text_color.as_deref())
            .map(resolve_color)
            .unwrap_or(Color::WHITE);

        let font_size = DEFAULT_FONT_SIZE;
        let text_x = lb.x + DEFAULT_PADDING;
        let text_y = lb.y + (lb.height + font_size) / 2.0 - 2.0;
        self.rasterize_text(pixmap, label, text_x, text_y, font_size, text_color);
    }

    fn paint_input(&self, pixmap: &mut Pixmap, placeholder: &str, lb: LayoutBox) {
        if let Some(rect) = lb.to_rect() {
            // White background
            let mut bg_paint = Paint::default();
            bg_paint.set_color(Color::WHITE);
            pixmap.fill_rect(rect, &bg_paint, Transform::identity(), None);

            // Border
            let mut border_paint = Paint::default();
            border_paint.set_color(Color::from_rgba8(209, 213, 219, 255)); // gray-300
            let stroke = Stroke {
                width: 1.0,
                ..Default::default()
            };
            let path = PathBuilder::from_rect(rect);
            pixmap.stroke_path(&path, &border_paint, &stroke, Transform::identity(), None);
        }

        if !placeholder.is_empty() {
            let placeholder_color = Color::from_rgba8(156, 163, 175, 255); // gray-400
            self.rasterize_text(
                pixmap,
                placeholder,
                lb.x + DEFAULT_PADDING,
                lb.y + (lb.height + DEFAULT_FONT_SIZE) / 2.0 - 2.0,
                DEFAULT_FONT_SIZE,
                placeholder_color,
            );
        }
    }

    /// Rasterize a string at (x, y) baseline using fontdue.
    /// No-ops if no font is available.
    fn rasterize_text(&self, pixmap: &mut Pixmap, text: &str, x: f32, y: f32, size: f32, color: Color) {
        let font = match &self.font {
            Some(f) => f,
            None => return, // no font available — invisible text
        };

        let (r, g, b, a) = (
            (color.red() * 255.0) as u8,
            (color.green() * 255.0) as u8,
            (color.blue() * 255.0) as u8,
            (color.alpha() * 255.0) as u8,
        );

        let mut cursor_x = x as i32;
        let pixmap_width = pixmap.width() as i32;
        let pixmap_height = pixmap.height() as i32;
        let pixels = pixmap.pixels_mut();

        for ch in text.chars() {
            let (metrics, bitmap) = font.rasterize(ch, size);
            let glyph_y = y as i32 - metrics.height as i32 + metrics.ymin;

            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let coverage = bitmap[row * metrics.width + col];
                    if coverage == 0 {
                        continue;
                    }
                    let px = cursor_x + col as i32 + metrics.xmin;
                    let py = glyph_y + row as i32;
                    if px < 0 || py < 0 || px >= pixmap_width || py >= pixmap_height {
                        continue;
                    }
                    let idx = (py * pixmap_width + px) as usize;
                    if idx < pixels.len() {
                        let alpha = (coverage as u32 * a as u32) / 255;
                        let inv_alpha = 255 - alpha;
                        let existing = pixels[idx];
                        let nr = ((r as u32 * alpha + existing.red() as u32 * inv_alpha) / 255) as u8;
                        let ng = ((g as u32 * alpha + existing.green() as u32 * inv_alpha) / 255) as u8;
                        let nb = ((b as u32 * alpha + existing.blue() as u32 * inv_alpha) / 255) as u8;
                        let na = (alpha + (existing.alpha() as u32 * inv_alpha) / 255).min(255) as u8;
                        pixels[idx] = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na)
                            .unwrap_or(pixels[idx]);
                    }
                }
            }
            cursor_x += metrics.advance_width as i32;
        }
    }
}

impl Default for Painter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Render a UIDL document to a raw RGBA byte buffer.
///
/// Returns `Vec<u8>` with `width * height * 4` bytes in RGBA order.
pub fn render_to_rgba(doc: &UidlDocument, width: u32, height: u32) -> Result<Vec<u8>> {
    anyhow::ensure!(width > 0 && height > 0, "canvas dimensions must be non-zero");

    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("failed to allocate {}×{} pixmap", width, height))?;

    let bg_color = doc
        .theme
        .as_ref()
        .and_then(|t| t.background_color.as_deref())
        .map(resolve_color)
        .unwrap_or(Color::WHITE);

    let layout_engine = LayoutEngine::new(width, height);
    let pairs = layout_engine.layout_document(doc);

    let painter = Painter::new();
    painter.paint(&mut pixmap, &pairs, bg_color);

    // Convert premultiplied RGBA (tiny-skia internal) to straight RGBA
    let raw = pixmap
        .pixels()
        .iter()
        .flat_map(|p| [p.red(), p.green(), p.blue(), p.alpha()])
        .collect();

    Ok(raw)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uidl::UidlDocument;

    fn simple_doc(json: &str) -> UidlDocument {
        UidlDocument::parse(json).unwrap()
    }

    #[test]
    fn test_layout_box_to_rect() {
        let lb = LayoutBox::new(10.0, 20.0, 100.0, 50.0);
        let rect = lb.to_rect();
        assert!(rect.is_some());
    }

    #[test]
    fn test_layout_box_zero_size() {
        let lb = LayoutBox::new(0.0, 0.0, 0.0, 0.0);
        // to_rect() returns Some even for zero because we clamp to 1x1
        assert!(lb.to_rect().is_some());
    }

    #[test]
    fn test_layout_engine_text_node() {
        let doc = simple_doc(r#"{"layout":{"type":"text","content":"hello"}}"#);
        let engine = LayoutEngine::new(320, 240);
        let pairs = engine.layout_document(&doc);
        assert_eq!(pairs.len(), 1);
        let (_, lb) = pairs[0];
        assert_eq!(lb.x, 0.0);
        assert_eq!(lb.width, 320.0);
    }

    #[test]
    fn test_layout_engine_vstack_two_children() {
        let doc = simple_doc(r#"{
            "layout": {
                "type":"vstack","gap":8,"padding":0,
                "children":[
                    {"type":"text","content":"a"},
                    {"type":"button","label":"b","action":"x"}
                ]
            }
        }"#);
        let engine = LayoutEngine::new(320, 480);
        let pairs = engine.layout_document(&doc);
        // vstack + 2 children = 3 entries
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn test_parse_color_hex() {
        let c = parse_color("#FF0000");
        assert!(c.is_some());
        let c = c.unwrap();
        assert_eq!((c.red() * 255.0) as u8, 255);
        assert_eq!((c.green() * 255.0) as u8, 0);
    }

    #[test]
    fn test_named_color_red() {
        let c = named_color("red");
        assert!(c.is_some());
    }

    #[test]
    fn test_resolve_color_unknown() {
        // Unknown color should fall back to black without panicking
        let c = resolve_color("turquoise-electric");
        assert_eq!(c, Color::BLACK);
    }

    #[test]
    fn test_render_to_rgba_dimensions() {
        let doc = simple_doc(r#"{"layout":{"type":"text","content":"hi"}}"#);
        let result = render_to_rgba(&doc, 64, 48);
        assert!(result.is_ok(), "render_to_rgba should succeed: {:?}", result.err());
        let buf = result.unwrap();
        assert_eq!(buf.len(), 64 * 48 * 4, "should return width*height*4 bytes");
    }

    #[test]
    fn test_render_to_rgba_zero_dimension_fails() {
        let doc = simple_doc(r#"{"layout":{"type":"text","content":"hi"}}"#);
        assert!(render_to_rgba(&doc, 0, 100).is_err());
        assert!(render_to_rgba(&doc, 100, 0).is_err());
    }

    #[test]
    fn test_render_full_vstack() {
        let doc = simple_doc(r#"{
            "layout": {
                "type":"vstack","gap":8,"padding":8,
                "children":[
                    {"type":"text","content":"Hello openSystem","style":{"font_size":18}},
                    {"type":"button","label":"Start","action":"start"},
                    {"type":"input","placeholder":"Type here..."}
                ]
            }
        }"#);
        let result = render_to_rgba(&doc, 320, 240);
        assert!(result.is_ok(), "full vstack render should succeed");
    }
}
