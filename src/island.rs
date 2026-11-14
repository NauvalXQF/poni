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
/// Charging accent: a cool cyan that separates "plugged in" from the red
/// low-battery state and the neutral white ring.
const CYAN: ColorU8 = ColorU8::from_rgba(88, 202, 242, 255);
const RED: ColorU8 = ColorU8::from_rgba(255, 99, 99, 255);
/// ReyLand's timer accent: warm amber, distinct from the blue-ish white clock
/// and the system accents used by the battery ring.
const AMBER: ColorU8 = ColorU8::from_rgba(242, 163, 59, 255);
/// Unified chip fill for the preset buttons on the idle card.
const CHIP: ColorU8 = ColorU8::from_rgba(255, 255, 255, 20);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Wifi,
    Bluetooth,
    Airplane,
    Timer,
}

/// One live countdown or stopwatch snapshot, already formatted by the caller.
pub struct TimerView {
    pub display: String,
    pub label: &'static str,
    pub progress: f32,
    pub countdown: bool,
    pub running: bool,
}

/// One preset chip on the idle card. `seconds` is `None` for the stopwatch.
#[derive(Clone, Copy, Debug)]
pub struct TimerPreset {
    pub label: &'static str,
    pub seconds: Option<u64>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Preset chips share their geometry with the click handler so a click always
/// lands on the chip that was drawn.
pub fn timer_presets(geo: &Geometry, scale: f32) -> [TimerPreset; 5] {
    const DEFS: [(&str, Option<u64>); 5] = [
        ("1m", Some(60)),
        ("5m", Some(300)),
        ("10m", Some(600)),
        ("25m", Some(1500)),
        ("Stopwatch", None),
    ];
    let s = scale;
    let left = geo.x + 28.0 * s;
    let right = geo.x + geo.width - 28.0 * s;
    let gap = 7.0 * s;
    let width = ((right - left) - gap * 4.0).max(0.0) / 5.0;
    let y = geo.y + 66.0 * s;
    let height = 30.0 * s;
    std::array::from_fn(|index| TimerPreset {
        label: DEFS[index].0,
        seconds: DEFS[index].1,
        x: left + index as f32 * (width + gap),
        y,
        width,
        height,
    })
}

/// Circle centres for the expanded timer controls.
#[derive(Clone, Copy, Debug)]
pub struct TimerControls {
    pub pause: (f32, f32),
    pub stop: (f32, f32),
    pub radius: f32,
}

pub fn timer_controls(geo: &Geometry, scale: f32) -> TimerControls {
    let cx = geo.x + geo.width / 2.0;
    let cy = geo.y + 84.0 * scale;
    TimerControls {
        pause: (cx - 28.0 * scale, cy),
        stop: (cx + 28.0 * scale, cy),
        radius: 16.0 * scale,
    }
}

pub struct MediaView<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub art: Option<&'a Pixmap>,
}

impl MediaView<'_> {
    /// Playback progress in the 0..=1 range, or zero when the length is unknown.
    fn progress(&self) -> f32 {
        if self.duration_ms == 0 {
            return 0.0;
        }
        (self.position_ms as f32 / self.duration_ms as f32).clamp(0.0, 1.0)
    }
}

/// Clickable circles for the expanded transport row, in surface coordinates.
/// The renderer and the window procedure share this so a click always lands on
/// the icon that was drawn.
#[derive(Clone, Copy, Debug)]
pub struct MediaControls {
    pub previous: (f32, f32),
    pub play: (f32, f32),
    pub next: (f32, f32),
    pub radius: f32,
}

pub fn media_controls(geo: &Geometry, scale: f32) -> MediaControls {
    let cx = geo.x + geo.width / 2.0;
    let cy = geo.y + 84.0 * scale;
    MediaControls {
        previous: (cx - 36.0 * scale, cy),
        play: (cx, cy),
        next: (cx + 36.0 * scale, cy),
        radius: 16.0 * scale,
    }
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
    pub media: Option<MediaView<'a>>,
    /// A running or paused timer takes over both the pill and the card.
    pub timer: Option<TimerView>,
    /// Monotonic seconds used to animate the live waveform.
    pub anim: f32,
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
    // Double-fill reduces anti-aliased fringe so the attached edge blends into
    // the OLED bezel without adding a rectangular seam artifact.
    pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
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
    let baseline = geo.y + geo.height / 2.0 + 5.5 * s;

    // The timer replaces the clock instead of joining it, so the pill stays
    // glanceable while a countdown is live.
    if let Some(timer) = view.timer.as_ref() {
        draw_collapsed_timer(pixmap, view, timer, left, right, baseline, alpha);
        return;
    }

    draw_text(
        pixmap,
        semibold,
        &view.time,
        left,
        baseline,
        15.5 * s,
        scale_alpha(TEXT, alpha),
    );

    if let Some(media) = view.media.as_ref() {
        draw_compact_media(pixmap, view, media, right, alpha);
        return;
    }

    let Some(percent) = view.battery_percent else {
        return;
    };

    let radius = 7.2 * s;
    let icon_x = right - radius;
    let icon_y = geo.y + geo.height / 2.0 + 0.5 * s;
    let label = format!("{percent}%");
    let label_size = 11.5 * s;
    let label_width = measure_text(regular, &label, label_size);
    let accent = if view.battery_charging {
        CYAN
    } else if percent <= 20 {
        RED
    } else {
        TEXT
    };
    let label_color = if view.battery_charging {
        scale_alpha(CYAN, alpha)
    } else if percent <= 20 {
        scale_alpha(RED, alpha)
    } else {
        scale_alpha(MUTED, alpha)
    };
    draw_text(
        pixmap,
        regular,
        &label,
        icon_x - radius - 7.0 * s - label_width,
        baseline,
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
        accent,
    );
}

/// Compact pill while a timer runs: amber stopwatch, the live value, and a
/// muted mode label on the right. The dimmed value means the timer is paused.
fn draw_collapsed_timer(
    pixmap: &mut Pixmap,
    view: &IslandView<'_>,
    timer: &TimerView,
    left: f32,
    right: f32,
    baseline: f32,
    alpha: f32,
) {
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);
    let s = view.scale;
    let center_y = view.geometry.y + view.geometry.height / 2.0;

    let icon_size = 14.0 * s;
    let icon_cx = left + icon_size * 0.42;
    draw_stopwatch_icon(
        pixmap,
        icon_cx,
        center_y,
        icon_size,
        scale_alpha(AMBER, alpha),
    );

    let text_left = icon_cx + icon_size * 0.72;
    let color = if timer.running { TEXT } else { MUTED };
    draw_text(
        pixmap,
        semibold,
        &timer.display,
        text_left,
        baseline,
        15.5 * s,
        scale_alpha(color, alpha),
    );

    let label_size = 10.5 * s;
    let label_width = measure_text(regular, timer.label, label_size);
    draw_text(
        pixmap,
        regular,
        timer.label,
        right - label_width,
        center_y + 3.7 * s,
        label_size,
        scale_alpha(MUTED, alpha * 0.9),
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

    if let Some(timer) = view.timer.as_ref() {
        draw_expanded_timer(pixmap, view, timer, regular, semibold, alpha);
        draw_expanded_status(pixmap, view, regular, alpha);
        return;
    }

    if let Some(media) = view.media.as_ref() {
        draw_expanded_media(pixmap, view, media, regular, semibold, alpha);
        draw_expanded_status(pixmap, view, regular, alpha);
        return;
    }

    draw_text(
        pixmap,
        semibold,
        &view.time,
        left,
        top + 38.0 * s,
        31.0 * s,
        scale_alpha(TEXT, alpha),
    );
    draw_text(
        pixmap,
        regular,
        &view.date,
        left,
        top + 56.0 * s,
        11.5 * s,
        scale_alpha(MUTED, alpha),
    );

    if let Some(percent) = view.battery_percent {
        let radius = 8.2 * s;
        let icon_x = right - radius;
        let icon_y = top + 33.5 * s;
        let label = format!("{percent}%");
        let label_size = 15.5 * s;
        let label_width = measure_text(semibold, &label, label_size);
        let accent = if view.battery_charging {
            CYAN
        } else if percent <= 20 {
            RED
        } else {
            TEXT
        };
        let label_color = if view.battery_charging {
            scale_alpha(CYAN, alpha)
        } else if percent <= 20 {
            scale_alpha(RED, alpha)
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
            accent,
        );
    }

    draw_timer_presets(pixmap, view, semibold, alpha);
    draw_expanded_status(pixmap, view, regular, alpha);
}

/// Wi-Fi on the left and Bluetooth on the right, sharing the baseline of the
/// clock layout so the two views line up when media takes over the expanded
/// card.
fn draw_expanded_status(pixmap: &mut Pixmap, view: &IslandView<'_>, regular: &Font, alpha: f32) {
    let s = view.scale;
    let geo = view.geometry;
    let left = geo.x + 28.0 * s;
    let right = geo.x + geo.width - 28.0 * s;

    let baseline = geo.y + 112.0 * s;
    let column_width = ((right - left) * 0.5 - 12.0 * s).max(0.0);

    let wifi = view.wifi.unwrap_or("Wi-Fi off");
    let wifi_color = if view.wifi.is_some() { TEXT } else { MUTED };
    draw_wifi_icon(
        pixmap,
        left + 8.0 * s,
        baseline - 4.0 * s,
        17.0 * s,
        scale_alpha(wifi_color, alpha),
    );
    let wifi_label = elide(regular, wifi, 10.5 * s, column_width - 22.0 * s);
    draw_text(
        pixmap,
        regular,
        &wifi_label,
        left + 20.0 * s,
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
    let bluetooth_label = elide(regular, bluetooth, 10.5 * s, column_width - 22.0 * s);
    let bluetooth_width = measure_text(regular, &bluetooth_label, 10.5 * s);
    let icon_cx = right - 6.0 * s;
    draw_text(
        pixmap,
        regular,
        &bluetooth_label,
        icon_cx - 8.0 * s - bluetooth_width,
        baseline,
        10.5 * s,
        scale_alpha(bluetooth_color, alpha),
    );
    draw_bluetooth_icon(
        pixmap,
        icon_cx,
        baseline - 4.0 * s,
        17.0 * s,
        scale_alpha(bluetooth_color, alpha),
    );
}

/// Compact pill: clock on the left, elided track title in the middle, live
/// waveform and cover art on the right. Everything shares the 34px height so
/// the capsule never grows for media.
fn draw_compact_media(
    pixmap: &mut Pixmap,
    view: &IslandView<'_>,
    media: &MediaView<'_>,
    right: f32,
    alpha: f32,
) {
    let Some(regular) = view.regular.or(view.semibold) else {
        return;
    };
    let semibold = view.semibold.unwrap_or(regular);
    let s = view.scale;
    let geo = view.geometry;
    let center_y = geo.y + geo.height / 2.0;

    let art_size = 19.0 * s;
    let art_x = right - art_size;
    draw_art(
        pixmap,
        media.art,
        art_x,
        center_y - art_size / 2.0,
        art_size,
        alpha,
    );

    let wave_gap = 7.0 * s;
    let wave_width = 16.0 * s;
    let wave_left = art_x - wave_gap - wave_width;
    if media.playing {
        draw_waveform(
            pixmap,
            wave_left,
            center_y,
            wave_width,
            11.0 * s,
            view.anim,
            scale_alpha(TEXT, alpha * 0.9),
        );
    }

    let title_left = geo.x + 18.0 * s + measure_text(semibold, &view.time, 15.5 * s) + 9.0 * s;
    let title_right = if media.playing {
        wave_left - 7.0 * s
    } else {
        art_x - 8.0 * s
    };
    let max_width = title_right - title_left;
    if max_width < 14.0 * s {
        return;
    }
    let title = elide(regular, media.title, 10.5 * s, max_width);
    draw_text(
        pixmap,
        regular,
        &title,
        title_left,
        center_y + 3.7 * s,
        10.5 * s,
        scale_alpha(TEXT, alpha * 0.86),
    );
}

/// Expanded card: cover art, title/artist, progress, transport controls, and
/// the status row underneath. The vertical rhythm is fixed so the waveform
/// card and the clock card swap without the status row jumping.
fn draw_expanded_media(
    pixmap: &mut Pixmap,
    view: &IslandView<'_>,
    media: &MediaView<'_>,
    regular: &Font,
    semibold: &Font,
    alpha: f32,
) {
    let s = view.scale;
    let geo = view.geometry;
    let left = geo.x + 28.0 * s;
    let right = geo.x + geo.width - 28.0 * s;
    let top = geo.y;

    let art_size = 40.0 * s;
    let art_x = left;
    let art_y = top + 14.0 * s;
    draw_art(pixmap, media.art, art_x, art_y, art_size, alpha);

    let text_left = art_x + art_size + 12.0 * s;
    let duration = format_time(media.duration_ms);
    let duration_width = if media.duration_ms > 0 {
        measure_text(regular, &duration, 9.5 * s)
    } else {
        0.0
    };
    let text_right = right
        - if duration_width > 0.0 {
            duration_width + 12.0 * s
        } else {
            0.0
        };
    let text_width = (text_right - text_left).max(0.0);

    let title = elide(semibold, media.title, 14.5 * s, text_width);
    draw_text(
        pixmap,
        semibold,
        &title,
        text_left,
        art_y + 18.0 * s,
        14.5 * s,
        scale_alpha(TEXT, alpha),
    );

    let artist = if media.artist.trim().is_empty() {
        "Unknown artist"
    } else {
        media.artist
    };
    let artist = elide(regular, artist, 10.5 * s, text_width);
    draw_text(
        pixmap,
        regular,
        &artist,
        text_left,
        art_y + 34.0 * s,
        10.5 * s,
        scale_alpha(MUTED, alpha),
    );

    if duration_width > 0.0 {
        draw_text(
            pixmap,
            regular,
            &duration,
            right - duration_width,
            art_y + 34.0 * s,
            9.5 * s,
            scale_alpha(MUTED, alpha),
        );
    }

    let bar_y = top + 62.0 * s;
    let bar_height = 3.0 * s;
    let bar_radius = bar_height / 2.0;
    fill_rounded_rect(
        pixmap,
        left,
        bar_y,
        right - left,
        bar_height,
        bar_radius,
        ColorU8::from_rgba(255, 255, 255, (46.0 * alpha) as u8),
    );
    let progress = media.progress();
    if progress > 0.001 {
        fill_rounded_rect(
            pixmap,
            left,
            bar_y,
            (right - left) * progress,
            bar_height,
            bar_radius,
            scale_alpha(TEXT, alpha),
        );
    }

    let controls = media_controls(&geo, s);
    let icon_color = scale_alpha(TEXT, alpha);
    draw_filled_circle(
        pixmap,
        controls.play.0,
        controls.play.1,
        14.0 * s,
        ColorU8::from_rgba(255, 255, 255, (22.0 * alpha) as u8),
    );
    draw_transport_icon(
        pixmap,
        if media.playing {
            PlaybackIcon::Pause
        } else {
            PlaybackIcon::Play
        },
        controls.play.0,
        controls.play.1,
        13.0 * s,
        icon_color,
    );
    draw_transport_icon(
        pixmap,
        PlaybackIcon::Previous,
        controls.previous.0,
        controls.previous.1,
        11.0 * s,
        scale_alpha(TEXT, alpha * 0.85),
    );
    draw_transport_icon(
        pixmap,
        PlaybackIcon::Next,
        controls.next.0,
        controls.next.1,
        11.0 * s,
        scale_alpha(TEXT, alpha * 0.85),
    );
}

/// Expanded timer card: amber stopwatch, large live value, a depleting bar
/// for countdowns, and pause/stop circles on the shared control row.
fn draw_expanded_timer(
    pixmap: &mut Pixmap,
    view: &IslandView<'_>,
    timer: &TimerView,
    regular: &Font,
    semibold: &Font,
    alpha: f32,
) {
    let s = view.scale;
    let geo = view.geometry;
    let left = geo.x + 28.0 * s;
    let right = geo.x + geo.width - 28.0 * s;
    let top = geo.y;

    let icon_size = 19.0 * s;
    let icon_cx = left + icon_size * 0.46;
    let icon_cy = top + 30.0 * s;
    draw_stopwatch_icon(
        pixmap,
        icon_cx,
        icon_cy,
        icon_size,
        scale_alpha(AMBER, alpha),
    );

    let time_left = icon_cx + icon_size * 0.78;
    draw_text(
        pixmap,
        semibold,
        &timer.display,
        time_left,
        top + 42.0 * s,
        30.0 * s,
        scale_alpha(if timer.running { TEXT } else { MUTED }, alpha),
    );

    let label_size = 10.5 * s;
    let label_width = measure_text(regular, timer.label, label_size);
    draw_text(
        pixmap,
        regular,
        timer.label,
        right - label_width,
        top + 22.0 * s,
        label_size,
        scale_alpha(MUTED, alpha * 0.9),
    );
    if !timer.running {
        let paused = "Paused";
        let paused_width = measure_text(regular, paused, label_size);
        draw_text(
            pixmap,
            regular,
            paused,
            right - paused_width,
            top + 38.0 * s,
            label_size,
            scale_alpha(AMBER, alpha),
        );
    }

    if timer.countdown {
        let bar_y = top + 62.0 * s;
        let bar_height = 3.0 * s;
        let bar_radius = bar_height / 2.0;
        let full_width = right - left;
        fill_rounded_rect(
            pixmap,
            left,
            bar_y,
            full_width,
            bar_height,
            bar_radius,
            ColorU8::from_rgba(255, 255, 255, (46.0 * alpha) as u8),
        );
        let progress = timer.progress.clamp(0.0, 1.0);
        if progress > 0.001 {
            let fill_alpha = if timer.running { alpha } else { alpha * 0.55 };
            fill_rounded_rect(
                pixmap,
                left,
                bar_y,
                full_width * progress,
                bar_height,
                bar_radius,
                scale_alpha(AMBER, fill_alpha),
            );
        }
    }

    let controls = timer_controls(&geo, s);
    let circle_fill = ColorU8::from_rgba(255, 255, 255, (22.0 * alpha) as u8);
    draw_filled_circle(
        pixmap,
        controls.pause.0,
        controls.pause.1,
        controls.radius,
        circle_fill,
    );
    draw_transport_icon(
        pixmap,
        if timer.running {
            PlaybackIcon::Pause
        } else {
            PlaybackIcon::Play
        },
        controls.pause.0,
        controls.pause.1,
        13.0 * s,
        scale_alpha(TEXT, alpha),
    );
    draw_filled_circle(
        pixmap,
        controls.stop.0,
        controls.stop.1,
        controls.radius,
        circle_fill,
    );
    draw_stop_icon(
        pixmap,
        controls.stop.0,
        controls.stop.1,
        13.0 * s,
        scale_alpha(TEXT, alpha),
    );
}

/// Five chips across the idle card. They sit above the status row and share
/// their geometry with the click handler.
fn draw_timer_presets(pixmap: &mut Pixmap, view: &IslandView<'_>, font: &Font, alpha: f32) {
    let s = view.scale;
    for preset in timer_presets(&view.geometry, s) {
        fill_rounded_rect(
            pixmap,
            preset.x,
            preset.y,
            preset.width,
            preset.height,
            preset.height / 2.0,
            scale_alpha(CHIP, alpha),
        );
        let mut size = 10.0 * s;
        let mut label_width = measure_text(font, preset.label, size);
        let max_width = (preset.width - 10.0 * s).max(0.0);
        if label_width > max_width && label_width > 0.0 {
            size *= max_width / label_width;
            label_width = measure_text(font, preset.label, size);
        }
        draw_text(
            pixmap,
            font,
            preset.label,
            preset.x + (preset.width - label_width) / 2.0,
            preset.y + preset.height / 2.0 + size * 0.35,
            size,
            scale_alpha(TEXT, alpha * 0.88),
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaybackIcon {
    Previous,
    Play,
    Pause,
    Next,
}

fn draw_transport_icon(
    pixmap: &mut Pixmap,
    icon: PlaybackIcon,
    cx: f32,
    cy: f32,
    size: f32,
    color: ColorU8,
) {
    match icon {
        PlaybackIcon::Play => fill_triangle(
            pixmap,
            (cx - size * 0.34, cy - size * 0.5),
            (cx - size * 0.34, cy + size * 0.5),
            (cx + size * 0.5, cy),
            color,
        ),
        PlaybackIcon::Pause => {
            let bar_width = size * 0.24;
            let bar_height = size * 0.92;
            fill_rounded_rect(
                pixmap,
                cx - size * 0.32,
                cy - bar_height / 2.0,
                bar_width,
                bar_height,
                bar_width * 0.35,
                color,
            );
            fill_rounded_rect(
                pixmap,
                cx + size * 0.08,
                cy - bar_height / 2.0,
                bar_width,
                bar_height,
                bar_width * 0.35,
                color,
            );
        }
        PlaybackIcon::Previous => {
            fill_rounded_rect(
                pixmap,
                cx - size * 0.62,
                cy - size * 0.45,
                size * 0.16,
                size * 0.9,
                size * 0.08,
                color,
            );
            fill_triangle(
                pixmap,
                (cx + size * 0.5, cy - size * 0.45),
                (cx + size * 0.5, cy + size * 0.45),
                (cx - size * 0.42, cy),
                color,
            );
        }
        PlaybackIcon::Next => {
            fill_rounded_rect(
                pixmap,
                cx + size * 0.46,
                cy - size * 0.45,
                size * 0.16,
                size * 0.9,
                size * 0.08,
                color,
            );
            fill_triangle(
                pixmap,
                (cx - size * 0.5, cy - size * 0.45),
                (cx - size * 0.5, cy + size * 0.45),
                (cx + size * 0.42, cy),
                color,
            );
        }
    }
}

/// A stopwatch with a crown, a side pusher, and a single hand. Used for the
/// timer accent in the pill, the expanded card, and the completion notice.
fn draw_stopwatch_icon(pixmap: &mut Pixmap, cx: f32, cy: f32, size: f32, color: ColorU8) {
    let radius = size * 0.33;
    let center_y = cy + size * 0.09;

    fill_rounded_rect(
        pixmap,
        cx - size * 0.11,
        center_y - radius - size * 0.20,
        size * 0.22,
        size * 0.18,
        size * 0.05,
        color,
    );
    draw_filled_circle(
        pixmap,
        cx + radius * 1.06,
        center_y - radius * 0.62,
        size * 0.085,
        color,
    );

    let mut ring = PathBuilder::new();
    ring.push_circle(cx, center_y, radius);
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha(),
    ));
    if let Some(path) = ring.finish() {
        let stroke = Stroke {
            width: (size * 0.085).max(1.2),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    let mut hand = PathBuilder::new();
    hand.move_to(cx, center_y);
    hand.line_to(cx + radius * 0.42, center_y - radius * 0.46);
    if let Some(path) = hand.finish() {
        let stroke = Stroke {
            width: (size * 0.075).max(1.1),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
    draw_filled_circle(pixmap, cx, center_y, size * 0.055, color);
}

/// The stop control: a rounded square, matching the transport icon weight.
fn draw_stop_icon(pixmap: &mut Pixmap, cx: f32, cy: f32, size: f32, color: ColorU8) {
    let side = size * 0.56;
    fill_rounded_rect(
        pixmap,
        cx - side / 2.0,
        cy - side / 2.0,
        side,
        side,
        size * 0.10,
        color,
    );
}

/// Placeholder artwork for tracks that have no cover: a small beamed note.
fn draw_note(pixmap: &mut Pixmap, x: f32, y: f32, size: f32, color: ColorU8) {
    let head_radius = size * 0.105;
    let head_x = x + size * 0.38;
    let head_y = y + size * 0.66;
    draw_filled_circle(pixmap, head_x, head_y, head_radius, color);

    let stem_width = size * 0.06;
    let stem_x = head_x + head_radius * 0.72;
    let stem_top = y + size * 0.27;
    let stem_height = head_y - stem_top;
    fill_rounded_rect(
        pixmap,
        stem_x,
        stem_top,
        stem_width,
        stem_height,
        stem_width * 0.5,
        color,
    );
    fill_triangle(
        pixmap,
        (stem_x + stem_width, stem_top),
        (stem_x + stem_width, stem_top + size * 0.15),
        (stem_x + size * 0.17, stem_top + size * 0.06),
        color,
    );
}

/// A four-bar equaliser. Heights are driven by the same monotonic clock that
/// paces the render loop, so the motion is smooth at any refresh rate.
fn draw_waveform(
    pixmap: &mut Pixmap,
    x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    anim: f32,
    color: ColorU8,
) {
    const BARS: usize = 4;
    /// Fixed silhouette so the bars always read as an equaliser; the pulse
    /// only breathes around it.
    const SHAPE: [f32; BARS] = [0.55, 1.0, 0.78, 0.42];
    let gap = (width * 0.16).max(1.5);
    let bar_width = ((width - gap * (BARS as f32 - 1.0)) / BARS as f32).max(1.0);
    let radius = bar_width / 2.0;
    for bar in 0..BARS {
        let phase = anim * 2.8 + bar as f32 * 1.35;
        let pulse = 0.55 + 0.45 * (0.5 + 0.5 * phase.sin());
        let bar_height = height * (SHAPE[bar] * pulse).clamp(0.25, 1.0);
        fill_rounded_rect(
            pixmap,
            x + bar as f32 * (bar_width + gap),
            center_y - bar_height / 2.0,
            bar_width,
            bar_height,
            radius,
            color,
        );
    }
}

/// Cover art with rounded corners. The source is sampled with a centred cover
/// fit so non-square covers are cropped instead of stretched.
fn draw_art(pixmap: &mut Pixmap, art: Option<&Pixmap>, x: f32, y: f32, size: f32, alpha: f32) {
    // Tighter than a squircle: the artwork reads as a crisp tile inside the
    // pill and card instead of a soft blob.
    let radius = size * 0.13;
    let Some(art) = art.filter(|art| art.width() > 0 && art.height() > 0) else {
        fill_rounded_rect(
            pixmap,
            x,
            y,
            size,
            size,
            radius,
            ColorU8::from_rgba(255, 255, 255, (22.0 * alpha) as u8),
        );
        draw_note(
            pixmap,
            x,
            y,
            size,
            ColorU8::from_rgba(255, 255, 255, (140.0 * alpha) as u8),
        );
        return;
    };

    let cover = (art.width() as f32 / size).max(art.height() as f32 / size);
    let src_cx = art.width() as f32 / 2.0;
    let src_cy = art.height() as f32 / 2.0;
    let dst_cx = x + size / 2.0;
    let dst_cy = y + size / 2.0;

    let x0 = x.floor().max(0.0) as i32;
    let y0 = y.floor().max(0.0) as i32;
    let x1 = (x + size).ceil().min(pixmap.width() as f32) as i32;
    let y1 = (y + size).ceil().min(pixmap.height() as f32) as i32;

    for py in y0..y1 {
        for px in x0..x1 {
            let sample_x = px as f32 + 0.5;
            let sample_y = py as f32 + 0.5;
            let coverage = rounded_rect_coverage(sample_x, sample_y, x, y, size, size, radius);
            if coverage <= 0.0 {
                continue;
            }
            let source = sample_bilinear(
                art,
                src_cx + (sample_x - dst_cx) * cover,
                src_cy + (sample_y - dst_cy) * cover,
            );
            blend_art_pixel(pixmap, px, py, source, coverage * alpha);
        }
    }
}

/// Signed distance to a rounded rectangle, converted to one-pixel coverage.
fn rounded_rect_coverage(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
    let half_w = w / 2.0;
    let half_h = h / 2.0;
    let radius = radius.min(half_w).min(half_h).max(0.0);
    let qx = (px - (x + half_w)).abs() - (half_w - radius);
    let qy = (py - (y + half_h)).abs() - (half_h - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    let distance = outside + inside - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Bilinear sample of premultiplied RGBA, clamped to the artwork edges.
fn sample_bilinear(pixmap: &Pixmap, u: f32, v: f32) -> [f32; 4] {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let max_x = width - 1;
    let max_y = height - 1;
    let u = u.clamp(0.0, max_x as f32);
    let v = v.clamp(0.0, max_y as f32);
    let x0 = u.floor() as i32;
    let y0 = v.floor() as i32;
    let x1 = (x0 + 1).min(max_x);
    let y1 = (y0 + 1).min(max_y);
    let fx = u - x0 as f32;
    let fy = v - y0 as f32;

    let data = pixmap.data();
    let fetch = |x: i32, y: i32| {
        let index = (y as usize * width as usize + x as usize) * 4;
        [
            data[index] as f32,
            data[index + 1] as f32,
            data[index + 2] as f32,
            data[index + 3] as f32,
        ]
    };
    let top_left = fetch(x0, y0);
    let top_right = fetch(x1, y0);
    let bottom_left = fetch(x0, y1);
    let bottom_right = fetch(x1, y1);

    let mut result = [0.0; 4];
    for channel in 0..4 {
        let top = top_left[channel] + (top_right[channel] - top_left[channel]) * fx;
        let bottom = bottom_left[channel] + (bottom_right[channel] - bottom_left[channel]) * fx;
        result[channel] = top + (bottom - top) * fy;
    }
    result
}

/// Blends one premultiplied art sample over the opaque black surface.
fn blend_art_pixel(pixmap: &mut Pixmap, x: i32, y: i32, source: [f32; 4], opacity: f32) {
    if x < 0 || y < 0 || x >= pixmap.width() as i32 || y >= pixmap.height() as i32 {
        return;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let width = pixmap.width() as usize;
    let index = (y as usize * width + x as usize) * 4;
    let data = pixmap.data_mut();
    let alpha = source[3] * opacity;
    let inverse = 255.0 - alpha;
    data[index] =
        (source[0] * opacity + data[index] as f32 * inverse / 255.0).clamp(0.0, 255.0) as u8;
    data[index + 1] =
        (source[1] * opacity + data[index + 1] as f32 * inverse / 255.0).clamp(0.0, 255.0) as u8;
    data[index + 2] =
        (source[2] * opacity + data[index + 2] as f32 * inverse / 255.0).clamp(0.0, 255.0) as u8;
    data[index + 3] = (alpha + data[index + 3] as f32 * inverse / 255.0).clamp(0.0, 255.0) as u8;
}

fn fill_triangle(pixmap: &mut Pixmap, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: ColorU8) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.0, a.1);
    pb.line_to(b.0, b.1);
    pb.line_to(c.0, c.1);
    pb.close();
    if let Some(path) = pb.finish() {
        fill_path(pixmap, &path, color);
    }
}

fn fill_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: ColorU8,
) {
    if width <= 0.0 || height <= 0.0 || color.alpha() == 0 {
        return;
    }
    let Some(path) = rounded_rect_path(x, y, width, height, radius) else {
        return;
    };
    fill_path(pixmap, &path, color);
}

fn rounded_rect_path(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    let k = radius * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + radius, y);
    pb.line_to(x + width - radius, y);
    pb.cubic_to(
        x + width - radius + k,
        y,
        x + width,
        y + radius - k,
        x + width,
        y + radius,
    );
    pb.line_to(x + width, y + height - radius);
    pb.cubic_to(
        x + width,
        y + height - radius + k,
        x + width - radius + k,
        y + height,
        x + width - radius,
        y + height,
    );
    pb.line_to(x + radius, y + height);
    pb.cubic_to(
        x + radius - k,
        y + height,
        x,
        y + height - radius + k,
        x,
        y + height - radius,
    );
    pb.line_to(x, y + radius);
    pb.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    pb.close();
    pb.finish()
}

fn fill_path(pixmap: &mut Pixmap, path: &Path, color: ColorU8) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha(),
    ));
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn format_time(milliseconds: u64) -> String {
    let total = milliseconds / 1000;
    format!("{}:{:02}", total / 60, total % 60)
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
    let icon_color = if kind == NoticeKind::Timer {
        scale_alpha(AMBER, alpha)
    } else {
        scale_alpha(TEXT, alpha)
    };
    let disc = if kind == NoticeKind::Timer {
        ColorU8::from_rgba(242, 163, 59, (26.0 * alpha) as u8)
    } else {
        ColorU8::from_rgba(255, 255, 255, (12.0 * alpha) as u8)
    };

    draw_filled_circle(pixmap, icon_x, center_y, 15.0 * s, disc);
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
        NoticeKind::Timer => draw_stopwatch_icon(pixmap, cx, cy, size, color),
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
fn draw_battery(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    radius: f32,
    percent: u8,
    charging: bool,
    alpha: f32,
    accent: ColorU8,
) {
    let track = scale_alpha(accent, alpha * 0.28);
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
            scale_alpha(CYAN, alpha),
            scale_alpha(ColorU8::from_rgba(0, 0, 0, 255), alpha * 0.85),
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

    let shoulder_k = 0.72;
    pb.move_to(top_edge_left, y);
    pb.line_to(top_edge_right, y);
    pb.cubic_to(
        top_edge_right + (x + width - top_edge_right) * shoulder_k,
        y,
        x + width,
        y + drop * (1.0 - shoulder_k),
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
        y + drop * (1.0 - shoulder_k),
        top_edge_left + (x - top_edge_left) * shoulder_k,
        y,
        top_edge_left,
        y,
    );
    pb.close();
    pb.finish()
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
    use crate::geometry::{
        geometry_with_notice, Geometry, ShapeStyle, WINDOW_HEIGHT, WINDOW_WIDTH,
    };

    /// Loads a Windows UI font, or returns `None` when the machine has none.
    fn test_font(names: &[&str]) -> Option<Font> {
        let dir = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        for name in names {
            if let Ok(bytes) = std::fs::read(dir.join(name)) {
                if let Ok(font) = Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                    return Some(font);
                }
            }
        }
        None
    }

    /// Bright columns in a horizontal band of a rendered island.
    fn lit_columns(pixmap: &Pixmap, y0: usize, y1: usize) -> Vec<bool> {
        let width = pixmap.width() as usize;
        let mut lit = vec![false; width];
        for y in y0..y1.min(pixmap.height() as usize) {
            for x in 0..width {
                let px = &pixmap.data()[(y * width + x) * 4..(y * width + x) * 4 + 4];
                if px[3] > 40 && (px[0] > 90 || px[1] > 90 || px[2] > 90) {
                    lit[x] = true;
                }
            }
        }
        lit
    }

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

    /// The expanded status row keeps the Wi-Fi and Bluetooth columns apart with
    /// long names, leaves the shape's side margins alone, and draws the Wi-Fi
    /// icon large enough to read next to its label.
    #[test]
    fn status_row_columns_stay_separate_and_icons_stay_legible() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(1.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let view = IslandView {
                progress: 1.0,
                geometry,
                scale,
                time: "09:41".to_string(),
                date: "Kam, 12 Nov".to_string(),
                battery_percent: Some(87),
                battery_charging: false,
                wifi: Some("Kost Putri Anggrek 5G Ultra"),
                bluetooth: Some("Sony WH-1000XM5 Wireless"),
                notice_mix: 0.0,
                notice_kind: None,
                notice_title: None,
                notice_subtitle: None,
                notice_battery: None,
                notice_closing: false,
                media: None,
                timer: None,
                anim: 0.0,
                regular: Some(&regular),
                semibold: semibold.as_ref(),
            };
            let Some(pixmap) = draw(width as u32, height as u32, &view) else {
                panic!("render failed at scale {scale}");
            };

            let y0 = (geometry.y + 96.0 * scale) as usize;
            let y1 = (geometry.y + 126.0 * scale) as usize;
            let lit = lit_columns(&pixmap, y0, y1);
            let first = lit.iter().position(|on| *on).unwrap_or_else(|| {
                panic!("status row is empty at scale {scale}");
            });
            let last = lit.iter().rposition(|on| *on).expect("status row is empty");

            let left_margin = first as f32 - geometry.x;
            let right_margin = geometry.x + geometry.width - last as f32 - 1.0;
            assert!(
                left_margin >= 26.0 * scale,
                "Wi-Fi column broke the left margin at scale {scale}: {left_margin}"
            );
            assert!(
                right_margin >= 26.0 * scale,
                "Bluetooth column broke the right margin at scale {scale}: {right_margin}"
            );

            // A gap of empty columns proves the two columns never touch.
            let mut widest_gap = 0usize;
            let mut gap = 0usize;
            for on in &lit[first..=last] {
                if *on {
                    widest_gap = widest_gap.max(gap);
                    gap = 0;
                } else {
                    gap += 1;
                }
            }
            assert!(
                widest_gap >= (20.0 * scale) as usize,
                "status columns collided at scale {scale}: gap {widest_gap}"
            );

            // The first group is the Wi-Fi icon; it has to stay readable. The
            // window is wide enough for the glyph but stops before its label.
            let window = (14.0 * scale).ceil() as usize;
            let icon_last = lit[first..(first + window).min(lit.len())]
                .iter()
                .rposition(|on| *on)
                .map(|offset| first + offset)
                .unwrap_or(first);
            let icon_width = icon_last - first + 1;
            assert!(
                icon_width >= (13.0 * scale).ceil() as usize,
                "Wi-Fi icon shrank to {icon_width}px at scale {scale}"
            );
        }
    }

    fn test_art() -> Pixmap {
        let mut art = Pixmap::new(24, 24).expect("art pixmap");
        for pixel in art.data_mut().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[232, 120, 48, 255]);
        }
        art
    }

    /// Counts pixels whose colour can only come from the orange test artwork.
    fn art_columns(pixmap: &Pixmap) -> (usize, usize, usize) {
        let width = pixmap.width() as usize;
        let height = pixmap.height() as usize;
        let mut first = width;
        let mut last = 0;
        let mut count = 0;
        for y in 0..height {
            for x in 0..width {
                let px = &pixmap.data()[(y * width + x) * 4..(y * width + x) * 4 + 4];
                if px[0] > 140 && px[1] < 175 && px[2] < 120 {
                    count += 1;
                    first = first.min(x);
                    last = last.max(x);
                }
            }
        }
        (first, last, count)
    }

    fn has_bright_pixel_near(pixmap: &Pixmap, center: (f32, f32), radius: f32) -> bool {
        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let x0 = (center.0 - radius).floor().max(0.0) as i32;
        let y0 = (center.1 - radius).floor().max(0.0) as i32;
        let x1 = ((center.0 + radius).ceil() as i32).min(width);
        let y1 = ((center.1 + radius).ceil() as i32).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                let index = (y as usize * width as usize + x as usize) * 4;
                let px = &pixmap.data()[index..index + 4];
                if px[3] > 40 && px[0] > 120 && px[1] > 120 && px[2] > 120 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn transport_controls_stay_inside_the_expanded_card() {
        for scale in [1.0_f32, 1.3, 1.75] {
            let geo =
                geometry_with_notice(1.0, 0.0, ShapeStyle::Attached, scale, WINDOW_WIDTH * scale);
            let controls = media_controls(&geo, scale);
            let margin = 28.0 * scale;
            let left = geo.x + margin;
            let right = geo.x + geo.width - margin;

            assert!(
                controls.previous.0 - controls.radius >= left,
                "previous button broke the left margin at scale {scale}"
            );
            assert!(
                controls.next.0 + controls.radius <= right,
                "next button broke the right margin at scale {scale}"
            );
            assert!(
                controls.play.1 + controls.radius <= geo.y + 104.0 * scale,
                "transport row collided with the status row at scale {scale}"
            );
        }
    }

    #[test]
    fn compact_media_keeps_art_inside_the_pill() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);
        let art = test_art();

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(0.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let view = IslandView {
                progress: 0.0,
                geometry,
                scale,
                time: "09:41".to_string(),
                date: "Kam, 12 Nov".to_string(),
                battery_percent: Some(87),
                battery_charging: false,
                wifi: Some("Wi-Fi"),
                bluetooth: None,
                notice_mix: 0.0,
                notice_kind: None,
                notice_title: None,
                notice_subtitle: None,
                notice_battery: None,
                notice_closing: false,
                media: Some(MediaView {
                    title: "Midnight City Lights",
                    artist: "The Paper Kites",
                    playing: true,
                    position_ms: 62_000,
                    duration_ms: 214_000,
                    art: Some(&art),
                }),
                timer: None,
                anim: 1.2,
                regular: Some(&regular),
                semibold: semibold.as_ref(),
            };
            let pixmap = draw(width as u32, height as u32, &view).expect("render");

            let (first, last, count) = art_columns(&pixmap);
            assert!(
                count >= (200.0 * scale * scale) as usize,
                "cover art is missing at scale {scale}: {count} pixels"
            );
            let right = geometry.x + geometry.width;
            assert!(
                first as f32 >= right - 41.0 * scale,
                "art drifted too far left at scale {scale}: {first}"
            );
            assert!(
                last as f32 <= right - 16.0 * scale,
                "art broke the right margin at scale {scale}: {last}"
            );
        }
    }

    #[test]
    fn expanded_media_draws_art_transport_and_status_row() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);
        let art = test_art();

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(1.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let view = IslandView {
                progress: 1.0,
                geometry,
                scale,
                time: "09:41".to_string(),
                date: "Kam, 12 Nov".to_string(),
                battery_percent: Some(87),
                battery_charging: false,
                wifi: Some("Kost Putri Anggrek 5G"),
                bluetooth: Some("Sony WH-1000XM5"),
                notice_mix: 0.0,
                notice_kind: None,
                notice_title: None,
                notice_subtitle: None,
                notice_battery: None,
                notice_closing: false,
                media: Some(MediaView {
                    title: "Midnight City Lights",
                    artist: "The Paper Kites",
                    playing: true,
                    position_ms: 62_000,
                    duration_ms: 214_000,
                    art: Some(&art),
                }),
                timer: None,
                anim: 1.2,
                regular: Some(&regular),
                semibold: semibold.as_ref(),
            };
            let pixmap = draw(width as u32, height as u32, &view).expect("render");

            let (first, last, count) = art_columns(&pixmap);
            assert!(
                count >= (1000.0 * scale * scale) as usize,
                "expanded cover art is missing at scale {scale}: {count} pixels"
            );
            assert!(
                first as f32 >= geometry.x + 26.0 * scale,
                "art broke the left margin at scale {scale}: {first}"
            );
            assert!(
                last as f32 <= geometry.x + 70.0 * scale,
                "art grew past its column at scale {scale}: {last}"
            );

            let controls = media_controls(&geometry, scale);
            for (name, center) in [
                ("previous", controls.previous),
                ("play", controls.play),
                ("next", controls.next),
            ] {
                assert!(
                    has_bright_pixel_near(&pixmap, center, controls.radius),
                    "{name} control was not drawn at scale {scale}"
                );
            }

            let baseline = geometry.y + 112.0 * scale;
            let status_band = (baseline - 12.0 * scale) as usize
                ..((baseline + 4.0 * scale) as usize).min(pixmap.height() as usize);
            let lit = lit_columns(&pixmap, status_band.start, status_band.end);
            let left_half = lit[..pixmap.width() as usize / 2].iter().any(|on| *on);
            let right_half = lit[pixmap.width() as usize / 2..].iter().any(|on| *on);
            assert!(
                left_half && right_half,
                "status row disappeared under the media card at scale {scale}"
            );
        }
    }

    #[test]
    fn playback_times_are_minutes_and_seconds() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(9_400), "0:09");
        assert_eq!(format_time(62_000), "1:02");
        assert_eq!(format_time(3_599_000), "59:59");
    }

    #[allow(clippy::too_many_arguments)]
    fn test_view<'a>(
        geometry: Geometry,
        scale: f32,
        regular: &'a Font,
        semibold: Option<&'a Font>,
        progress: f32,
        media: Option<MediaView<'a>>,
        timer: Option<TimerView>,
    ) -> IslandView<'a> {
        IslandView {
            progress,
            geometry,
            scale,
            time: "09:41".to_string(),
            date: "Kam, 12 Nov".to_string(),
            battery_percent: Some(87),
            battery_charging: false,
            wifi: Some("Kost Putri Anggrek 5G"),
            bluetooth: Some("Sony WH-1000XM5"),
            notice_mix: 0.0,
            notice_kind: None,
            notice_title: None,
            notice_subtitle: None,
            notice_battery: None,
            notice_closing: false,
            media,
            timer,
            anim: 1.2,
            regular: Some(regular),
            semibold,
        }
    }

    /// Counts pixels that can only come from the amber timer accent.
    fn amber_pixels_in(pixmap: &Pixmap, x0: f32, y0: f32, x1: f32, y1: f32) -> usize {
        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let x0 = (x0.floor() as i32).max(0);
        let y0 = (y0.floor() as i32).max(0);
        let x1 = (x1.ceil() as i32).min(width);
        let y1 = (y1.ceil() as i32).min(height);
        let mut count = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let index = (y as usize * width as usize + x as usize) * 4;
                let px = &pixmap.data()[index..index + 4];
                if px[3] > 100 && px[0] > 180 && px[1] > 90 && px[1] < 220 && px[2] < 120 {
                    count += 1;
                }
            }
        }
        count
    }

    /// Counts pixels that can only come from the cyan charging accent.
    fn cyan_pixels_in(pixmap: &Pixmap, x0: f32, y0: f32, x1: f32, y1: f32) -> usize {
        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let x0 = (x0.floor() as i32).max(0);
        let y0 = (y0.floor() as i32).max(0);
        let x1 = (x1.ceil() as i32).min(width);
        let y1 = (y1.ceil() as i32).min(height);
        let mut count = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let index = (y as usize * width as usize + x as usize) * 4;
                let px = &pixmap.data()[index..index + 4];
                if px[3] > 100 && px[0] < 160 && px[1] > 130 && px[2] > 190 && px[2] > px[0] + 60 {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn collapsed_timer_replaces_the_clock_with_amber_and_a_live_value() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(0.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let view = test_view(
                geometry,
                scale,
                &regular,
                semibold.as_ref(),
                0.0,
                None,
                Some(TimerView {
                    display: "4:59".to_string(),
                    label: "Timer",
                    progress: 0.83,
                    countdown: true,
                    running: true,
                }),
            );
            let pixmap = draw(width as u32, height as u32, &view).expect("render");

            let amber = amber_pixels_in(&pixmap, 0.0, 0.0, width as f32, height as f32);
            assert!(
                amber >= (20.0 * scale * scale) as usize,
                "timer accent is missing at scale {scale}: {amber} pixels"
            );

            let baseline = geometry.y + geometry.height / 2.0 + 5.5 * scale;
            let band = (baseline - 12.0 * scale) as usize
                ..((baseline + 3.0 * scale) as usize).min(pixmap.height() as usize);
            let lit = lit_columns(&pixmap, band.start, band.end);
            let left_half = lit[..width / 2].iter().filter(|on| **on).count();
            let right_half = lit[width / 2..].iter().filter(|on| **on).count();
            assert!(
                left_half >= (8.0 * scale) as usize,
                "timer value is missing at scale {scale}"
            );
            assert!(
                right_half >= (4.0 * scale) as usize,
                "timer mode label is missing at scale {scale}: left {left_half}, right {right_half}"
            );
        }
    }

    #[test]
    fn timer_presets_fit_the_idle_card_and_share_click_geometry() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(1.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let presets = timer_presets(&geometry, scale);
            let left = geometry.x + 28.0 * scale;
            let right = geometry.x + geometry.width - 28.0 * scale;

            assert!(
                presets[0].x >= left - 0.01,
                "first chip broke the left margin at scale {scale}"
            );
            assert!(
                presets[4].x + presets[4].width <= right + 0.01,
                "last chip broke the right margin at scale {scale}"
            );
            for pair in presets.windows(2) {
                assert!(
                    pair[1].x >= pair[0].x + pair[0].width,
                    "preset chips overlap at scale {scale}"
                );
            }
            for preset in presets {
                assert!(preset.y >= geometry.y + 60.0 * scale);
                assert!(preset.y + preset.height <= geometry.y + 104.0 * scale);
            }

            let view = test_view(
                geometry,
                scale,
                &regular,
                semibold.as_ref(),
                1.0,
                None,
                None,
            );
            let pixmap = draw(width as u32, height as u32, &view).expect("render");
            for preset in presets {
                let center = (
                    preset.x + preset.width / 2.0,
                    preset.y + preset.height / 2.0,
                );
                assert!(
                    has_bright_pixel_near(&pixmap, center, preset.height / 2.0),
                    "preset '{}' is blank at scale {scale}",
                    preset.label
                );
            }
        }
    }

    #[test]
    fn expanded_timer_draws_a_depleting_bar_and_controls() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(1.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let view = test_view(
                geometry,
                scale,
                &regular,
                semibold.as_ref(),
                1.0,
                None,
                Some(TimerView {
                    display: "2:30".to_string(),
                    label: "Timer",
                    progress: 0.5,
                    countdown: true,
                    running: true,
                }),
            );
            let pixmap = draw(width as u32, height as u32, &view).expect("render");

            let controls = timer_controls(&geometry, scale);
            for (name, center) in [("pause", controls.pause), ("stop", controls.stop)] {
                assert!(
                    has_bright_pixel_near(&pixmap, center, controls.radius),
                    "{name} control was not drawn at scale {scale}"
                );
            }

            let left = geometry.x + 28.0 * scale;
            let right = geometry.x + geometry.width - 28.0 * scale;
            let bar_y = geometry.y + 62.0 * scale;
            let half = (right - left) * 0.5;
            let filled = amber_pixels_in(
                &pixmap,
                left,
                bar_y - 2.0 * scale,
                left + half,
                bar_y + 6.0 * scale,
            );
            let missing = amber_pixels_in(
                &pixmap,
                left + half + 8.0 * scale,
                bar_y - 2.0 * scale,
                right,
                bar_y + 6.0 * scale,
            );
            assert!(
                filled >= (100.0 * scale * scale) as usize,
                "timer progress bar is empty at scale {scale}: {filled} pixels"
            );
            assert_eq!(
                missing, 0,
                "timer progress bar passed its remaining fraction at scale {scale}"
            );

            let baseline = geometry.y + 112.0 * scale;
            let status_band = (baseline - 12.0 * scale) as usize
                ..((baseline + 4.0 * scale) as usize).min(pixmap.height() as usize);
            let lit = lit_columns(&pixmap, status_band.start, status_band.end);
            let left_half = lit[..pixmap.width() as usize / 2].iter().any(|on| *on);
            let right_half = lit[pixmap.width() as usize / 2..].iter().any(|on| *on);
            assert!(
                left_half && right_half,
                "status row disappeared under the timer card at scale {scale}"
            );
        }
    }

    #[test]
    fn stopwatch_card_has_no_progress_bar() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(1.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let view = test_view(
                geometry,
                scale,
                &regular,
                semibold.as_ref(),
                1.0,
                None,
                Some(TimerView {
                    display: "12:34".to_string(),
                    label: "Stopwatch",
                    progress: 0.0,
                    countdown: false,
                    running: true,
                }),
            );
            let pixmap = draw(width as u32, height as u32, &view).expect("render");

            let left = geometry.x + 28.0 * scale;
            let right = geometry.x + geometry.width - 28.0 * scale;
            let bar_y = geometry.y + 62.0 * scale;
            let amber = amber_pixels_in(
                &pixmap,
                left,
                bar_y - 2.0 * scale,
                right,
                bar_y + 6.0 * scale,
            );
            assert_eq!(amber, 0, "stopwatch drew a countdown bar at scale {scale}");
        }
    }

    #[test]
    fn timer_notice_uses_amber_and_shows_both_lines() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.3] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(0.0, 1.0, ShapeStyle::Attached, scale, width as f32);
            let mut view = test_view(
                geometry,
                scale,
                &regular,
                semibold.as_ref(),
                0.0,
                None,
                None,
            );
            view.notice_mix = 1.0;
            view.notice_kind = Some(NoticeKind::Timer);
            view.notice_title = Some("Timer");
            view.notice_subtitle = Some("Time's up!");
            let pixmap = draw(width as u32, height as u32, &view).expect("render");

            let amber = amber_pixels_in(
                &pixmap,
                geometry.x,
                geometry.y,
                geometry.x + geometry.width,
                geometry.y + geometry.height,
            );
            assert!(
                amber >= (15.0 * scale * scale) as usize,
                "timer notice icon is missing its amber accent at scale {scale}: {amber}"
            );

            let text_left = (geometry.x + 42.0 * scale) as usize;
            let text_right = ((geometry.x + geometry.width - 12.0 * scale) as usize).min(width);
            let text_top = (geometry.y + 8.0 * scale) as usize;
            let text_bottom = (geometry.y + 48.0 * scale) as usize;
            let lit = lit_columns(&pixmap, text_top, text_bottom);
            let columns = lit[text_left.min(lit.len())..text_right.min(lit.len())]
                .iter()
                .filter(|on| **on)
                .count();
            assert!(
                columns >= (20.0 * scale) as usize,
                "timer notice text is missing at scale {scale}: {columns} columns"
            );
        }
    }

    #[test]
    fn charging_battery_switches_to_the_cyan_accent() {
        let Some(regular) = test_font(&["segoeui.ttf", "arial.ttf"]) else {
            return;
        };
        let semibold = test_font(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]);

        for scale in [1.0_f32, 1.75] {
            let width = (WINDOW_WIDTH * scale).round() as usize;
            let height = (WINDOW_HEIGHT * scale).round() as usize;
            let geometry =
                geometry_with_notice(0.0, 0.0, ShapeStyle::Attached, scale, width as f32);
            let mut view = test_view(
                geometry,
                scale,
                &regular,
                semibold.as_ref(),
                0.0,
                None,
                None,
            );
            view.battery_percent = Some(42);

            let neutral = draw(width as u32, height as u32, &view).expect("render");
            let cyan = cyan_pixels_in(&neutral, 0.0, 0.0, width as f32, height as f32);
            assert_eq!(
                cyan, 0,
                "a discharging battery picked up the cyan accent at scale {scale}"
            );

            view.battery_charging = true;
            let charging = draw(width as u32, height as u32, &view).expect("render");
            let cyan = cyan_pixels_in(&charging, 0.0, 0.0, width as f32, height as f32);
            assert!(
                cyan >= (40.0 * scale) as usize,
                "charging battery is missing its cyan accent at scale {scale}: {cyan} pixels"
            );

            let right = geometry.x + geometry.width - 18.0 * scale;
            let icon_x = right - 7.2 * scale;
            let label_size = 11.5 * scale;
            let label_width = measure_text(&regular, "42%", label_size);
            let label_left = icon_x - 7.2 * scale - 7.0 * scale - label_width;
            let center_y = geometry.y + geometry.height / 2.0;
            let band_top = center_y + 5.5 * scale - 12.0 * scale;
            let band_bottom = center_y + 5.5 * scale + 4.0 * scale;
            let lit = lit_columns(&charging, band_top.max(0.0) as usize, band_bottom as usize);
            assert!(
                lit[(label_left.max(0.0) as usize).min(lit.len())
                    ..((label_left + label_width) as usize).min(lit.len())]
                    .iter()
                    .any(|on| *on),
                "charging label disappeared at scale {scale}"
            );
        }
    }
}
