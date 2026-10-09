//! Composite a projected native window only after its real lower scene is available.
use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};
use crate::layout::opening_window::OpenPresentation;
use crate::layout::tile::TileRenderElement;
use crate::render_helpers::background_effect::RenderParams;
use crate::render_helpers::encompassing_geo;
use crate::render_helpers::framebuffer_effect::FramebufferEffect;
use crate::render_helpers::offscreen::{OffscreenBuffer, OffscreenData, OffscreenRenderElement};
use crate::render_helpers::renderer::AsGlesFrame as _;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::render_helpers::RenderTarget;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{
    Bind as _, Color32F, ContextId, Frame as _, FrameContext as _, Offscreen as _, Renderer,
    Texture as _,
};
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Transform};
use std::cell::RefCell;
use std::rc::Rc;

use crate::render_helpers::offscreen::OffscreenDataSlot;
type Signature = Vec<(Id, CommitCounter, Rectangle<i32, Physical>, u32)>;

#[derive(Debug)]
pub struct ProjectionBuffer {
    id: Id,
    slots: [ProjectionSlot; RenderTarget::COUNT],
}
#[derive(Debug)]
struct ProjectionSlot {
    signature: RefCell<Signature>,
    commit: RefCell<CommitCounter>,
    animation_signature: RefCell<(u64, u32)>,
    captures: RefCell<CommitCounter>,
    scene: RefCell<Option<SceneTexture>>,
    object: OffscreenBuffer,
    opening: OffscreenBuffer,
    backdrop: FramebufferEffect,
    backdrop_cache: RefCell<UserDataMap>,
    renderer_context: RefCell<Option<ContextId<GlesTexture>>>,
    child_caches: RefCell<std::collections::HashMap<Id, Rc<UserDataMap>>>,
}
#[derive(Debug)]
struct SceneTexture {
    texture: GlesTexture,
    context: ContextId<GlesTexture>,
}

#[derive(Debug)]
pub struct ProjectedGroup {
    buffer: Rc<ProjectionBuffer>,
    elements: Vec<TileRenderElement<GlesRenderer>>,
    source: Rectangle<f64, Logical>,
    geometry: Rectangle<f64, Logical>,
    content_source: Rectangle<f64, Logical>,
    capture_geometry: Rectangle<f64, Logical>,
    open: Option<OpenPresentation>,
    tile_size: smithay::utils::Size<f64, Logical>,
    tile_alpha: f32,
    native_scale: Scale<f64>,
    alpha: f32,
    target: RenderTarget,
    data: Option<OffscreenDataSlot>,
    native_data: Option<OffscreenData>,
    id: Id,
    commit: CommitCounter,
}

impl Default for ProjectionBuffer {
    fn default() -> Self {
        Self {
            id: Id::new(),
            slots: std::array::from_fn(|_| ProjectionSlot {
                signature: Default::default(),
                commit: Default::default(),
                animation_signature: Default::default(),
                captures: Default::default(),
                scene: Default::default(),
                object: Default::default(),
                opening: Default::default(),
                backdrop: FramebufferEffect::new(),
                backdrop_cache: RefCell::new(UserDataMap::new()),
                renderer_context: Default::default(),
                child_caches: Default::default(),
            }),
        }
    }
}

impl ProjectedGroup {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer: Rc<ProjectionBuffer>,
        elements: Vec<TileRenderElement<GlesRenderer>>,
        position: Point<f64, Logical>,
        zoom: f64,
        alpha: f32,
        native_scale: f64,
        target: RenderTarget,
        data: Option<OffscreenDataSlot>,
        native_data: Option<OffscreenData>,
        open: Option<OpenPresentation>,
        tile_size: smithay::utils::Size<f64, Logical>,
        tile_alpha: f32,
        custom_open: bool,
    ) -> Self {
        let scale = Scale::from(native_scale);
        let source = encompassing_geo(scale, elements.iter())
            .to_f64()
            .to_logical(scale);
        let content_source = source;
        let source = open.as_ref().map_or(source, |open| {
            open.geometry(source, tile_size, scale, custom_open)
        });
        let capture_source = open
            .as_ref()
            .filter(|_| !custom_open)
            .map_or(content_source, |open| {
                open.geometry(content_source, tile_size, scale, false)
            });
        let capture_geometry = Rectangle::new(
            position + capture_source.loc.upscale(zoom),
            capture_source.size.upscale(zoom),
        );
        let signature = elements
            .iter()
            .map(|e| {
                (
                    e.id().clone(),
                    e.current_commit(),
                    e.geometry(scale),
                    e.alpha().to_bits(),
                )
            })
            .collect();
        let slot = &buffer.slots[target as usize];
        let animation_signature = (
            open.as_ref().map_or(0, |open| open.progress_signature()),
            tile_alpha.to_bits(),
        );
        if *slot.signature.borrow() != signature
            || *slot.animation_signature.borrow() != animation_signature
        {
            *slot.animation_signature.borrow_mut() = animation_signature;
            *slot.signature.borrow_mut() = signature;
            slot.commit.borrow_mut().increment();
        }
        let commit = *slot.commit.borrow();
        let id = buffer.id.clone().namespaced(target as usize);
        Self {
            buffer,
            elements,
            source,
            geometry: Rectangle::new(
                position + source.loc.upscale(zoom),
                source.size.upscale(zoom),
            ),
            content_source,
            capture_geometry,
            open,
            tile_size,
            tile_alpha,
            native_scale: scale,
            alpha,
            target,
            data,
            native_data,
            id,
            commit,
        }
    }

    fn prepare(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    ) -> Result<(), GlesError> {
        let slot = &self.buffer.slots[self.target as usize];
        let rendered = cache
            .get_or_insert::<RefCell<Option<OffscreenRenderElement>>, _>(|| RefCell::new(None));
        rendered.borrow_mut().take();
        let ratio = dst.size.to_f64() / src.size;
        let full_origin = dst.loc.to_f64()
            - src
                .loc
                .to_logical(1., Transform::Normal, &src.size)
                .to_physical(1.)
                .upscale(ratio);
        let zoom = self.geometry.size.w / self.source.size.w;
        let projected_to_output = Scale::from((ratio.x / zoom, ratio.y / zoom));
        let capture_rel = (self.capture_geometry.loc - self.geometry.loc)
            .to_physical(1.)
            .upscale(projected_to_output);
        let full_dst = Rectangle::new(
            (full_origin + capture_rel).to_i32_round(),
            self.capture_geometry
                .size
                .to_physical(1.)
                .upscale(projected_to_output)
                .to_i32_round(),
        );
        let backdrop = slot.backdrop.render(
            None,
            RenderParams {
                geometry: self.content_source,
                subregion: None,
                clip: None,
                scale: self.native_scale.x,
            },
            None,
            0.,
            1.,
        );
        if slot.renderer_context.borrow().as_ref() != Some(&frame.context_id()) {
            *slot.renderer_context.borrow_mut() = Some(frame.context_id());
            *slot.backdrop_cache.borrow_mut() = UserDataMap::new();
            slot.child_caches.borrow_mut().clear();
        }
        let background_cache = slot.backdrop_cache.borrow();
        let background_cache = &*background_cache;
        RenderElement::<GlesRenderer>::capture_framebuffer(
            &backdrop,
            frame,
            backdrop.src(),
            full_dst,
            background_cache,
        )?;
        let Some(captured) = backdrop.captured_texture(background_cache) else {
            return Ok(());
        };
        let mut guard = frame.renderer();
        let renderer = guard.as_mut();
        let pixel_size = self
            .content_source
            .size
            .to_physical_precise_round(self.native_scale);
        if pixel_size.w <= 0 || pixel_size.h <= 0 {
            return Ok(());
        }
        let size = pixel_size.to_logical(1).to_buffer(1, Transform::Normal);
        let mut scene = slot.scene.borrow_mut();
        if scene
            .as_ref()
            .is_none_or(|s| s.texture.size() != size || s.context != renderer.context_id())
        {
            *scene = Some(SceneTexture {
                texture: renderer.create_buffer(Fourcc::Abgr8888, size)?,
                context: renderer.context_id(),
            });
        }
        let texture = &mut scene.as_mut().unwrap().texture;
        let base = TextureRenderElement::from_texture_buffer(
            TextureBuffer::from_texture(
                renderer,
                captured.texture,
                self.native_scale,
                captured.transform,
                Vec::new(),
            ),
            (0., 0.),
            1.,
            Some(captured.source),
            Some(self.content_source.size),
            Kind::Unspecified,
        );
        let children_cache = {
            let mut caches = slot.child_caches.borrow_mut();
            caches.retain(|id, _| self.elements.iter().any(|element| element.id() == id));
            self.elements
                .iter()
                .map(|element| {
                    caches
                        .entry(element.id().clone())
                        .or_insert_with(|| Rc::new(UserDataMap::new()))
                        .clone()
                })
                .collect::<Vec<_>>()
        };
        {
            let mut target = renderer.bind(texture)?;
            let mut scene_frame = renderer.render(&mut target, pixel_size, Transform::Normal)?;
            let damage = [Rectangle::from_size(pixel_size)];
            scene_frame.clear(Color32F::TRANSPARENT, &damage)?;
            RenderElement::<GlesRenderer>::draw(
                &base,
                &mut scene_frame,
                base.src(),
                Rectangle::from_size(pixel_size),
                &damage,
                &[],
                None,
            )?;
            // Resolve each popup/main effect against the lower scene, including previously drawn
            // parts of this same window, never foreground windows or the window's own future pixels.
            for (index, element) in self.elements.iter().enumerate().rev() {
                let mut dst = element.geometry(self.native_scale);
                dst.loc -= self
                    .content_source
                    .loc
                    .to_physical_precise_round(self.native_scale);
                let Some(mut damage) = dst.intersection(Rectangle::from_size(pixel_size)) else {
                    continue;
                };
                damage.loc -= dst.loc;
                let child_cache = &children_cache[index];
                if element.is_framebuffer_effect() {
                    element.capture_framebuffer(
                        &mut scene_frame,
                        element.src(),
                        dst,
                        child_cache,
                    )?;
                }
                element.draw(
                    &mut scene_frame,
                    element.src(),
                    dst,
                    &[damage],
                    &[],
                    Some(child_cache),
                )?;
            }
            // The scene and the object are submitted on this same GLES context.
            let _scene_sync = scene_frame.finish()?;
        }
        slot.captures.borrow_mut().increment();
        let counter = *slot.captures.borrow();
        let children = self
            .elements
            .iter()
            .zip(children_cache)
            .map(|(element, cache)| CachedNative {
                element,
                cache,
                counter,
            })
            .collect::<Vec<_>>();
        // Keep the scene's backdrop out of the transparent object. Depth alpha is applied once,
        // after content, native alpha, chrome, shadows and resolved effects have been combined.
        let (element, _sync, mut states) = slot
            .object
            .render(renderer, self.native_scale, &children)
            .map_err(|e| {
                warn!("projected group render failed: {e:?}");
                GlesError::BlitError
            })?;
        let element = if let Some(open) = &self.open {
            let (opening, native_states) = open
                .render(
                    renderer,
                    element,
                    states,
                    self.tile_size,
                    Point::from((0., 0.)),
                    self.native_scale,
                    self.tile_alpha,
                )
                .map_err(|_| GlesError::BlitError)?;
            let (element, _, mut data) = slot
                .opening
                .render(renderer, self.native_scale, &[opening])
                .map_err(|_| GlesError::BlitError)?;
            data.states.states.extend(native_states.states.states);
            states = data;
            element
        } else {
            element.with_alpha(self.tile_alpha)
        };
        states.id = self.id.clone();
        if self.target == RenderTarget::Output {
            if let Some(slot) = &self.data {
                let mut data = slot.borrow_mut();
                if let Some(existing) = self.native_data.as_ref() {
                    states.states.states.extend(existing.states.states.clone());
                }
                *data = Some(states);
            }
        }
        *rendered.borrow_mut() = Some(element.with_alpha(
            self.alpha
                * if self.open.is_some() {
                    1.
                } else {
                    self.tile_alpha
                },
        ));
        Ok(())
    }
}

impl Element for ProjectedGroup {
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        self.commit
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(self.source.size).to_buffer(1., Transform::Normal, &self.source.size)
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry.to_physical_precise_round(scale)
    }
    fn alpha(&self) -> f32 {
        self.alpha
    }
    fn is_framebuffer_effect(&self) -> bool {
        true
    }
}
impl RenderElement<GlesRenderer> for ProjectedGroup {
    fn capture_framebuffer(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    ) -> Result<(), GlesError> {
        self.prepare(frame, src, dst, cache)
    }
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        let Some(element) = cache.and_then(|c| c.get::<RefCell<Option<OffscreenRenderElement>>>())
        else {
            return Ok(());
        };
        let element = element.borrow();
        let Some(element) = element.as_ref() else {
            return Ok(());
        };
        let ratio = element.src().size / self.source.size.to_buffer(1., Transform::Normal);
        let source = Rectangle::new(src.loc.upscale(ratio), src.size.upscale(ratio));
        RenderElement::<GlesRenderer>::draw(element, frame, source, dst, damage, opaque, None)
    }
}
impl<'render> RenderElement<TtyRenderer<'render>> for ProjectedGroup {
    fn capture_framebuffer(
        &self,
        frame: &mut TtyFrame<'_, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    ) -> Result<(), TtyRendererError<'render>> {
        Ok(self.prepare(frame.as_gles_frame(), src, dst, cache)?)
    }
    fn draw(
        &self,
        frame: &mut TtyFrame<'_, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), TtyRendererError<'render>> {
        Ok(RenderElement::<GlesRenderer>::draw(
            self,
            frame.as_gles_frame(),
            src,
            dst,
            damage,
            opaque,
            cache,
        )?)
    }
}

struct CachedNative<'a> {
    element: &'a TileRenderElement<GlesRenderer>,
    cache: Rc<UserDataMap>,
    counter: CommitCounter,
}
impl Element for CachedNative<'_> {
    fn id(&self) -> &Id {
        self.element.id()
    }
    fn current_commit(&self) -> CommitCounter {
        // A projection can move over xray or non-xray backgrounds without a client commit.
        // Its transparent native object must reflect this capture, not a cached old sample.
        self.counter
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.element.src()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.element.geometry(scale)
    }
    fn transform(&self) -> Transform {
        self.element.transform()
    }
    fn alpha(&self) -> f32 {
        self.element.alpha()
    }
    fn opaque_regions(
        &self,
        scale: Scale<f64>,
    ) -> smithay::backend::renderer::utils::OpaqueRegions<i32, Physical> {
        self.element.opaque_regions(scale)
    }
}
impl RenderElement<GlesRenderer> for CachedNative<'_> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
        _cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        self.element
            .draw(frame, src, dst, damage, opaque, Some(&self.cache))
    }
}
