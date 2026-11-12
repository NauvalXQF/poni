use fontdue::Font;
use tiny_skia::{Color, ColorU8, FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::geometry::{lerp, Geometry, IslandMode};

const CORNER_K: f32 = 0.552_284_8;

pub struct IslandView<'a> {
    pub mode: IslandMode,
    pub mode_mix: f32,
    pub progress: f32,
    pub geometry: Geometry,
    pub scale: f32,
    pub pinned: bool,
    pub time: String,
    pub regular: Option<&'a Font>,
    pub semibold: Option<&'a Font>,
}

pub fn draw(width: u32, height: u32, view: &IslandView<'_>) -> Option<Pixmap> {
    let mut shape = Pixmap::new(width, height)?;
    draw_shape(&mut shape, view);

    // Content is rendered into its own layer so it can be clipped to the
    // shape. Without this, expanded text briefly floats outside the island
    // while the shape is still growing.
    let mut content = Pixmap::new(width, height)?;
    draw_content(&mut content, view);
    clip_to_shape(&mut content, &shape, 128);
    composite_layer(&mut shape, &content, 1.0);

    Some(shape)
}

fn draw_shape(pixmap: &mut Pixmap, view: &IslandView<'_>) {
    let geo = view.geometry;

    if view.mode_mix > 0.01 {
        let mut shadow_geo = geo;
        shadow_geo.x -= 1.0 * view.scale;
        shadow_geo.y += 4.0 * view.scale;
        shadow_geo.width += 2.0 * view.scale;
        if let Some(shadow) = island_path(&shadow_geo, view.mode_mix) {
            let mut paint = Paint::default();
            paint.set_color_rgba8(0, 0, 0, (58.0 * view.mode_mix) as u8);
            pixmap.fill_path(
                &shadow,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    let Some(path) = island_path(&geo, view.mode_mix) else {
        return;
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
}

fn draw_content(pixmap: &mut Pixmap, view: &IslandView<'_>) {
    if view.regular.is_none() && view.semibold.is_none() {
        return;
    }

    let accent = ColorU8::from_rgba(255, 178, 36, 255);
    let cream = ColorU8::from_rgba(245, 241, 232, 245);
    let muted = ColorU8::from_rgba(190, 184, 174, 185);

    let content_mix = smoothstep(0.35, 0.72, view.progress);
    let collapsed_alpha = 1.0 - content_mix;
    let expanded_alpha = content_mix;

    if collapsed_alpha > 0.01 {
        draw_collapsed(pixmap, view, collapsed_alpha, accent, cream, muted);
    }
    if expanded_alpha > 0.01 {
        draw_expanded(pixmap, view, expanded_alpha, accent, cream, muted);
    }
}

fn draw_collapsed(
    pixmap: &mut Pixmap,
    view: &IslandView<'_>,
    alpha: f32,
    accent: ColorU8,
    cream: ColorU8,
    muted: ColorU8,
) {
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);

    let s = view.scale;
    let geo = view.geometry;
    let baseline = geo.y + geo.height / 2.0 + 4.5 * s;

    draw_circle(
        pixmap,
        geo.x + 18.0 * s,
        geo.y + geo.height / 2.0,
        4.0 * s,
        scale_alpha(accent, alpha),
    );
    draw_text(
        pixmap,
        regular,
        "PONI",
        geo.x + 30.0 * s,
        baseline,
        10.5 * s,
        scale_alpha(muted, alpha),
    );

    let time_width = measure_text(semibold, &view.time, 13.0 * s);
    draw_text(
        pixmap,
        semibold,
        &view.time,
        geo.x + geo.width - 18.0 * s - time_width,
        baseline,
        13.0 * s,
        scale_alpha(cream, alpha),
    );
}

fn draw_expanded(
    pixmap: &mut Pixmap,
    view: &IslandView<'_>,
    alpha: f32,
    accent: ColorU8,
    cream: ColorU8,
    muted: ColorU8,
) {
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);

    let s = view.scale;
    let geo = view.geometry;
    let left = geo.x + 24.0 * s;
    let right = geo.x + geo.width - 24.0 * s;

    draw_text(
        pixmap,
        semibold,
        "Poni",
        left,
        geo.y + 36.0 * s,
        20.0 * s,
        scale_alpha(cream, alpha),
    );

    // The target mode is already selected when the toggle happens, so fade
    // the previous label/subtitle out and the new one in as mode_mix settles.
    let target_mix = view.mode.mix();
    let to_target = if target_mix > 0.5 {
        view.mode_mix
    } else {
        1.0 - view.mode_mix
    };
    let start = view.mode.toggle();
    let label_mix = smoothstep(0.28, 0.72, to_target);
    let out_alpha = alpha * (1.0 - label_mix);
    let in_alpha = alpha * label_mix;

    let label_size = 10.0 * s;
    let label_baseline = geo.y + 34.0 * s;
    if out_alpha > 0.01 {
        let label = start.label();
        let width = measure_text(semibold, label, label_size);
        draw_text(
            pixmap,
            semibold,
            label,
            right - width,
            label_baseline,
            label_size,
            scale_alpha(accent, out_alpha),
        );
    }
    if in_alpha > 0.01 {
        let label = view.mode.label();
        let width = measure_text(semibold, label, label_size);
        draw_text(
            pixmap,
            semibold,
            label,
            right - width,
            label_baseline,
            label_size,
            scale_alpha(accent, in_alpha),
        );
    }

    let subtitle_size = 12.0 * s;
    let subtitle_baseline = geo.y + 60.0 * s;
    if out_alpha > 0.01 {
        draw_text(
            pixmap,
            regular,
            start.subtitle(),
            left,
            subtitle_baseline,
            subtitle_size,
            scale_alpha(muted, out_alpha),
        );
    }
    if in_alpha > 0.01 {
        draw_text(
            pixmap,
            regular,
            view.mode.subtitle(),
            left,
            subtitle_baseline,
            subtitle_size,
            scale_alpha(muted, in_alpha),
        );
    }

    let divider_y = geo.y + 82.0 * s;
    draw_line(
        pixmap,
        left,
        divider_y,
        right,
        divider_y,
        scale_alpha(ColorU8::from_rgba(255, 178, 36, 28), alpha),
        1.0 * s,
    );

    draw_text(
        pixmap,
        regular,
        "Two shapes. One quiet island.",
        left,
        geo.y + 112.0 * s,
        13.0 * s,
        scale_alpha(cream, alpha),
    );

    let hint = if view.pinned {
        "Pinned. Left-click to release."
    } else {
        "Hover · right-click to switch shape"
    };
    draw_text(
        pixmap,
        regular,
        hint,
        left,
        geo.y + 138.0 * s,
        11.0 * s,
        scale_alpha(muted, alpha),
    );

    draw_text(
        pixmap,
        regular,
        "Ctrl+Alt+N  shape   ·   Ctrl+Alt+Q  quit",
        left,
        geo.y + 160.0 * s,
        10.5 * s,
        scale_alpha(muted, alpha),
    );
}

fn island_path(geo: &Geometry, mode_mix: f32) -> Option<Path> {
    let x = geo.x;
    let y = geo.y;
    let width = geo.width;
    let height = geo.height;
    let top_radius = geo.top_radius.min(width / 2.0).min(height / 2.0);
    let bottom_radius = geo.bottom_radius.min(width / 2.0).min(height / 2.0);
    let flare = geo.notch_flare.min(width / 2.0);
    let shoulder = geo.notch_shoulder.min(height / 2.0);

    // The lip and the rounded pill corner share one continuous corner curve.
    // As the mode mix grows, the lip collapses (flare -> 0) while the corner
    // radius opens up, so there is no topology swap and no visible pop.
    let top_edge_left = x - flare + top_radius;
    let top_edge_right = x + width + flare - top_radius;
    let drop = lerp(shoulder, top_radius, mode_mix.clamp(0.0, 1.0));
    let mut pb = PathBuilder::new();

    pb.move_to(top_edge_left, y);
    pb.line_to(top_edge_right, y);
    pb.cubic_to(
        top_edge_right + (x + width - top_edge_right) * CORNER_K,
        y,
        x + width,
        y + drop * (1.0 - CORNER_K),
        x + width,
        y + drop,
    );
    pb.line_to(x + width, y + height - bottom_radius);
    pb.cubic_to(
        x + width,
        y + height - bottom_radius + bottom_radius * CORNER_K,
        x + width - bottom_radius + bottom_radius * CORNER_K,
        y + height,
        x + width - bottom_radius,
        y + height,
    );
    pb.line_to(x + bottom_radius, y + height);
    pb.cubic_to(
        x + bottom_radius - bottom_radius * CORNER_K,
        y + height,
        x,
        y + height - bottom_radius + bottom_radius * CORNER_K,
        x,
        y + height - bottom_radius,
    );
    pb.line_to(x, y + drop);
    pb.cubic_to(
        x,
        y + drop * (1.0 - CORNER_K),
        top_edge_left + (x - top_edge_left) * CORNER_K,
        y,
        top_edge_left,
        y,
    );
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
    let mut width = 0.0;
    let mut previous: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = previous {
            width += font.horizontal_kern(prev, ch, size).unwrap_or(0.0);
        }
        width += font.metrics(ch, size).advance_width;
        previous = Some(ch);
    }
    width
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
    let mut previous: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = previous {
            pen_x += font.horizontal_kern(prev, ch, size).unwrap_or(0.0);
        }
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
        previous = Some(ch);
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

fn composite_layer(dst: &mut Pixmap, src: &Pixmap, opacity: f32) {
    let opacity = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    if opacity == 0 {
        return;
    }

    let dst_data = dst.data_mut();
    let src_data = src.data();
    debug_assert_eq!(dst_data.len(), src_data.len());

    for i in (0..src_data.len()).step_by(4) {
        let src_alpha = src_data[i + 3] as u32 * opacity / 255;
        if src_alpha == 0 {
            continue;
        }
        let inv = 255 - src_alpha;
        dst_data[i] =
            ((src_data[i] as u32 * opacity / 255) + (dst_data[i] as u32 * inv / 255)) as u8;
        dst_data[i + 1] =
            ((src_data[i + 1] as u32 * opacity / 255) + (dst_data[i + 1] as u32 * inv / 255)) as u8;
        dst_data[i + 2] =
            ((src_data[i + 2] as u32 * opacity / 255) + (dst_data[i + 2] as u32 * inv / 255)) as u8;
        dst_data[i + 3] = (src_alpha + (dst_data[i + 3] as u32 * inv / 255)) as u8;
    }
}

fn clip_to_shape(content: &mut Pixmap, shape: &Pixmap, threshold: u8) {
    let content_data = content.data_mut();
    let shape_data = shape.data();
    debug_assert_eq!(content_data.len(), shape_data.len());

    for i in (0..content_data.len()).step_by(4) {
        if shape_data[i + 3] < threshold {
            content_data[i..i + 4].fill(0);
        }
    }
}

fn scale_alpha(color: ColorU8, factor: f32) -> ColorU8 {
    ColorU8::from_rgba(
        color.red(),
        color.green(),
        color.blue(),
        (color.alpha() as f32 * factor.clamp(0.0, 1.0)).round() as u8,
    )
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
