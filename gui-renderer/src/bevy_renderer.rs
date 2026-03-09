//! Bevy ECS renderer for UIDL documents.
//!
//! Architecture:
//!   UidlDocument → spawn_widget_tree() → Bevy Entities/Components → wgpu → KMS/DRM
//!
//! TODO: Add `bevy = { version = "0.13" }` to Cargo.toml when implementing.
//! Requires: bevy, wgpu, either Vulkan/OpenGL drivers or DRM for framebuffer access.

use crate::uidl::UidlDocument;
use anyhow::Result;

/// Handle to a rendered widget tree in Bevy ECS
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderHandle(pub u64);

/// Bevy renderer — interprets UIDL and manages ECS entities
pub struct BevyRenderer {
    // TODO: Replace with actual Bevy App when implementing
    // app: bevy::app::App,
    next_handle: u64,
}

impl BevyRenderer {
    pub fn new() -> Self {
        // TODO: Initialize Bevy app with:
        //   - DefaultPlugins (or MinimalPlugins + custom window plugin)
        //   - UIDLPlugin (custom plugin for UIDL → ECS mapping)
        //   - wgpu backend selection (Vulkan → OpenGL → Software)
        Self { next_handle: 1 }
    }

    /// Render a UIDL document, returning a handle for future updates
    pub fn render(&mut self, doc: &UidlDocument) -> Result<RenderHandle> {
        let handle = RenderHandle(self.next_handle);
        self.next_handle += 1;

        // TODO: Implement Bevy ECS entity spawning:
        //   self.spawn_widget_tree(&doc.layout, None)?;
        tracing::info!(
            "BevyRenderer::render — {} widgets (handle={})",
            doc.widget_count(),
            handle.0
        );
        Ok(handle)
    }

    /// Apply a partial update to an existing render tree
    pub fn update(&mut self, handle: RenderHandle, new_doc: &UidlDocument) -> Result<()> {
        // TODO: Diff old ECS tree with new UidlDocument and emit minimal ECS mutations
        tracing::info!(
            "BevyRenderer::update — handle={}, {} widgets",
            handle.0,
            new_doc.widget_count()
        );
        Ok(())
    }

    /// Remove a render tree
    pub fn destroy(&mut self, handle: RenderHandle) -> Result<()> {
        // TODO: Despawn all ECS entities associated with handle
        tracing::info!("BevyRenderer::destroy — handle={}", handle.0);
        Ok(())
    }
}

impl Default for BevyRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Software renderer fallback using tiny-skia (no GPU required)
///
/// Used when: Bevy init fails, no GPU available, headless testing
pub struct SoftwareRenderer {
    width: u32,
    height: u32,
    // TODO: framebuffer: tiny_skia::Pixmap,
}

impl SoftwareRenderer {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        if width == 0 || height == 0 {
            anyhow::bail!(
                "SoftwareRenderer dimensions must be non-zero, got {}x{}",
                width,
                height
            );
        }
        // TODO: Add tiny-skia to Cargo.toml and initialize:
        //   framebuffer: tiny_skia::Pixmap::new(width, height).unwrap()
        Ok(Self { width, height })
    }

    /// Render UIDL to raw RGBA pixel buffer
    pub fn render_to_buffer(&self, doc: &UidlDocument) -> Result<Vec<u8>> {
        // TODO: Walk UidlDocument tree and paint using tiny-skia primitives:
        //   draw_widget(pixmap, &doc.layout, 0, 0, self.width, self.height)
        let pixel_count = (self.width * self.height) as usize;
        let buffer = vec![0u8; pixel_count * 4]; // RGBA black
        tracing::info!(
            "SoftwareRenderer::render_to_buffer — {}x{}, {} widgets (stub)",
            self.width,
            self.height,
            doc.widget_count()
        );
        Ok(buffer)
    }
}

/// Choose between hardware and software renderer
pub enum Renderer {
    Hardware(BevyRenderer),
    Software(SoftwareRenderer),
}

impl Renderer {
    /// Auto-detect best available renderer.
    /// Returns an error if the given dimensions are invalid (zero width or height).
    pub fn auto_detect(width: u32, height: u32) -> anyhow::Result<Self> {
        // TODO: Try to initialize BevyRenderer with GPU; fall back to SoftwareRenderer
        // For now, always use software renderer (safe default)
        tracing::info!("Renderer::auto_detect — using SoftwareRenderer (GPU detection TODO)");
        Ok(Renderer::Software(SoftwareRenderer::new(width, height)?))
    }
}
