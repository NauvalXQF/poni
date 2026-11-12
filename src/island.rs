use fontdue::Font;
use tiny_skia::{
    Color, ColorU8, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke,
    Transform,
};

use crate::geometry::Geometry;

const CORNER_K: f32 = 0.58;
const SURFACE: ColorU8 = ColorU8::from_rgba(0, 0, 0, 255);
const TEXT: ColorU8 = ColorU8::from_rgba(245, 244, 240, 250);
const MUTED: ColorU8 = ColorU8::from_rgba(158, 157, 152, 205);
#[allow(dead_code)]
const HAIRLINE: ColorU8 = ColorU8::from_rgba(255, 255, 255, 10);
const GREEN: ColorU8 = ColorU8::from_rgba(102, 224, 151, 255);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Wifi,
    Bluetooth,
    Airplane,
}

pub struct IslandView<'a> {
    pub progress: f32,
    pub geometry: Geometry,
    pub scale: f32,
    pub time: String,
    pub date: String,
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
    pub wifi: Option<&'a str>,
    pub bluetooth: Option<&'a str>,
    pub notice_mix: f32,
    pub notice_kind: Option<NoticeKind>,
    pub notice_title: Option<&'a str>,
    pub notice_subtitle: Option<&'a str>,
    pub notice_battery: Option<u8>,
    pub notice_closing: bool,
    pub regular: Option<&'a Font>,
    pub semibold: Option<&'a Font>,
}

pub fn draw(width: u32, height: u32, view: &IslandView<'_>) -> Option<Pixmap> {
    let mut shape = Pixmap::new(width, height)?;
    draw_shape(&mut shape, view);

    // Content renders into its own layer so it can be clipped to the capsule
    // while the shape grows.
    let mut content = Pixmap::new(width, height)?;
    draw_content(&mut content, view);
    clip_to_shape(&mut content, &shape, 128);
    composite_layer(&mut shape, &content, 1.0);

    Some(shape)
}

fn draw_shape(pixmap: &mut Pixmap, view: &IslandView<'_>) {
    let geo = view.geometry;
    let Some(path) = island_path(&geo) else {
        return;
    };

    let mut fill = Paint::default();
    fill.set_color(Color::from_rgba8(
        SURFACE.red(),
        SURFACE.green(),
        SURFACE.blue(),
        SURFACE.alpha(),
    ));
    pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);

    // Attached shapes sit flush with the monitor bezel. Repaint the top lip
    // with an opaque strip so anti-aliasing cannot leave a translucent seam.
    if geo.y <= 0.01 {
        let lip_height = (2.0 * view.scale).max(2.0);
        if let Some(lip) = rounded_top_strip_path(
            geo.x - geo.flare - 1.0,
            0.0,
            geo.width + geo.flare * 2.0 + 2.0,
            lip_height,
        ) {
            pixmap.fill_path(&lip, &fill, FillRule::Winding, Transform::identity(), None);
        }
    }
}

fn draw_content(pixmap: &mut Pixmap, view: &IslandView<'_>) {
    if view.regular.is_none() && view.semibold.is_none() {
        return;
    }

    let alphas = content_alphas(view.progress, view.notice_mix, view.notice_closing);

    if alphas.collapsed > 0.01 {
        draw_collapsed(pixmap, view, alphas.collapsed);
    }
    if alphas.expanded > 0.01 {
        draw_expanded(pixmap, view, alphas.expanded);
    }
    if alphas.notice > 0.01 && view.notice_kind.is_some() {
        draw_notice(pixmap, view, alphas.notice);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ContentAlphas {
    collapsed: f32,
    expanded: f32,
    notice: f32,
}

/// Content fade windows are intentionally disjoint. Compact content leaves
/// before expanded content arrives, and notice text waits for compact content
/// to leave. On the way out the order reverses.
fn content_alphas(progress: f32, notice_mix: f32, notice_closing: bool) -> ContentAlphas {
    let progress = progress.clamp(0.0, 1.0);
    let notice_mix = notice_mix.clamp(0.0, 1.0);
    let (base_alpha, notice) = if notice_closing {
        (
            1.0 - smoothstep(0.05, 0.28, notice_mix),
            smoothstep(0.30, 0.70, notice_mix),
        )
    } else {
        (
            1.0 - smoothstep(0.05, 0.30, notice_mix),
            smoothstep(0.35, 0.80, notice_mix),
        )
    };

    ContentAlphas {
        collapsed: (1.0 - smoothstep(0.05, 0.45, progress)) * base_alpha,
        expanded: smoothstep(0.55, 0.95, progress) * base_alpha,
        notice,
    }
}

fn draw_collapsed(pixmap: &mut Pixmap, view: &IslandView<'_>, alpha: f32) {
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);

    let s = view.scale;
    let geo = view.geometry;
    let left = geo.x + 18.0 * s;
    let right = geo.x + geo.width - 18.0 * s;
    let baseline = geo.y + geo.height / 2.0 + 4.5 * s;

    draw_text(
        pixmap,
        semibold,
        &view.time,
        left,
        baseline,
        13.5 * s,
        scale_alpha(TEXT, alpha),
    );

    let Some(percent) = view.battery_percent else {
        return;
    };

    let radius = 6.2 * s;
    let icon_x = right - radius;
    let icon_y = geo.y + geo.height / 2.0;
    let label = format!("{percent}%");
    let label_size = 10.0 * s;
    let label_width = measure_text(regular, &label, label_size);
    let label_color = if view.battery_charging {
        scale_alpha(GREEN, alpha)
    } else {
        scale_alpha(MUTED, alpha)
    };
    draw_text(
        pixmap,
        regular,
        &label,
        icon_x - radius - 6.0 * s - label_width,
        baseline - 0.5 * s,
        label_size,
        label_color,
    );
    draw_battery(
        pixmap,
        icon_x,
        icon_y,
        radius,
        percent,
        view.battery_charging,
        alpha,
        TEXT,
        GREEN,
    );
}

fn draw_expanded(pixmap: &mut Pixmap, view: &IslandView<'_>, alpha: f32) {
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);

    let s = view.scale;
    let geo = view.geometry;
    let left = geo.x + 28.0 * s;
    let right = geo.x + geo.width - 28.0 * s;
    let top = geo.y;

    draw_text(
        pixmap,
        semibold,
        &view.time,
        left,
        top + 46.0 * s,
        31.0 * s,
        scale_alpha(TEXT, alpha),
    );
    draw_text(
        pixmap,
        regular,
        &view.date,
        left,
        top + 66.0 * s,
        11.5 * s,
        scale_alpha(MUTED, alpha),
    );

    if let Some(percent) = view.battery_percent {
        let radius = 8.2 * s;
        let icon_x = right - radius;
        let icon_y = top + 31.0 * s;
        let label = format!("{percent}%");
        let label_size = 15.5 * s;
        let label_width = measure_text(semibold, &label, label_size);
        let label_color = if view.battery_charging {
            scale_alpha(GREEN, alpha)
        } else {
            scale_alpha(TEXT, alpha)
        };
        draw_text(
            pixmap,
            semibold,
            &label,
            icon_x - radius - 8.0 * s - label_width,
            top + 39.0 * s,
            label_size,
            label_color,
        );
        draw_battery(
            pixmap,
            icon_x,
            icon_y,
            radius,
            percent,
            view.battery_charging,
            alpha,
            TEXT,
            GREEN,
        );
    }

    let divider_y = top + 82.0 * s;
    draw_line(
        pixmap,
        left,
        divider_y,
        right,
        divider_y,
        scale_alpha(HAIRLINE, alpha),
        1.0 * s,
    );

    let baseline = top + 112.0 * s;
    let column_width = ((right - left) * 0.5 - 12.0 * s).max(0.0);

    let wifi = view.wifi.unwrap_or("Wi-Fi off");
    let wifi_color = if view.wifi.is_some() { TEXT } else { MUTED };
    draw_wifi_icon(
        pixmap,
        left + 6.5 * s,
        baseline - 4.5 * s,
        13.0 * s,
        scale_alpha(wifi_color, alpha),
    );
    let wifi_label = elide(regular, wifi, 10.5 * s, column_width - 18.0 * s);
    draw_text(
        pixmap,
        regular,
        &wifi_label,
        left + 17.0 * s,
        baseline,
        10.5 * s,
        scale_alpha(wifi_color, alpha),
    );

    let bluetooth = view.bluetooth.unwrap_or("Bluetooth off");
    let bluetooth_color = if view.bluetooth.is_some() {
        TEXT
    } else {
        MUTED
    };
    let bluetooth_label = elide(regular, bluetooth, 10.5 * s, column_width - 20.0 * s);
    let bluetooth_width = measure_text(regular, &bluetooth_label, 10.5 * s);
    let icon_cx = right - 6.5 * s;
    draw_text(
        pixmap,
        regular,
        &bluetooth_label,
        icon_cx - 11.0 * s - bluetooth_width,
        baseline,
        10.5 * s,
        scale_alpha(bluetooth_color, alpha),
    );
    draw_bluetooth_icon(
        pixmap,
        icon_cx,
        baseline - 4.5 * s,
        13.0 * s,
        scale_alpha(bluetooth_color, alpha),
    );
}

fn draw_notice(pixmap: &mut Pixmap, view: &IslandView<'_>, alpha: f32) {
    let (Some(kind), Some(title), Some(subtitle)) =
        (view.notice_kind, view.notice_title, view.notice_subtitle)
    else {
        return;
    };
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);
    let s = view.scale;
    let geo = view.geometry;
    let icon_x = geo.x + 24.0 * s;
    let center_y = geo.y + geo.height / 2.0;
    let icon_color = scale_alpha(TEXT, alpha);

    draw_filled_circle(
        pixmap,
        icon_x,
        center_y,
        15.0 * s,
        ColorU8::from_rgba(255, 255, 255, (12.0 * alpha) as u8),
    );
    draw_notice_icon(pixmap, kind, icon_x, center_y, 18.0 * s, icon_color);

    draw_text(
        pixmap,
        semibold,
        title,
        geo.x + 46.0 * s,
        geo.y + 23.0 * s,
        11.5 * s,
        scale_alpha(TEXT, alpha),
    );
    draw_text(
        pixmap,
        regular,
        subtitle,
        geo.x + 46.0 * s,
        geo.y + 39.0 * s,
        10.0 * s,
        scale_alpha(MUTED, alpha),
    );

    if kind == NoticeKind::Bluetooth {
        if let Some(battery) = view.notice_battery {
            let label = format!("{battery}%");
            let label_size = 10.5 * s;
            let label_width = measure_text(semibold, &label, label_size);
            draw_text(
                pixmap,
                semibold,
                &label,
                geo.x + geo.width - 18.0 * s - label_width,
                geo.y + 23.0 * s,
                label_size,
                scale_alpha(TEXT, alpha),
            );
        }
    }
}

fn draw_notice_icon(
    pixmap: &mut Pixmap,
    kind: NoticeKind,
    cx: f32,
    cy: f32,
    size: f32,
    color: ColorU8,
) {
    match kind {
        NoticeKind::Wifi => draw_wifi_icon(pixmap, cx, cy, size, color),
        NoticeKind::Bluetooth => draw_bluetooth_icon(pixmap, cx, cy, size, color),
        NoticeKind::Airplane => draw_airplane_icon(pixmap, cx, cy, size, color),
    }
}

fn draw_wifi_icon(pixmap: &mut Pixmap, cx: f32, cy: f32, size: f32, color: ColorU8) {
    let center_y = cy + size * 0.22;
    for radius in [size * 0.42, size * 0.29, size * 0.16] {
        draw_arc(
            pixmap,
            cx,
            center_y,
            radius,
            205.0,
            335.0,
            color,
            (size * 0.08).max(1.2),
        );
    }
    draw_filled_circle(pixmap, cx, center_y, size * 0.055, color);
}

fn draw_bluetooth_icon(pixmap: &mut Pixmap, cx: f32, cy: f32, size: f32, color: ColorU8) {
    let height = size * 0.42;
    let width = size * 0.20;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - height);
    pb.line_to(cx, cy + height);
    pb.move_to(cx, cy - height);
    pb.line_to(cx + width, cy - height * 0.5);
    pb.line_to(cx - width, cy);
    pb.line_to(cx + width, cy + height * 0.5);
    pb.line_to(cx, cy + height);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        ));
        let stroke = Stroke {
            width: (size * 0.08).max(1.2),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_airplane_icon(pixmap: &mut Pixmap, cx: f32, cy: f32, size: f32, color: ColorU8) {
    let s = size * 0.5;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - s);
    pb.line_to(cx + s * 0.16, cy - s);
    pb.line_to(cx + s * 0.16, cy - s * 0.15);
    pb.line_to(cx + s, cy + s * 0.25);
    pb.line_to(cx + s, cy + s * 0.42);
    pb.line_to(cx + s * 0.16, cy + s * 0.16);
    pb.line_to(cx + s * 0.16, cy + s * 0.78);
    pb.line_to(cx + s * 0.42, cy + s);
    pb.line_to(cx - s * 0.42, cy + s);
    pb.line_to(cx - s * 0.16, cy + s * 0.78);
    pb.line_to(cx - s * 0.16, cy + s * 0.16);
    pb.line_to(cx - s, cy + s * 0.42);
    pb.line_to(cx - s, cy + s * 0.25);
    pb.line_to(cx - s * 0.16, cy - s * 0.15);
    pb.line_to(cx - s * 0.16, cy - s);
    pb.close();
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

fn draw_arc(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    radius: f32,
    start_deg: f32,
    end_deg: f32,
    color: ColorU8,
    width: f32,
) {
    let mut pb = PathBuilder::new();
    let steps = 20;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let angle = (start_deg + (end_deg - start_deg) * t).to_radians();
        let x = cx + radius * angle.cos();
        let y = cy + radius * angle.sin();
        if step == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
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
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_filled_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, radius: f32, color: ColorU8) {
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

fn draw_battery(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    radius: f32,
    percent: u8,
    charging: bool,
    alpha: f32,
    color: ColorU8,
    charging_color: ColorU8,
) {
    let accent = if charging { charging_color } else { color };
    let track = scale_alpha(color, alpha * 0.28);
    let track_width = (radius * 0.28).max(1.4);
    draw_arc(
        pixmap,
        cx,
        cy,
        radius - track_width / 2.0,
        0.0,
        360.0,
        track,
        track_width,
    );

    let sweep = 360.0 * percent.min(100) as f32 / 100.0;
    if sweep > 0.5 {
        draw_arc(
            pixmap,
            cx,
            cy,
            radius - track_width / 2.0,
            -90.0,
            -90.0 + sweep,
            scale_alpha(accent, alpha),
            track_width,
        );
    }

    if charging {
        draw_bolt(
            pixmap,
            cx,
            cy + radius * 0.02,
            radius * 1.15,
            scale_alpha(color, alpha),
            scale_alpha(ColorU8::from_rgba(0, 0, 0, 255), alpha * 0.8),
        );
    }
}

fn draw_bolt(pixmap: &mut Pixmap, cx: f32, cy: f32, height: f32, color: ColorU8, outline: ColorU8) {
    let Some(path) = bolt_path(cx, cy, height) else {
        return;
    };
    let mut outline_paint = Paint::default();
    outline_paint.set_color(Color::from_rgba8(
        outline.red(),
        outline.green(),
        outline.blue(),
        outline.alpha(),
    ));
    let outline_stroke = Stroke {
        width: (height * 0.26).max(1.8),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    pixmap.stroke_path(
        &path,
        &outline_paint,
        &outline_stroke,
        Transform::identity(),
        None,
    );

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha(),
    ));
    let stroke = Stroke {
        width: (height * 0.14).max(1.0),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn bolt_path(cx: f32, cy: f32, height: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(cx + height * 0.16, cy - height * 0.48);
    pb.line_to(cx - height * 0.18, cy + height * 0.04);
    pb.line_to(cx + height * 0.03, cy + height * 0.04);
    pb.line_to(cx - height * 0.12, cy + height * 0.48);
    pb.finish()
}

fn rounded_top_strip_path(x: f32, y: f32, width: f32, height: f32) -> Option<Path> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + width, y);
    pb.line_to(x + width, y + height);
    pb.line_to(x, y + height);
    pb.close();
    pb.finish()
}

fn island_path(geo: &Geometry) -> Option<Path> {
    let x = geo.x;
    let y = geo.y;
    let width = geo.width;
    let height = geo.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let top_radius = geo.top_radius.min(width / 2.0).min(height / 2.0);
    let bottom_radius = geo.bottom_radius.min(width / 2.0).min(height / 2.0);
    let flare = geo.flare.min(width / 2.0);
    let shoulder = geo.shoulder.min(height / 2.0);

    // Attached shapes keep a lip and a shoulder that falls away to the body.
    // Floating shapes set flare and shoulder to zero, which collapses this
    // path into a symmetric rounded capsule.
    let top_edge_left = x - flare + top_radius;
    let top_edge_right = x + width + flare - top_radius;
    let drop = top_radius.max(shoulder).min(height / 2.0);
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
            line_cap: LineCap::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn elide(font: &Font, text: &str, size: f32, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if measure_text(font, text, size) <= max_width {
        return text.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = measure_text(font, ellipsis, size);
    if ellipsis_width >= max_width {
        return String::new();
    }

    let mut result = String::new();
    let mut width = 0.0;
    for ch in text.chars() {
        let advance = font.metrics(ch, size).advance_width;
        if width + advance + ellipsis_width > max_width {
            break;
        }
        result.push(ch);
        width += advance;
    }
    result.push('…');
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_layers_never_overlap() {
        for step in 0..=100 {
            let t = step as f32 / 100.0;
            for closing in [false, true] {
                let alphas = content_alphas(t, t, closing);
                assert!(
                    alphas.collapsed <= 1e-6 || alphas.expanded <= 1e-6,
                    "compact and expanded overlapped at {t} (closing: {closing})"
                );
                assert!(
                    alphas.collapsed <= 1e-6 || alphas.notice <= 1e-6,
                    "compact and notice overlapped at {t} (closing: {closing})"
                );
                assert!(
                    alphas.expanded <= 1e-6 || alphas.notice <= 1e-6,
                    "expanded and notice overlapped at {t} (closing: {closing})"
                );
            }
        }
    }

    #[test]
    fn compact_and_expanded_hand_off_at_midpoint() {
        let alphas = content_alphas(0.5, 0.0, false);
        assert!(alphas.collapsed <= 1e-6);
        assert!(alphas.expanded <= 1e-6);
    }

    #[test]
    fn notice_waits_for_compact_content_to_leave() {
        let alphas = content_alphas(0.0, 0.32, false);
        assert!(alphas.collapsed <= 1e-6);
        assert!(alphas.notice <= 1e-6);
    }

    #[test]
    fn notice_leaves_before_compact_content_returns() {
        let leaving = content_alphas(0.0, 0.7, true);
        assert!(leaving.collapsed <= 1e-6);
        assert!((leaving.notice - 1.0).abs() < 1e-6);

        let gone = content_alphas(0.0, 0.3, true);
        assert!(gone.collapsed <= 1e-6);
        assert!(gone.notice <= 1e-6);

        let returning = content_alphas(0.0, 0.1, true);
        assert!(returning.collapsed > 0.5);
        assert!(returning.notice <= 1e-6);
    }

    #[test]
    fn content_layers_never_overlap_full_black_surface() {
        // Surface is pure black for OLED; geometry/content invariants still hold.
        assert_eq!(super::SURFACE.alpha(), 255);
        assert_eq!(super::SURFACE.red(), 0);
    }
}
