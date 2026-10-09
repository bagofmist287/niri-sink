//! Runtime Sink configuration. No renderer or window ownership lives in the parser.
use knuffel::errors::DecodeError;

use crate::utils::{FloatOrInt, MergeWith};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkLevel {
    pub scale: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sink {
    pub enabled: bool,
    pub visible_depth: usize,
    pub levels: [SinkLevel; 4],
}

impl Default for Sink {
    fn default() -> Self {
        Self {
            enabled: false,
            visible_depth: 2,
            levels: std::array::from_fn(|depth| SinkLevel {
                scale: 0.93 * (0.85_f64 / 0.93).powi(depth as i32),
                opacity: 0.82 * (0.45_f64 / 0.82).powi(depth as i32),
            }),
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, PartialEq)]
struct RawSinkPart {
    #[knuffel(child, unwrap(argument))]
    enabled: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    visible_depth: Option<u8>,
    #[knuffel(children(name = "level"))]
    levels: Vec<LevelPart>,
}

#[derive(knuffel::Decode, Debug, PartialEq)]
struct LevelPart {
    #[knuffel(argument)]
    depth: u8,
    #[knuffel(property)]
    scale: Option<FloatOrInt<0, 1>>,
    #[knuffel(property)]
    opacity: Option<FloatOrInt<0, 1>>,
}

#[derive(Debug, PartialEq)]
pub struct SinkPart(RawSinkPart);

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for SinkPart {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let part = RawSinkPart::decode_node(node, ctx)?;
        if part.visible_depth.is_some_and(|d| !(1..=4).contains(&d)) {
            ctx.emit_error(DecodeError::conversion(
                node,
                "visible-depth must be between 1 and 4",
            ));
        }
        let mut seen = [false; 4];
        for level in &part.levels {
            if level.depth > 3 {
                ctx.emit_error(DecodeError::conversion(
                    node,
                    "level index must be between 0 and 3",
                ));
            } else if std::mem::replace(&mut seen[level.depth as usize], true) {
                ctx.emit_error(DecodeError::conversion(node, "duplicate Sink level index"));
            }
            if level.scale.is_some_and(|s| s.0 <= 0.) {
                ctx.emit_error(DecodeError::conversion(
                    node,
                    "scale must be greater than zero",
                ));
            }
        }
        Ok(Self(part))
    }
}

impl MergeWith<SinkPart> for Sink {
    fn merge_with(&mut self, part: &SinkPart) {
        let part = &part.0;
        if let Some(enabled) = part.enabled {
            self.enabled = enabled;
        }
        if let Some(depth) = part.visible_depth {
            self.visible_depth = usize::from(depth).clamp(1, 4);
        }
        for level in &part.levels {
            let Some(target) = self.levels.get_mut(level.depth as usize) else {
                continue;
            };
            if let Some(scale) = level.scale {
                target.scale = scale.0;
            }
            if let Some(opacity) = level.opacity {
                target.opacity = opacity.0;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewSink {
    pub animate: bool,
    pub entry_height: f64,
    pub tail_height: f64,
    pub tail_decay: f64,
}

impl Default for OverviewSink {
    fn default() -> Self {
        Self {
            animate: true,
            entry_height: 24.,
            tail_height: 12.,
            tail_decay: 0.5,
        }
    }
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
struct RawOverviewSinkPart {
    #[knuffel(child, unwrap(argument))]
    animate: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    entry_height: Option<FloatOrInt<1, 256>>,
    #[knuffel(child, unwrap(argument))]
    tail_height: Option<FloatOrInt<0, 256>>,
    #[knuffel(child, unwrap(argument))]
    tail_decay: Option<FloatOrInt<0, 1>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewSinkPart(RawOverviewSinkPart);

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for OverviewSinkPart {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let part = RawOverviewSinkPart::decode_node(node, ctx)?;
        if part
            .tail_decay
            .is_some_and(|q| !(0.01..=0.99).contains(&q.0))
        {
            ctx.emit_error(DecodeError::conversion(
                node,
                "tail-decay must be between 0.01 and 0.99",
            ));
        }
        Ok(Self(part))
    }
}

impl MergeWith<OverviewSinkPart> for OverviewSink {
    fn merge_with(&mut self, part: &OverviewSinkPart) {
        let part = &part.0;
        merge_clone!((self, part), animate);
        merge!((self, part), entry_height, tail_height, tail_decay);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Action, Config};

    #[test]
    fn sink_defaults_overrides_and_actions() {
        let defaults = Config::default();
        assert!(!defaults.sink.enabled);
        assert_eq!(defaults.sink.visible_depth, 2);
        let config = Config::parse_mem(
            r#"
            sink {
                enabled true
                visible-depth 4
                level 0 scale=0.9 opacity=0.7
            }
            overview {
                sink {
                    animate false
                    entry-height 30
                    tail-decay 0.9
                }
            }
            binds {
                Mod+S { sink-window; }
                Mod+P { pull-window; }
            }
        "#,
        )
        .unwrap();
        assert!(config.sink.enabled);
        assert_eq!(config.sink.levels[0].scale, 0.9);
        assert_eq!(config.sink.levels[1], defaults.sink.levels[1]);
        assert!(!config.overview.sink.animate);
        assert_eq!(config.overview.sink.entry_height, 30.);
        assert!(config
            .binds
            .0
            .iter()
            .any(|b| b.action == Action::SinkWindow));
        assert!(config
            .binds
            .0
            .iter()
            .any(|b| b.action == Action::PullWindow));
    }

    #[test]
    fn sink_rejects_invalid_parameters_and_removed_options() {
        for source in [
            "sink { visible-depth 0; }",
            "sink { visible-depth 5; }",
            "sink { level 4 scale=0.5; }",
            "sink { level 0; level 0; }",
            "sink { level 0 scale=0; }",
            "sink { level 0 scale=1.1; }",
            "sink { level 0 opacity=-0.1; }",
            "sink { level 0 opacity=2; }",
            "sink { level 0 scale=\"NaN\"; }",
            "sink { self-blur true; }",
            "overview { sink { mode \"none\"; }; }",
            "overview { sink { animate \"yes\"; }; }",
            "overview { sink { entry-height 0; }; }",
            "overview { sink { tail-height 257; }; }",
            "overview { sink { tail-decay 0; }; }",
            "overview { sink { tail-decay 1; }; }",
        ] {
            assert!(Config::parse_mem(source).is_err(), "accepted {source}");
        }
    }
}
