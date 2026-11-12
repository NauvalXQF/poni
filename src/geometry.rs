use serde::{Deserialize, Serialize};

pub const WINDOW_WIDTH: f32 = 500.0;
pub const WINDOW_HEIGHT: f32 = 230.0;

pub const COLLAPSED_WIDTH: f32 = 200.0;
pub const COLLAPSED_HEIGHT: f32 = 34.0;
pub const EXPANDED_WIDTH: f32 = 380.0;
pub const EXPANDED_HEIGHT: f32 = 140.0;
pub const NOTICE_WIDTH: f32 = 320.0;
pub const NOTICE_HEIGHT: f32 = 56.0;

pub const COLLAPSED_RADIUS: f32 = 17.0;
pub const EXPANDED_RADIUS: f32 = 38.0;
pub const NOTICE_RADIUS: f32 = NOTICE_HEIGHT / 2.0;

/// How far the attached lip reaches past the body on each side, in logical pixels.
pub const NOTCH_FLARE: f32 = 10.0;
/// How far the attached shoulder drops from the top edge, in logical pixels.
pub const NOTCH_SHOULDER: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeStyle {
    Attached,
    Floating,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self::Floating
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub top_radius: f32,
    pub bottom_radius: f32,
    pub flare: f32,
    pub shoulder: f32,
}

#[allow(dead_code)]
pub fn geometry(progress: f32, scale: f32, window_width: f32) -> Geometry {
    geometry_with_notice(progress, 0.0, ShapeStyle::Floating, scale, window_width)
}

pub fn geometry_with_notice(
    progress: f32,
    notice_mix: f32,
    style: ShapeStyle,
    scale: f32,
    window_width: f32,
) -> Geometry {
    let progress = progress.clamp(0.0, 1.0);
    let notice_mix = notice_mix.clamp(0.0, 1.0);

    let width = lerp(
        lerp(COLLAPSED_WIDTH, EXPANDED_WIDTH, progress),
        NOTICE_WIDTH,
        notice_mix,
    ) * scale;
    let height = lerp(
        lerp(COLLAPSED_HEIGHT, EXPANDED_HEIGHT, progress),
        NOTICE_HEIGHT,
        notice_mix,
    ) * scale;
    let radius = lerp(
        lerp(COLLAPSED_RADIUS, EXPANDED_RADIUS, progress),
        NOTICE_RADIUS,
        notice_mix,
    ) * scale;

    let (y, top_radius, flare, shoulder) = match style {
        ShapeStyle::Attached => (0.0, 0.0, NOTCH_FLARE * scale, NOTCH_SHOULDER * scale),
        ShapeStyle::Floating => (10.0 * scale, radius, 0.0, 0.0),
    };

    Geometry {
        x: (window_width - width) / 2.0,
        y,
        width,
        height,
        top_radius,
        bottom_radius: radius,
        flare,
        shoulder,
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_capsule_floats_below_the_top_edge() {
        let geo = geometry(0.0, 1.0, 500.0);
        assert!(geo.y > 0.0);
        assert!(geo.top_radius > 0.0);
        assert_eq!(geo.flare, 0.0);
    }

    #[test]
    fn attached_shape_sits_flush_against_the_top_edge() {
        let geo = geometry_with_notice(0.0, 0.0, ShapeStyle::Attached, 1.0, 500.0);
        assert_eq!(geo.y, 0.0);
        assert_eq!(geo.top_radius, 0.0);
        assert!(geo.flare > 0.0);
        assert!(geo.shoulder > 0.0);
    }

    #[test]
    fn compact_capsule_is_pill_shaped() {
        let geo = geometry(0.0, 1.0, 500.0);
        assert!((geo.bottom_radius - geo.height / 2.0).abs() < 0.01);
    }

    #[test]
    fn notice_has_a_capsule_bottom() {
        let geo = geometry_with_notice(0.0, 1.0, ShapeStyle::Floating, 1.0, 500.0);
        assert!((geo.bottom_radius - geo.height / 2.0).abs() < 0.01);
    }

    #[test]
    fn attached_notice_stays_attached() {
        let geo = geometry_with_notice(0.0, 1.0, ShapeStyle::Attached, 1.0, 500.0);
        assert_eq!(geo.y, 0.0);
        assert_eq!(geo.top_radius, 0.0);
        assert!(geo.flare > 0.0);
    }

    #[test]
    fn expanded_geometry_is_larger() {
        for style in [ShapeStyle::Attached, ShapeStyle::Floating] {
            let collapsed = geometry_with_notice(0.0, 0.0, style, 1.75, 875.0);
            let expanded = geometry_with_notice(1.0, 0.0, style, 1.75, 875.0);
            assert!(expanded.width > collapsed.width);
            assert!(expanded.height > collapsed.height);
            assert!(expanded.bottom_radius > collapsed.bottom_radius);
        }
    }

    #[test]
    fn geometry_stays_centered() {
        let geo = geometry_with_notice(0.42, 0.31, ShapeStyle::Floating, 1.25, 500.0);
        assert!((geo.x + geo.width / 2.0 - 250.0).abs() < 0.01);
    }

    #[test]
    fn floating_radius_transition_is_monotonic() {
        let compact = geometry(0.0, 1.0, 500.0);
        let middle = geometry(0.5, 1.0, 500.0);
        let expanded = geometry(1.0, 1.0, 500.0);
        assert!(compact.bottom_radius < middle.bottom_radius);
        assert!(middle.bottom_radius < expanded.bottom_radius);
    }
}
