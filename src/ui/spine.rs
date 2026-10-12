//! Independent spine paint and caption. No client surface is rendered here.
use std::cell::RefCell;

use niri_config::{Color, CornerRadius, Gradient, GradientRelativeTo};
use pango::{EllipsizeMode, FontDescription};
use pangocairo::cairo::{self, ImageSurface};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::utils::{Logical, Point, Rectangle, Scale, Size, Transform};

use super::spine_icon::AppIconCache;
use crate::layout::shrink_expand::ShrinkAxis;
use crate::niri_render_elements;
use crate::render_helpers::border::BorderRenderElement;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::render_helpers::RenderCtx;

niri_render_elements! {
    SpineRenderElement => {
        Fill = BorderRenderElement,
        Solid = SolidColorRenderElement,
        Caption = PrimaryGpuTextureRenderElement,
    }
}

#[derive(Debug, Default)]
pub struct Spine {
    fill: Option<BorderRenderElement>,
    solid: SolidColorBuffer,
    blocked: SolidColorBuffer,
    geometry: Option<PaintGeometry>,
    caption: RefCell<CaptionCache>,
}

#[derive(Debug, Clone, Copy)]
struct PaintGeometry {
    interior: Rectangle<f64, Logical>,
    axis: ShrinkAxis,
    scale: f64,
    text_color: Color,
}

#[derive(Debug, Default)]
struct CaptionCache {
    key: Option<CaptionKey>,
    label: Option<(TextureBuffer<GlesTexture>, Point<f64, Logical>)>,
    icon: Option<(TextureBuffer<GlesTexture>, Point<f64, Logical>)>,
    app_icon: AppIconCache,
}

#[derive(Debug, Clone, PartialEq)]
struct CaptionKey {
    title: String,
    app_id: String,
    axis: ShrinkAxis,
    size: Size<f64, Logical>,
    scale: f64,
    color: Color,
}

impl Spine {
    pub fn clear(&mut self) {
        self.fill = None;
        self.geometry = None;
        self.caption.get_mut().clear();
    }

    pub fn reload_icons(&mut self) {
        let caption = self.caption.get_mut();
        caption.key = None;
        caption.app_icon.clear();
    }

    pub fn update_shaders(&mut self) {
        if let Some(fill) = &mut self.fill {
            fill.damage_all();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        outer: Size<f64, Logical>,
        border: f64,
        axis: ShrinkAxis,
        scale: f64,
        gradient: Gradient,
        view: Rectangle<f64, Logical>,
        radius: CornerRadius,
        text_color: Option<Color>,
    ) {
        let interior = Rectangle::new(
            (border, border).into(),
            outer - Size::from((2. * border, 2. * border)),
        );
        let gradient_area = match gradient.relative_to {
            GradientRelativeTo::Window => Rectangle::from_size(interior.size),
            GradientRelativeTo::WorkspaceView => Rectangle::new(view.loc - interior.loc, view.size),
        };
        let radius = radius
            .expanded_by(-border as f32)
            .fit_to(interior.size.w as f32, interior.size.h as f32);
        let fill = self.fill.get_or_insert_with(BorderRenderElement::empty);
        fill.update(
            interior.size,
            gradient_area,
            gradient.in_,
            gradient.from,
            gradient.to,
            ((gradient.angle as f32) - 90.).to_radians(),
            Rectangle::from_size(interior.size),
            0.,
            radius,
            scale as f32,
            1.,
        );
        // Match native shader-unavailable fallback without importing any client content.
        self.solid.resize(interior.size);
        self.solid.set_color(gradient.from);
        self.blocked.resize(interior.size);
        self.blocked.set_color([0., 0., 0., 1.]);
        let color = text_color.unwrap_or_else(|| readable_text(gradient));
        self.geometry = Some(PaintGeometry {
            interior,
            axis,
            scale,
            text_color: color,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        alpha: f32,
        blocked: bool,
        title: &str,
        app_id: &str,
        push: &mut dyn FnMut(SpineRenderElement),
    ) {
        let Some(geometry) = self.geometry else {
            return;
        };
        let origin = location + geometry.interior.loc;
        if blocked {
            push(
                SolidColorRenderElement::from_buffer(
                    &self.blocked,
                    origin,
                    alpha,
                    Kind::Unspecified,
                )
                .into(),
            );
            return;
        }
        let mut caption = self.caption.borrow_mut();
        caption.update(
            ctx.as_gles().renderer,
            CaptionKey {
                title: title.chars().take(4096).collect(),
                app_id: app_id.chars().take(1024).collect(),
                axis: geometry.axis,
                size: geometry.interior.size,
                scale: geometry.scale,
                color: geometry.text_color,
            },
        );
        for (texture, offset) in caption.label.iter().chain(caption.icon.iter()) {
            let elem = TextureRenderElement::from_texture_buffer(
                texture.clone(),
                origin + *offset,
                alpha,
                None,
                None,
                Kind::Unspecified,
            );
            push(SpineRenderElement::Caption(PrimaryGpuTextureRenderElement(
                elem,
            )));
        }
        if BorderRenderElement::has_shader(ctx.renderer) {
            if let Some(fill) = &self.fill {
                push(fill.clone().with_location(origin).with_alpha(alpha).into());
            }
        } else {
            push(
                SolidColorRenderElement::from_buffer(&self.solid, origin, alpha, Kind::Unspecified)
                    .into(),
            );
        }
    }
}

impl CaptionCache {
    fn clear(&mut self) {
        self.key = None;
        self.label = None;
        self.icon = None;
        self.app_icon.clear();
    }

    fn update(&mut self, renderer: &mut GlesRenderer, key: CaptionKey) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.label = None;
        self.icon = None;
        let vertical = key.axis == ShrinkAxis::Horizontal;
        let (cross, length) = if vertical {
            (key.size.w, key.size.h)
        } else {
            (key.size.h, key.size.w)
        };
        let pad = 4_f64.min(cross / 4.).min(length / 4.);
        let icon_size = 16_f64.min(cross - 2. * pad).min(length - 2. * pad);
        let text_length = length - 2. * pad - icon_size - pad;
        let has_text = cross >= 12. && text_length >= 24.;
        if icon_size >= 4. {
            let offset = if vertical {
                Point::from((
                    (cross - icon_size) / 2.,
                    if has_text {
                        pad
                    } else {
                        (length - icon_size) / 2.
                    },
                ))
            } else {
                Point::from((
                    if has_text {
                        pad
                    } else {
                        (length - icon_size) / 2.
                    },
                    (cross - icon_size) / 2.,
                ))
            };
            let surface = self.app_icon.get(
                &key.app_id,
                (icon_size * key.scale).round().clamp(1., 256.) as i32,
            );
            self.icon = surface
                .map(Ok)
                .unwrap_or_else(|| fallback_icon(icon_size, key.scale, key.color))
                .and_then(|surface| upload(renderer, surface, key.scale))
                .ok()
                .map(|texture| (texture, align(offset, key.scale)));
        }
        if has_text {
            let text = if key.title.is_empty() {
                if key.app_id.is_empty() {
                    "Window"
                } else {
                    &key.app_id
                }
            } else {
                &key.title
            };
            self.label = label_surface(
                text,
                text_length,
                cross - 2. * pad,
                key.scale,
                key.color,
                vertical,
            )
            .and_then(|surface| upload(renderer, surface, key.scale))
            .ok()
            .map(|texture| {
                let size = texture.logical_size();
                let offset = if vertical {
                    Point::from(((cross - size.w) / 2., pad + icon_size + pad))
                } else {
                    Point::from((pad + icon_size + pad, (cross - size.h) / 2.))
                };
                (texture, align(offset, key.scale))
            });
        }
        // Failed resources are cached too, rather than retried on every idle frame.
        self.key = Some(key);
    }
}

fn align(point: Point<f64, Logical>, scale: f64) -> Point<f64, Logical> {
    point.to_physical_precise_round(scale).to_logical(scale)
}

fn readable_text(gradient: Gradient) -> Color {
    let luminance = |color: Color| {
        let linear = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    };
    let low = luminance(gradient.from).min(luminance(gradient.to));
    let high = luminance(gradient.from).max(luminance(gradient.to));
    if (low + 0.05) / 0.05 >= 1.05 / (high + 0.05) {
        Color::from_array_unpremul([0., 0., 0., 1.])
    } else {
        Color::from_array_unpremul([1., 1., 1., 1.])
    }
}

fn label_surface(
    title: &str,
    length: f64,
    cross: f64,
    scale: f64,
    color: Color,
    vertical: bool,
) -> anyhow::Result<ImageSurface> {
    let width = (length * scale).floor().clamp(1., 16383.) as i32;
    let max_height = (cross * scale).floor().clamp(1., 256.) as i32;
    let surface = ImageSurface::create(cairo::Format::ARgb32, 0, 0)?;
    let cr = cairo::Context::new(&surface)?;
    let layout = pangocairo::functions::create_layout(&cr);
    let mut font = FontDescription::from_string("Sans 12");
    font.set_absolute_size(12_f64.min(cross) * scale * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_single_paragraph_mode(true);
    layout.set_width(width * pango::SCALE);
    layout.set_ellipsize(EllipsizeMode::End);
    layout.set_text(title);
    let (_, line_height) = layout.pixel_size();
    if line_height > max_height {
        font.set_absolute_size(font.size() as f64 * f64::from(max_height) / f64::from(line_height));
        layout.set_font_description(Some(&font));
    }
    let (text_width, text_height) = layout.pixel_size();
    let width = text_width.clamp(1, width);
    let height = text_height.clamp(1, max_height);
    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    let cr = cairo::Context::new(&surface)?;
    cr.set_source_rgba(
        f64::from(color.r),
        f64::from(color.g),
        f64::from(color.b),
        f64::from(color.a),
    );
    pangocairo::functions::show_layout(&cr, &layout);
    drop(cr);
    if !vertical {
        return Ok(surface);
    }
    let rotated = ImageSurface::create(cairo::Format::ARgb32, height, width)?;
    let cr = cairo::Context::new(&rotated)?;
    cr.translate(f64::from(height), 0.);
    cr.rotate(std::f64::consts::FRAC_PI_2);
    cr.set_source_surface(&surface, 0., 0.)?;
    cr.source().set_filter(cairo::Filter::Nearest);
    cr.paint()?;
    drop(cr);
    Ok(rotated)
}

fn fallback_icon(size: f64, scale: f64, color: Color) -> anyhow::Result<ImageSurface> {
    let pixels = (size * scale).round().clamp(1., 256.) as i32;
    let surface = ImageSurface::create(cairo::Format::ARgb32, pixels, pixels)?;
    let cr = cairo::Context::new(&surface)?;
    cr.scale(f64::from(pixels) / 16., f64::from(pixels) / 16.);
    cr.set_source_rgba(
        f64::from(color.r),
        f64::from(color.g),
        f64::from(color.b),
        f64::from(color.a),
    );
    cr.set_line_width(1.5);
    cr.rectangle(2., 3., 12., 10.);
    cr.move_to(2., 6.);
    cr.line_to(14., 6.);
    cr.stroke()?;
    drop(cr);
    Ok(surface)
}

fn upload(
    renderer: &mut GlesRenderer,
    surface: ImageSurface,
    scale: f64,
) -> anyhow::Result<TextureBuffer<GlesTexture>> {
    let size = (surface.width(), surface.height());
    let mut result = None;
    surface
        .with_data(|data| {
            result = Some(TextureBuffer::from_memory(
                renderer,
                data,
                Fourcc::Argb8888,
                size,
                false,
                Scale::from(scale),
                Transform::Normal,
                Vec::new(),
            ));
        })
        .map_err(|error| anyhow::anyhow!("cannot read caption surface: {error}"))?;
    Ok(result.unwrap()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_title_is_shaped_once_then_rotated_as_one_texture() {
        for scale in [1., 1.25, 2.] {
            let color = Color::from_array_unpremul([0.9, 0.8, 0.7, 1.]);
            let mut horizontal = label_surface(
                "Editor 编辑器 · ملف / long title",
                110.,
                20.,
                scale,
                color,
                false,
            )
            .unwrap();
            let mut vertical = label_surface(
                "Editor 编辑器 · ملف / long title",
                110.,
                20.,
                scale,
                color,
                true,
            )
            .unwrap();
            let (w, h) = (horizontal.width(), horizontal.height());
            assert_eq!((vertical.width(), vertical.height()), (h, w));
            let hs = horizontal.stride() as usize;
            let vs = vertical.stride() as usize;
            let original = horizontal.data().unwrap().to_vec();
            let rotated = vertical.data().unwrap().to_vec();
            assert!(original.iter().any(|v| *v != 0));
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let from = y * hs + x * 4;
                    let to = x * vs + (h as usize - 1 - y) * 4;
                    assert_eq!(&original[from..from + 4], &rotated[to..to + 4]);
                }
            }
        }
    }

    #[test]
    fn caption_dimensions_are_bounded_for_long_titles_and_narrow_cross_axis() {
        let title = "mixed中".repeat(8000);
        for vertical in [false, true] {
            let surface = label_surface(
                &title,
                80.,
                14.,
                1.25,
                Color::from_array_unpremul([1.; 4]),
                vertical,
            )
            .unwrap();
            let (length, cross) = if vertical {
                (surface.height(), surface.width())
            } else {
                (surface.width(), surface.height())
            };
            assert!(length <= 100 && cross <= 17);
        }
    }

    #[test]
    fn automatic_text_contrast_tracks_paint() {
        let black = Color::from_array_unpremul([0., 0., 0., 1.]);
        let white = Color::from_array_unpremul([1.; 4]);
        assert_eq!(readable_text(black.into()), white);
        assert_eq!(readable_text(white.into()), black);
    }
}
