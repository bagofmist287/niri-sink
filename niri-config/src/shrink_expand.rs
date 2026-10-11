//! Shrink / Expand configuration and live layout paint selection.
use knuffel::errors::DecodeError;

use crate::utils::{FloatOrInt, MergeWith};
use crate::{Border, Color, FocusRing, Gradient};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShrinkExpand {
    pub enabled: bool,
    pub thickness: f64,
    pub shrink_buffer_ms: u32,
    pub expand_buffer_ms: u32,
    pub mouse_zone_px: f64,
    pub preview_scale: f64,
    pub animate: bool,
}

impl Default for ShrinkExpand {
    fn default() -> Self {
        Self {
            enabled: false,
            thickness: 32.,
            shrink_buffer_ms: 700,
            expand_buffer_ms: 1000,
            mouse_zone_px: 24.,
            preview_scale: 0.75,
            animate: true,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, PartialEq)]
struct RawShrinkExpandPart {
    #[knuffel(child, unwrap(argument))]
    enabled: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    thickness: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child, unwrap(argument))]
    shrink_buffer_ms: Option<u32>,
    #[knuffel(child, unwrap(argument))]
    expand_buffer_ms: Option<u32>,
    #[knuffel(child, unwrap(argument))]
    mouse_zone_px: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child, unwrap(argument))]
    preview_scale: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    animate: Option<bool>,
}

#[derive(Debug, PartialEq)]
pub struct ShrinkExpandPart(RawShrinkExpandPart);

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for ShrinkExpandPart {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let part = RawShrinkExpandPart::decode_node(node, ctx)?;
        for (name, value) in [
            ("thickness", part.thickness.map(|v| v.0)),
            ("preview-scale", part.preview_scale.map(|v| v.0)),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v <= 0.) {
                ctx.emit_error(DecodeError::conversion(
                    node,
                    format!("{name} must be finite and greater than zero"),
                ));
            }
        }
        for (name, value) in [
            ("shrink-buffer-ms", part.shrink_buffer_ms),
            ("expand-buffer-ms", part.expand_buffer_ms),
        ] {
            if value.is_some_and(|v| v > 60000) {
                ctx.emit_error(DecodeError::conversion(
                    node,
                    format!("{name} must be between 0 and 60000"),
                ));
            }
        }
        Ok(Self(part))
    }
}

impl MergeWith<ShrinkExpandPart> for ShrinkExpand {
    fn merge_with(&mut self, part: &ShrinkExpandPart) {
        let part = &part.0;
        merge_clone!(
            (self, part),
            enabled,
            shrink_buffer_ms,
            expand_buffer_ms,
            animate
        );
        merge!((self, part), thickness, mouse_zone_px, preview_scale);
    }
}

/// Inherit remains symbolic so a later theme or window rule can change its source.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum SpinePaint {
    #[default]
    Inherit,
    Solid(Color),
    Gradient(Gradient),
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SpineAppearance {
    pub active: SpinePaint,
    pub inactive: SpinePaint,
    pub urgent: SpinePaint,
    pub text_color: Option<Color>,
}

impl SpineAppearance {
    /// Call with the window's effective decorations, after layout and window-rule merge.
    /// Visual selection is independent of actual client activation.
    pub fn resolve(
        &self,
        active: bool,
        urgent: bool,
        focus_ring: FocusRing,
        border: Border,
    ) -> Gradient {
        let source = if focus_ring.off {
            FocusRing::from(border)
        } else {
            focus_ring
        };
        let (paint, color, gradient) = if urgent {
            (self.urgent, source.urgent_color, source.urgent_gradient)
        } else if active {
            (self.active, source.active_color, source.active_gradient)
        } else {
            (
                self.inactive,
                source.inactive_color,
                source.inactive_gradient,
            )
        };
        match paint {
            SpinePaint::Inherit => gradient.unwrap_or_else(|| color.into()),
            SpinePaint::Solid(color) => color.into(),
            SpinePaint::Gradient(gradient) => gradient,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct SpineAppearancePart {
    #[knuffel(child)]
    active_color: Option<ColorOverride>,
    #[knuffel(child)]
    inactive_color: Option<ColorOverride>,
    #[knuffel(child)]
    urgent_color: Option<ColorOverride>,
    #[knuffel(child)]
    active_gradient: Option<Gradient>,
    #[knuffel(child)]
    inactive_gradient: Option<Gradient>,
    #[knuffel(child)]
    urgent_gradient: Option<Gradient>,
    #[knuffel(child)]
    text_color: Option<ColorOverride>,
}

impl MergeWith<SpineAppearancePart> for SpineAppearance {
    fn merge_with(&mut self, part: &SpineAppearancePart) {
        for (paint, color, gradient) in [
            (&mut self.active, part.active_color, part.active_gradient),
            (
                &mut self.inactive,
                part.inactive_color,
                part.inactive_gradient,
            ),
            (&mut self.urgent, part.urgent_color, part.urgent_gradient),
        ] {
            // Match the existing border merge: gradient wins within one part;
            // a color in a later part replaces the entire earlier paint.
            if let Some(gradient) = gradient {
                *paint = SpinePaint::Gradient(gradient);
            } else if let Some(color) = color {
                *paint = color.0.map_or(SpinePaint::Inherit, SpinePaint::Solid);
            }
        }
        if let Some(color) = part.text_color {
            self.text_color = color.0;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ColorOverride(Option<Color>);

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for ColorOverride {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        if node.arguments.first().is_some_and(|value| {
            matches!(&*value.literal, knuffel::ast::Literal::String(s) if s.as_ref() == "inherit")
        }) {
            // Reuse derive's checks for extra arguments, properties, children and types.
            #[derive(knuffel::Decode)]
            struct Inherit(#[knuffel(argument)] String);
            let Inherit(value) = Inherit::decode_node(node, ctx)?;
            debug_assert_eq!(value, "inherit");
            Ok(Self(None))
        } else {
            Color::decode_node(node, ctx).map(|color| Self(Some(color)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn defaults_and_partial_behavior_merge() {
        let defaults = Config::load_default();
        assert_eq!(defaults.shrink_expand, ShrinkExpand::default());
        assert!(!defaults.shrink_expand.enabled);
        assert_eq!(defaults.layout.spine, SpineAppearance::default());
        let config = Config::parse_mem(
            r#"
            shrink-expand {
                enabled true
                thickness 35.5
                shrink-buffer-ms 900
                expand-buffer-ms 600
                mouse-zone-px 17.5
                preview-scale 0.5
                animate false
            }
            "#,
        )
        .unwrap();
        let part = knuffel::parse::<Vec<ShrinkExpandPart>>(
            "override.kdl",
            "shrink-expand { shrink-buffer-ms 0; }",
        )
        .unwrap();
        let mut behavior = config.shrink_expand;
        behavior.merge_with(&part[0]);
        assert_eq!(
            behavior,
            ShrinkExpand {
                enabled: true,
                thickness: 35.5,
                shrink_buffer_ms: 0,
                expand_buffer_ms: 600,
                mouse_zone_px: 17.5,
                preview_scale: 0.5,
                animate: false,
            }
        );
        // Zero removes the timed delay, but mouse buffering remains a separate distance.
        assert_eq!(config.sink, defaults.sink);
    }

    #[test]
    fn invalid_behavior_and_unknown_nodes_are_rejected() {
        for option in [
            "thickness 0",
            "thickness -1",
            "thickness 65536",
            "thickness 1e999",
            "thickness \"NaN\"",
            "thickness \"inf\"",
            "preview-scale 0",
            "preview-scale 1.01",
            "preview-scale -0.2",
            "preview-scale 1e999",
            "mouse-zone-px -1",
            "mouse-zone-px 65536",
            "mouse-zone-px 1e999",
            "shrink-buffer-ms -1",
            "shrink-buffer-ms 60001",
            "shrink-buffer-ms 0.5",
            "expand-buffer-ms 60001",
            "enabled \"true\"",
            "animate 1",
            "mouse-buffer-ms 700",
            "depend-on \"horizontal\"",
        ] {
            let source = format!("shrink-expand {{ {option}; }}");
            assert!(Config::parse_mem(&source).is_err(), "accepted {source}");
        }
        Config::parse_mem("shrink-expand { thickness 0.25; preview-scale 1; mouse-zone-px 0; expand-buffer-ms 60000; }").unwrap();
    }

    #[test]
    fn paints_merge_per_state_and_inherit_is_not_transparency() {
        let mut config = Config::parse_mem(
            r##"
            layout {
                spine {
                    active-gradient from="#f008" to="#00f4" angle=45 in="oklab"
                    inactive-color "#12345600"
                    urgent-color 200 10 20 255
                    text-color "#fff"
                }
            }
        "##,
        )
        .unwrap();
        let part = knuffel::parse::<Vec<crate::LayoutPart>>(
            "override.kdl",
            "layout { spine { active-color \"#abc\"; text-color \"inherit\"; }; }",
        )
        .unwrap();
        config.layout.merge_with(&part[0]);
        assert_eq!(
            config.layout.spine.active,
            SpinePaint::Solid("#abc".parse().unwrap())
        );
        assert_eq!(
            config.layout.spine.inactive,
            SpinePaint::Solid("#12345600".parse().unwrap())
        );
        assert_eq!(
            config.layout.spine.urgent,
            SpinePaint::Solid(Color::from_rgba8_unpremul(200, 10, 20, 255))
        );
        assert_eq!(config.layout.spine.text_color, None);
        let part = knuffel::parse::<Vec<SpineAppearancePart>>(
            "test.kdl",
            "spine { active-color \"inherit\"; }",
        )
        .unwrap();
        let mut paint = config.layout.spine;
        paint.merge_with(&part[0]);
        assert_eq!(paint.active, SpinePaint::Inherit);
        assert_eq!(paint.inactive, config.layout.spine.inactive);
        assert_eq!(paint.urgent, config.layout.spine.urgent);
    }

    #[test]
    fn layout_source_is_live_and_urgency_takes_priority() {
        let config = Config::parse_mem(r##"
            layout {
                focus-ring { inactive-gradient from="#f00" to="#00f" in="srgb-linear"; }
                border { off; active-color "#123456"; inactive-color "#654321"; urgent-color "#ff0022"; }
                spine { active-color "#abcdef"; }
            }
        "##).unwrap();
        let layout = &config.layout;
        let paint = layout.spine;
        assert_eq!(
            paint.resolve(false, false, layout.focus_ring, layout.border),
            layout.focus_ring.inactive_gradient.unwrap()
        );
        assert_eq!(
            paint.resolve(true, true, layout.focus_ring, layout.border),
            layout.focus_ring.urgent_color.into()
        );
        let mut ring = layout.focus_ring;
        ring.off = true;
        assert_eq!(
            paint.resolve(false, false, ring, layout.border),
            layout.border.inactive_color.into()
        );
        assert_eq!(
            paint.resolve(true, false, ring, layout.border),
            "#abcdef".parse::<Color>().unwrap().into()
        );
        assert_eq!(
            paint.resolve(true, true, ring, layout.border),
            layout.border.urgent_color.into()
        );
        // A later effective window rule or theme is read now, not snapshotted on Shrink.
        let mut border = layout.border;
        border.inactive_gradient = Some(Gradient {
            angle: 90,
            .."#10203080".parse::<Color>().unwrap().into()
        });
        assert_eq!(
            paint.resolve(false, false, ring, border),
            border.inactive_gradient.unwrap()
        );
    }

    #[test]
    fn include_and_workspace_merge_preserve_live_inheritance() {
        struct TempDir(std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = TempDir(
            std::env::temp_dir().join(format!("niri-spine-config-{}-{unique}", std::process::id())),
        );
        std::fs::create_dir(&dir.0).unwrap();
        std::fs::write(dir.0.join("theme.kdl"), r##"
            shrink-expand { thickness 38; shrink-buffer-ms 850; }
            layout { spine { active-gradient from="#f00" to="#00f"; inactive-color "#333"; text-color "#fff"; }; }
        "##).unwrap();
        let parsed = Config::parse(
            &dir.0.join("config.kdl"),
            r##"
            include "theme.kdl"
            shrink-expand { enabled true; }
            layout { spine { inactive-color "inherit"; }; }
            workspace "test" {
                layout {
                    border { inactive-color "#778899"; }
                    focus-ring { off; }
                    spine { active-color "inherit"; urgent-color "#f0f"; text-color "inherit"; }
                }
            }
        "##,
        );
        assert_eq!(parsed.includes, [dir.0.join("theme.kdl")]);
        let config = parsed.config.unwrap();
        assert!(config.shrink_expand.enabled);
        assert_eq!(config.shrink_expand.thickness, 38.);
        assert_eq!(config.shrink_expand.shrink_buffer_ms, 850);
        assert_eq!(config.layout.spine.inactive, SpinePaint::Inherit);
        assert!(matches!(
            config.layout.spine.active,
            SpinePaint::Gradient(_)
        ));
        let mut layout = config.layout.clone();
        layout.merge_with(&config.workspaces[0].layout.as_ref().unwrap().0);
        assert_eq!(layout.spine.active, SpinePaint::Inherit);
        assert_eq!(layout.spine.text_color, None);
        assert_eq!(
            layout
                .spine
                .resolve(false, false, layout.focus_ring, layout.border),
            "#778899".parse::<Color>().unwrap().into()
        );
        assert_eq!(
            layout
                .spine
                .resolve(true, true, layout.focus_ring, layout.border),
            "#f0f".parse::<Color>().unwrap().into()
        );
        // Workspace merge must not modify the global appearance.
        assert!(matches!(
            config.layout.spine.active,
            SpinePaint::Gradient(_)
        ));
    }

    #[test]
    fn invalid_paint_forms_are_rejected() {
        for option in [
            "inactive-color \"inherit\" \"extra\"",
            "text-color \"inherit\" extra=true",
            "active-color \"inherit\" { unexpected; }",
            "urgent-color (invalid)\"inherit\"",
            "inactive-color \"no-such-color\"",
            "active-gradient from=\"inherit\" to=\"#fff\"",
            "text-color 256 0 0 255",
            "fill-color \"#fff\"",
        ] {
            let source = format!("layout {{ spine {{ {option}; }}; }}");
            assert!(Config::parse_mem(&source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn effective_window_rules_supply_inherited_paint() {
        let config = Config::parse_mem(
            r##"
            layout { focus-ring { inactive-gradient from="#f00" to="#00f"; }; }
            window-rule {
                focus-ring { off; }
                border { inactive-gradient from="#1234" to="#5678" angle=90; }
            }
            window-rule {
                border { inactive-color "#abcdef88"; }
            }
        "##,
        )
        .unwrap();
        let mut ring = config.layout.focus_ring;
        let mut border = config.layout.border;
        let appearance = config.layout.spine;
        ring.merge_with(&config.window_rules[0].focus_ring);
        border.merge_with(&config.window_rules[0].border);
        assert_eq!(
            appearance.resolve(false, false, ring, border),
            border.inactive_gradient.unwrap()
        );
        border.merge_with(&config.window_rules[1].border);
        assert!(border.inactive_gradient.is_none());
        assert_eq!(
            appearance.resolve(false, false, ring, border),
            "#abcdef88".parse::<Color>().unwrap().into()
        );
        // Enabling the effective ring again restores its live gradient source.
        ring.merge_with(&crate::BorderRule {
            on: true,
            ..Default::default()
        });
        assert_eq!(
            appearance.resolve(false, false, ring, border),
            config.layout.focus_ring.inactive_gradient.unwrap()
        );
    }
}
