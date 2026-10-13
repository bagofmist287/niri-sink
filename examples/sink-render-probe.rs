//! GPU oracle for projection sampling across output transforms and clipped edges.
use std::rc::Rc;

use niri::layout::tile::TileRenderElement;
use niri::render_helpers::background_effect::{BackgroundEffectElement, RenderParams};
use niri::render_helpers::blur::BlurOptions;
use niri::render_helpers::framebuffer_effect::FramebufferEffect;
use niri::render_helpers::projected_group::{ProjectedGroup, ProjectionBuffer};
use niri::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use niri::render_helpers::{shaders, RenderTarget};
use niri_config::CornerRadius;
use smithay::backend::allocator::Fourcc;
use smithay::backend::egl::native::EGLSurfacelessDisplay;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::element::utils::{Relocate, RelocateRenderElement};
use smithay::backend::renderer::element::{Element, Kind, RenderElement};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{
    Bind as _, Color32F, ExportMem as _, Frame as _, Offscreen as _, Renderer as _,
};
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform};

fn solid(
    location: (f64, f64),
    size: (f64, f64),
    color: [f32; 4],
) -> TileRenderElement<GlesRenderer> {
    SolidColorRenderElement::from_buffer(
        &SolidColorBuffer::new(size, color),
        location,
        1.,
        Kind::Unspecified,
    )
    .into()
}
fn native(scale: f64) -> Vec<TileRenderElement<GlesRenderer>> {
    let effect = |rect: Rectangle<f64, Logical>| -> TileRenderElement<GlesRenderer> {
        let radius = CornerRadius::from(9.);
        let effect = FramebufferEffect::new().render(
            None,
            RenderParams {
                geometry: rect,
                subregion: None,
                clip: Some((rect, radius)),
                scale,
            },
            (std::env::var_os("SINK_PROBE_NO_BLUR").is_none()).then_some(BlurOptions {
                passes: 2,
                offset: 1.,
            }),
            0.,
            0.7,
        );
        BackgroundEffectElement::from(effect).into()
    };
    vec![
        solid((65., 20.), (50., 40.), [0., 0.2, 0.1, 0.4]),
        effect(Rectangle::new((65., 20.).into(), (50., 40.).into())),
        solid((0., 0.), (160., 100.), [0.3, 0., 0.1, 0.5]),
        effect(Rectangle::from_size((160., 100.).into())),
    ]
}
#[allow(clippy::too_many_arguments)]
fn render(
    renderer: &mut GlesRenderer,
    transform: Transform,
    scale: f64,
    position: Point<f64, Logical>,
    projected: bool,
    buffer: Rc<ProjectionBuffer>,
    render_target: RenderTarget,
    blocked: bool,
) -> anyhow::Result<Vec<u8>> {
    let size = Size::<i32, Physical>::from((256, 192));
    let mut texture: GlesTexture = renderer.create_buffer(
        Fourcc::Abgr8888,
        size.to_logical(1).to_buffer(1, Transform::Normal),
    )?;
    let mut target = renderer.bind(&mut texture)?;
    let mut frame = renderer.render(&mut target, size, transform)?;
    let area = Rectangle::from_size(frame.output_size());
    frame.clear(Color32F::BLACK, &[area])?;
    let logical = area.size.to_f64().to_logical(scale);
    for y in 0..(logical.h / 16.).ceil() as i32 {
        for x in 0..(logical.w / 16.).ceil() as i32 {
            let color = if (x + y) % 2 == 0 {
                [0.2, 0.4, 0.8, 1.]
            } else {
                [0.8, 0.4, 0.2, 1.]
            };
            let e = solid((x as f64 * 16., y as f64 * 16.), (16., 16.), color);
            let dst = e.geometry(Scale::from(scale));
            let mut damage = area.intersection(dst).unwrap();
            damage.loc -= dst.loc;
            e.draw(&mut frame, e.src(), dst, &[damage], &[], None)?;
        }
    }
    let native = if blocked {
        vec![solid((0., 0.), (160., 100.), [0., 0., 0., 1.])]
    } else {
        native(scale)
    };
    let elements: Vec<Box<dyn RenderElement<GlesRenderer>>> = if projected {
        vec![Box::new(ProjectedGroup::new(
            buffer,
            native,
            position,
            1.,
            1.,
            scale,
            render_target,
            None,
            None,
            None,
            (160., 100.).into(),
            1.,
            false,
        ))]
    } else {
        native
            .into_iter()
            .map(|e| {
                Box::new(RelocateRenderElement::from_element(
                    e,
                    position.to_physical_precise_round(scale),
                    Relocate::Relative,
                )) as Box<dyn RenderElement<GlesRenderer>>
            })
            .collect()
    };
    for e in elements.iter().rev() {
        let dst = e.geometry(Scale::from(scale));
        let Some(mut damage) = area.intersection(dst) else {
            continue;
        };
        damage.loc -= dst.loc;
        let cache = UserDataMap::new();
        if e.is_framebuffer_effect() {
            e.capture_framebuffer(&mut frame, e.src(), dst, &cache)?;
        }
        e.draw(&mut frame, e.src(), dst, &[damage], &[], Some(&cache))?;
    }
    frame.finish()?.wait()?;
    let mapping = renderer.copy_framebuffer(
        &target,
        Rectangle::from_size(size.to_logical(1).to_buffer(1, Transform::Normal)),
        Fourcc::Abgr8888,
    )?;
    Ok(renderer.map_texture(&mapping)?.to_vec())
}
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("smithay::backend::renderer::gles=info,niri=warn")
        .init();
    let mut renderer = unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay)?;
        let context = EGLContext::new(&display)?;
        GlesRenderer::new(context)?
    };
    shaders::init(&mut renderer);
    let transforms = [
        Transform::Normal,
        Transform::_90,
        Transform::_180,
        Transform::_270,
        Transform::Flipped,
        Transform::Flipped90,
        Transform::Flipped180,
        Transform::Flipped270,
    ];
    let mut count = 0;
    for transform in transforms {
        for scale in [1., 1.25, 1.5] {
            for position in [(40., 30.), (-24., -13.), (200., 150.)] {
                let position = Point::from(position);
                let a = render(
                    &mut renderer,
                    transform,
                    scale,
                    position,
                    false,
                    Rc::new(ProjectionBuffer::default()),
                    RenderTarget::Output,
                    false,
                )?;
                let b = render(
                    &mut renderer,
                    transform,
                    scale,
                    position,
                    true,
                    Rc::new(ProjectionBuffer::default()),
                    RenderTarget::Output,
                    false,
                )?;
                let differences = a
                    .iter()
                    .zip(&b)
                    .map(|(a, b)| a.abs_diff(*b) as u64)
                    .collect::<Vec<_>>();
                let mean = differences.iter().sum::<u64>() as f64 / differences.len() as f64;
                let max = *differences.iter().max().unwrap();
                println!("transform={transform:?} scale={scale} position={position:?} mean={mean:.6} max={max}");
                if mean >= 0.8 || max >= 12 {
                    std::fs::write("/tmp/sink-native.rgba", &a)?;
                    std::fs::write("/tmp/sink-projected.rgba", &b)?;
                }
                anyhow::ensure!(
                    mean < 0.8 && max < 12,
                    "projection differs from native sampling"
                );
                count += 1;
            }
        }
    }
    let buffer = Rc::new(ProjectionBuffer::default());
    let weak = Rc::downgrade(&buffer);
    let position = Point::from((20., 20.));
    let output = render(
        &mut renderer,
        Transform::Normal,
        1.,
        position,
        true,
        buffer.clone(),
        RenderTarget::Output,
        false,
    )?;
    for target in [RenderTarget::ScreenCapture, RenderTarget::Screencast] {
        let native = render(
            &mut renderer,
            Transform::Normal,
            1.,
            position,
            false,
            buffer.clone(),
            target,
            true,
        )?;
        let captured = render(
            &mut renderer,
            Transform::Normal,
            1.,
            position,
            true,
            buffer.clone(),
            target,
            true,
        )?;
        anyhow::ensure!(native == captured, "blocked target reused unblocked pixels");
    }
    let output_again = render(
        &mut renderer,
        Transform::Normal,
        1.,
        position,
        true,
        buffer.clone(),
        RenderTarget::Output,
        false,
    )?;
    anyhow::ensure!(output == output_again, "capture contaminated output target");
    drop(buffer);
    anyhow::ensure!(
        weak.upgrade().is_none(),
        "projection cache retained after its owner dropped"
    );
    println!("PASS: target isolation and projection resource owner release");
    println!("PASS: {count} GPU transform / DPI / clipped-edge native framebuffer comparisons");
    Ok(())
}
