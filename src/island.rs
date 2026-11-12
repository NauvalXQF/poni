use fontdue::Font;
use tiny_skia::{Color, ColorU8, FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::geometry::{lerp, Geometry, IslandMode};

pub struct IslandView<'a> {
    pub mode: IslandMode,
    pub mode_mix: f32,
    pub progress: f32,
    pub geometry: Geometry,
    pub scale: f32,
    pub pinned: bool,
    pub time: String,
    pub font: Option<&'a Font>,
}

pub fn draw(width: u32, height: u32, view: &IslandView<'_>) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let geo = view.geometry;

    if view.mode_mix > 0.01 {
        let shadow = rounded_rect_path(
            geo.x - 1.0 * view.scale,
            geo.y + 4.0 * view.scale,
            geo.width + 2.0 * view.scale,
            geo.height,
            geo.top_radius,
            geo.bottom_radius,
        );
        if let Some(path) = shadow {
            let mut paint = Paint::default();
            paint.set_color_rgba8(0, 0, 0, (70.0 * view.mode_mix) as u8);
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    let path = if view.mode == IslandMode::Notch {
        notch_path(&geo)
    } else {
        rounded_rect_path(
            geo.x,
            geo.y,
            geo.width,
            geo.height,
            geo.top_radius,
            geo.bottom_radius,
        )
    };
    let Some(path) = path else {
        return Some(pixmap);
    };

    let mut fill = Paint::default();
    let bg = blend_color(
        ColorU8::from_rgba(0, 0, 0, 255),
        ColorU8::from_rgba(8, 9, 12, 252),
        view.mode_mix,
    );
    fill.set_color(Color::from_rgba8(
        bg.red(),
        bg.green(),
        bg.blue(),
        bg.alpha(),
    ));
    pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);

    if view.mode_mix > 0.02 {
        let mut border = Paint::default();
        border.set_color_rgba8(255, 178, 36, (38.0 * view.mode_mix) as u8);
        let stroke = Stroke {
            width: 1.0 * view.scale,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &border, &stroke, Transform::identity(), None);
    }

    if let Some(font) = view.font {
        draw_content(&mut pixmap, view, font);
    }

    Some(pixmap)
}

fn draw_content(pixmap: &mut Pixmap, view: &IslandView<'_>, font: &Font) {
    let geo = view.geometry;
    let s = view.scale;
    let accent = ColorU8::from_rgba(255, 178, 36, 255);
    let cream = ColorU8::from_rgba(245, 241, 232, 245);
    let muted = ColorU8::from_rgba(190, 184, 174, 185);

    let dot_x = geo.x + 18.0 * s;
    let dot_y = geo.y + geo.height / 2.0;
    draw_circle(pixmap, dot_x, dot_y, 4.0 * s, accent);

    if view.progress < 0.45 {
        let baseline = dot_y + 4.5 * s;
        draw_text(
            pixmap,
            font,
            "PONI",
            geo.x + 30.0 * s,
            baseline,
            10.5 * s,
            muted,
        );
        let time_width = measure_text(font, &view.time, 13.0 * s);
        draw_text(
            pixmap,
            font,
            &view.time,
            geo.x + geo.width - 18.0 * s - time_width,
            baseline,
            13.0 * s,
            cream,
        );
        return;
    }

    let heading_baseline = geo.y + 34.0 * s;
    draw_text(
        pixmap,
        font,
        "Poni",
        geo.x + 43.0 * s,
        heading_baseline,
        20.0 * s,
        cream,
    );

    let mode_label = view.mode.label();
    let mode_width = measure_text(font, mode_label, 10.0 * s);
    draw_text(
        pixmap,
        font,
        mode_label,
        geo.x + geo.width - 23.0 * s - mode_width,
        geo.y + 31.0 * s,
        10.0 * s,
        accent,
    );

    let subtitle = match view.mode {
        IslandMode::Notch => "Attached notch",
        IslandMode::Pill => "Floating pill",
    };
    draw_text(
        pixmap,
        font,
        subtitle,
        geo.x + 43.0 * s,
        geo.y + 58.0 * s,
        11.5 * s,
        muted,
    );

    let divider_y = geo.y + 78.0 * s;
    draw_line(
        pixmap,
        geo.x + 23.0 * s,
        divider_y,
        geo.x + geo.width - 23.0 * s,
        divider_y,
        ColorU8::from_rgba(255, 178, 36, 28),
        1.0 * s,
    );

    draw_text(
        pixmap,
        font,
        "Two shapes. One quiet island.",
        geo.x + 24.0 * s,
        geo.y + 108.0 * s,
        12.5 * s,
        cream,
    );

    let hint = if view.pinned {
        "Pinned. Left-click to release."
    } else {
        "Hover · right-click to switch shape"
    };
    draw_text(
        pixmap,
        font,
        hint,
        geo.x + 24.0 * s,
        geo.y + 134.0 * s,
        10.5 * s,
        muted,
    );

    draw_text(
        pixmap,
        font,
        "Ctrl+Alt+N  shape   ·   Ctrl+Alt+Q  quit",
        geo.x + 24.0 * s,
        geo.y + 154.0 * s,
        9.5 * s,
        muted,
    );
}

fn notch_path(geo: &Geometry) -> Option<Path> {
    let x = geo.x;
    let y = geo.y;
    let width = geo.width;
    let height = geo.height;
    let flare = geo.notch_flare;
    let shoulder = geo.notch_shoulder;
    let radius = geo.bottom_radius.min(width / 2.0).min(height / 2.0);
    let mut pb = PathBuilder::new();

    pb.move_to(x - flare, y);
    pb.line_to(x + width + flare, y);
    pb.cubic_to(
        x + width + flare,
        y + shoulder * 0.56,
        x + width,
        y + shoulder * 0.18,
        x + width,
        y + shoulder,
    );
    pb.line_to(x + width, y + height - radius);
    pb.quad_to(x + width, y + height, x + width - radius, y + height);
    pb.line_to(x + radius, y + height);
    pb.quad_to(x, y + height, x, y + height - radius);
    pb.line_to(x, y + shoulder);
    pb.cubic_to(
        x,
        y + shoulder * 0.18,
        x - flare,
        y + shoulder * 0.56,
        x - flare,
        y,
    );
    pb.close();
    pb.finish()
}

fn rounded_rect_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    top_radius: f32,
    bottom_radius: f32,
) -> Option<Path> {
    let top_radius = top_radius.min(width / 2.0).min(height / 2.0);
    let bottom_radius = bottom_radius.min(width / 2.0).min(height / 2.0);
    let mut pb = PathBuilder::new();

    pb.move_to(x + top_radius, y);
    pb.line_to(x + width - top_radius, y);
    if top_radius > 0.01 {
        pb.quad_to(x + width, y, x + width, y + top_radius);
    } else {
        pb.line_to(x + width, y);
    }
    pb.line_to(x + width, y + height - bottom_radius);
    if bottom_radius > 0.01 {
        pb.quad_to(x + width, y + height, x + width - bottom_radius, y + height);
    } else {
        pb.line_to(x + width, y + height);
    }
    pb.line_to(x + bottom_radius, y + height);
    if bottom_radius > 0.01 {
        pb.quad_to(x, y + height, x, y + height - bottom_radius);
    } else {
        pb.line_to(x, y + height);
    }
    pb.line_to(x, y + top_radius);
    if top_radius > 0.01 {
        pb.quad_to(x, y, x + top_radius, y);
    }
    pb.close();
    pb.finish()
}

fn draw_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, radius: f32, color: ColorU8) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, radius);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        ));
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn draw_line(pixmap: &mut Pixmap, x1: f32, y1: f32, x2: f32, y2: f32, color: ColorU8, width: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        ));
        let stroke = Stroke {
            width,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

pub fn measure_text(font: &Font, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|ch| font.metrics(ch, size).advance_width)
        .sum()
}

fn draw_text(
    pixmap: &mut Pixmap,
    font: &Font,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    color: ColorU8,
) {
    let mut pen_x = x;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size);
        let glyph_x = (pen_x + metrics.xmin as f32).round() as i32;
        let glyph_y = (baseline - metrics.height as f32 - metrics.ymin as f32).round() as i32;
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let coverage = bitmap[row * metrics.width + col];
                if coverage == 0 {
                    continue;
                }
                blend_pixel(
                    pixmap,
                    glyph_x + col as i32,
                    glyph_y + row as i32,
                    color,
                    coverage,
                );
            }
        }
        pen_x += metrics.advance_width;
    }
}

fn blend_pixel(pixmap: &mut Pixmap, x: i32, y: i32, color: ColorU8, coverage: u8) {
    if x < 0 || y < 0 || x >= pixmap.width() as i32 || y >= pixmap.height() as i32 {
        return;
    }
    let width = pixmap.width() as usize;
    let index = (y as usize * width + x as usize) * 4;
    let data = pixmap.data_mut();
    let src_a = (coverage as u32 * color.alpha() as u32) / 255;
    if src_a == 0 {
        return;
    }
    let inv = 255 - src_a;
    let src_r = (color.red() as u32 * src_a) / 255;
    let src_g = (color.green() as u32 * src_a) / 255;
    let src_b = (color.blue() as u32 * src_a) / 255;

    data[index] = (src_r + (data[index] as u32 * inv) / 255) as u8;
    data[index + 1] = (src_g + (data[index + 1] as u32 * inv) / 255) as u8;
    data[index + 2] = (src_b + (data[index + 2] as u32 * inv) / 255) as u8;
    data[index + 3] = (src_a + (data[index + 3] as u32 * inv) / 255) as u8;
}

fn blend_color(a: ColorU8, b: ColorU8, t: f32) -> ColorU8 {
    let t = t.clamp(0.0, 1.0);
    ColorU8::from_rgba(
        lerp(a.red() as f32, b.red() as f32, t) as u8,
        lerp(a.green() as f32, b.green() as f32, t) as u8,
        lerp(a.blue() as f32, b.blue() as f32, t) as u8,
        lerp(a.alpha() as f32, b.alpha() as f32, t) as u8,
    )
}
