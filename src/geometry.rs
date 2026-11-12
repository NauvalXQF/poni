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

#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
}

#[allow(dead_code)]
pub fn geometry(progress: f32, scale: f32, window_width: f32) -> Geometry {
    geometry_with_notice(progress, 0.0, scale, window_width)
}

pub fn geometry_with_notice(
    progress: f32,
    notice_mix: f32,
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

    Geometry {
        x: (window_width - width) / 2.0,
        y: 10.0 * scale,
        width,
        height,
        radius: radius.min(width / 2.0).min(height / 2.0),
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_floats_below_the_top_edge() {
        let geo = geometry(0.0, 1.0, 500.0);
        assert!(geo.y > 0.0);
        assert!(geo.radius > 0.0);
        assert!(geo.width < 500.0);
    }

    #[test]
    fn compact_capsule_is_pill_shaped() {
        let geo = geometry(0.0, 1.0, 500.0);
        assert!((geo.radius - geo.height / 2.0).abs() < 0.01);
    }

    #[test]
    fn notice_is_a_full_capsule() {
        let geo = geometry_with_notice(0.0, 1.0, 1.0, 500.0);
        assert!((geo.radius - geo.height / 2.0).abs() < 0.01);
    }

    #[test]
    fn expanded_geometry_is_larger() {
        let collapsed = geometry(0.0, 1.75, 875.0);
        let expanded = geometry(1.0, 1.75, 875.0);
        assert!(expanded.width > collapsed.width);
        assert!(expanded.height > collapsed.height);
        assert!(expanded.radius > collapsed.radius);
    }

    #[test]
    fn geometry_stays_centered() {
        let geo = geometry_with_notice(0.42, 0.31, 1.25, 500.0);
        assert!((geo.x + geo.width / 2.0 - 250.0).abs() < 0.01);
    }

    #[test]
    fn radius_transition_is_monotonic() {
        let compact = geometry(0.0, 1.0, 500.0);
        let middle = geometry(0.5, 1.0, 500.0);
        let expanded = geometry(1.0, 1.0, 500.0);
        assert!(compact.radius < middle.radius);
        assert!(middle.radius < expanded.radius);
    }
}
