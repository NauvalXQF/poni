use serde::{Deserialize, Serialize};

pub const WINDOW_WIDTH: f32 = 500.0;
pub const WINDOW_HEIGHT: f32 = 230.0;

pub const COLLAPSED_WIDTH: f32 = 220.0;
pub const COLLAPSED_HEIGHT: f32 = 34.0;
pub const EXPANDED_WIDTH: f32 = 430.0;
pub const EXPANDED_HEIGHT: f32 = 176.0;
pub const NOTICE_WIDTH: f32 = 330.0;
pub const NOTICE_HEIGHT: f32 = 62.0;

/// How far the notch lip reaches past the body on each side, in logical pixels.
pub const NOTCH_FLARE: f32 = 20.0;
/// How far the notch shoulder drops from the top edge, in logical pixels.
pub const NOTCH_SHOULDER: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandMode {
    Notch,
    Pill,
}

impl Default for IslandMode {
    fn default() -> Self {
        Self::Notch
    }
}

impl IslandMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Notch => "NOTCH",
            Self::Pill => "PILL",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Notch => "Attached notch",
            Self::Pill => "Floating pill",
        }
    }

    pub fn mix(self) -> f32 {
        match self {
            Self::Notch => 0.0,
            Self::Pill => 1.0,
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Notch => Self::Pill,
            Self::Pill => Self::Notch,
        }
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
    pub notch_flare: f32,
    pub notch_shoulder: f32,
}

#[allow(dead_code)]
pub fn geometry(mode_mix: f32, progress: f32, scale: f32, window_width: f32) -> Geometry {
    geometry_with_notice(mode_mix, progress, 0.0, scale, window_width)
}

pub fn geometry_with_notice(
    mode_mix: f32,
    progress: f32,
    notice_mix: f32,
    scale: f32,
    window_width: f32,
) -> Geometry {
    let mode_mix = mode_mix.clamp(0.0, 1.0);
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
    let x = (window_width - width) / 2.0;
    let y = lerp(0.0, 10.0, mode_mix) * scale;
    let pill_radius = lerp(17.0, 26.0, progress) * scale;
    let top_radius = lerp(0.0, pill_radius, mode_mix);
    let bottom_radius = pill_radius;
    let notch_mix = 1.0 - mode_mix;
    let notch_flare = NOTCH_FLARE * notch_mix * scale;
    let notch_shoulder = NOTCH_SHOULDER * notch_mix * scale;

    Geometry {
        x,
        y,
        width,
        height,
        top_radius: top_radius.min(height / 2.0),
        bottom_radius: bottom_radius.min(height / 2.0),
        notch_flare,
        notch_shoulder,
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notch_is_attached_to_the_top_edge() {
        let geo = geometry(0.0, 0.0, 1.0, 500.0);
        assert_eq!(geo.y, 0.0);
        assert_eq!(geo.top_radius, 0.0);
        assert!(geo.notch_flare > 0.0);
        assert!(geo.width < 500.0);
    }

    #[test]
    fn pill_floats_below_the_top_edge() {
        let geo = geometry(1.0, 0.0, 1.0, 500.0);
        assert!(geo.y > 0.0);
        assert!(geo.top_radius > 0.0);
        assert_eq!(geo.notch_flare, 0.0);
    }

    #[test]
    fn expanded_geometry_is_larger() {
        let collapsed = geometry(0.0, 0.0, 1.75, 875.0);
        let expanded = geometry(0.0, 1.0, 1.75, 875.0);
        assert!(expanded.width > collapsed.width);
        assert!(expanded.height > collapsed.height);
    }

    #[test]
    fn pill_corners_are_symmetric() {
        let geo = geometry(1.0, 1.0, 1.0, 500.0);
        assert!((geo.top_radius - geo.bottom_radius).abs() < 0.01);
    }

    #[test]
    fn mode_transition_is_monotonic() {
        let notch = geometry(0.0, 0.0, 1.0, 500.0);
        let middle = geometry(0.5, 0.0, 1.0, 500.0);
        let pill = geometry(1.0, 0.0, 1.0, 500.0);
        assert!(notch.notch_flare > middle.notch_flare);
        assert!(middle.notch_flare > pill.notch_flare);
        assert!(notch.top_radius < middle.top_radius);
        assert!(middle.top_radius < pill.top_radius);
        assert!(notch.y < middle.y);
        assert!(middle.y < pill.y);
    }

    #[test]
    fn mode_toggles_between_notch_and_pill() {
        assert_eq!(IslandMode::Notch.toggle(), IslandMode::Pill);
        assert_eq!(IslandMode::Pill.toggle(), IslandMode::Notch);
    }
}
