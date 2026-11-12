use serde::{Deserialize, Serialize};

pub const WINDOW_WIDTH: f32 = 500.0;
pub const WINDOW_HEIGHT: f32 = 230.0;

pub const COLLAPSED_WIDTH: f32 = 220.0;
pub const COLLAPSED_HEIGHT: f32 = 34.0;
pub const EXPANDED_WIDTH: f32 = 430.0;
pub const EXPANDED_HEIGHT: f32 = 176.0;

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

pub fn geometry(mode_mix: f32, progress: f32, scale: f32, window_width: f32) -> Geometry {
    let mode_mix = mode_mix.clamp(0.0, 1.0);
    let progress = progress.clamp(0.0, 1.0);

    let width = lerp(COLLAPSED_WIDTH, EXPANDED_WIDTH, progress) * scale;
    let height = lerp(COLLAPSED_HEIGHT, EXPANDED_HEIGHT, progress) * scale;
    let x = (window_width - width) / 2.0;
    let y = lerp(0.0, 10.0, mode_mix) * scale;
    let top_radius = lerp(0.0, 17.0, mode_mix) * scale;
    let bottom_radius = lerp(17.0, 26.0, progress) * scale;
    let notch_mix = 1.0 - mode_mix;
    let notch_flare = 12.0 * notch_mix * scale;
    let notch_shoulder = 9.0 * notch_mix * scale;

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

impl Geometry {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        if self.notch_flare > 0.01
            && py >= self.y
            && py <= self.y + self.notch_shoulder
            && px >= self.x - self.notch_flare
            && px <= self.x + self.width + self.notch_flare
        {
            return true;
        }

        if px < self.x || px > self.x + self.width || py < self.y || py > self.y + self.height {
            return false;
        }

        let local_x = px - self.x;
        let local_y = py - self.y;
        let tr = self.top_radius;
        let br = self.bottom_radius;

        if local_y < tr {
            if local_x < tr {
                return circle_contains(local_x, local_y, tr, tr, tr);
            }
            if local_x > self.width - tr {
                return circle_contains(local_x, local_y, self.width - tr, tr, tr);
            }
        }

        if local_y > self.height - br {
            if local_x < br {
                return circle_contains(local_x, local_y, br, self.height - br, br);
            }
            if local_x > self.width - br {
                return circle_contains(local_x, local_y, self.width - br, self.height - br, br);
            }
        }

        true
    }
}

fn circle_contains(px: f32, py: f32, cx: f32, cy: f32, radius: f32) -> bool {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= radius * radius
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
        assert!(geo.contains(geo.x - geo.notch_flare + 1.0, 1.0));
    }

    #[test]
    fn pill_floats_below_the_top_edge() {
        let geo = geometry(1.0, 0.0, 1.0, 500.0);
        assert!(geo.y > 0.0);
        assert!(geo.top_radius > 0.0);
    }

    #[test]
    fn expanded_geometry_is_larger() {
        let collapsed = geometry(0.0, 0.0, 1.75, 875.0);
        let expanded = geometry(0.0, 1.0, 1.75, 875.0);
        assert!(expanded.width > collapsed.width);
        assert!(expanded.height > collapsed.height);
    }

    #[test]
    fn hit_test_respects_the_rounded_shape() {
        let geo = geometry(1.0, 0.0, 1.0, 500.0);
        assert!(geo.contains(250.0, 20.0));
        assert!(!geo.contains(10.0, 20.0));
        assert!(!geo.contains(250.0, 100.0));
    }

    #[test]
    fn mode_toggles_between_notch_and_pill() {
        assert_eq!(IslandMode::Notch.toggle(), IslandMode::Pill);
        assert_eq!(IslandMode::Pill.toggle(), IslandMode::Notch);
    }
}
