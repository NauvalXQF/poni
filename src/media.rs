use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tiny_skia::Pixmap;
use windows::core::Interface;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStream, IRandomAccessStreamReference};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Album art is decoded once per track at this edge so the renderer can scale
/// it down to the pill or the expanded card without touching the decoder.
const ART_EDGE: u32 = 128;
const ART_BYTE_LIMIT: usize = 8 * 1024 * 1024;

/// Actions the UI can ask the media session to perform. They travel over a
/// channel so the async WinRT calls never run on the render thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
}

#[derive(Clone, Default)]
pub struct MediaState {
    pub active: bool,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub art: Option<Arc<Pixmap>>,
    /// Bumped whenever the artwork changes so the UI can detect a new cover
    /// without comparing pixels.
    pub art_rev: u64,
}

impl MediaState {
    /// True when anything the UI draws has changed. Position is included so an
    /// expanded progress bar keeps moving.
    pub fn differs(&self, other: &Self) -> bool {
        self.active != other.active
            || self.title != other.title
            || self.artist != other.artist
            || self.playing != other.playing
            || self.art_rev != other.art_rev
            || self.position_ms != other.position_ms
            || self.duration_ms != other.duration_ms
    }
}

/// Watches the Windows media session on its own thread. The UI only drains the
/// receiver, so a slow media app can never stall the animation loop.
pub fn spawn_worker() -> (Receiver<MediaState>, Sender<MediaCommand>) {
    let (state_tx, state_rx) = mpsc::channel();
    let (command_tx, command_rx) = mpsc::channel();

    thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let mut manager = request_manager();
        let mut cache = ArtCache::default();
        // After a click the player can take a moment to report the new state;
        // poll quickly for a short burst so the icon flips without lag.
        let mut quick_polls = 0u32;

        loop {
            let state = manager
                .as_ref()
                .map(|manager| read_state(manager, &mut cache))
                .unwrap_or_default();
            let playing = state.playing;
            if state_tx.send(state).is_err() {
                break;
            }

            // Shorter cadence while music plays so the progress bar and the
            // waveform stay live; slower when paused or idle.
            let timeout = if quick_polls > 0 {
                quick_polls -= 1;
                150
            } else if playing {
                250
            } else {
                900
            };
            match command_rx.recv_timeout(Duration::from_millis(timeout)) {
                Ok(command) => {
                    quick_polls = 8;
                    if manager.is_none() {
                        manager = request_manager();
                    }
                    if let Some(manager) = manager.as_ref() {
                        apply_command(manager, command);
                    }
                    while let Ok(command) = command_rx.try_recv() {
                        if let Some(manager) = manager.as_ref() {
                            apply_command(manager, command);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    (state_rx, command_tx)
}

fn request_manager() -> Option<GlobalSystemMediaTransportControlsSessionManager> {
    GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .get()
        .ok()
}

fn read_state(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    cache: &mut ArtCache,
) -> MediaState {
    let Ok(session) = manager.GetCurrentSession() else {
        cache.reset();
        return MediaState::default();
    };

    let Some(properties) = session
        .TryGetMediaPropertiesAsync()
        .ok()
        .and_then(|operation| operation.get().ok())
    else {
        cache.reset();
        return MediaState::default();
    };

    let title = properties
        .Title()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let artist = properties
        .Artist()
        .map(|value| value.to_string())
        .unwrap_or_default();

    if title.trim().is_empty() {
        cache.reset();
        return MediaState::default();
    }

    let playing = session
        .GetPlaybackInfo()
        .ok()
        .and_then(|info| info.PlaybackStatus().ok())
        == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);

    let (position_ms, duration_ms) = session
        .GetTimelineProperties()
        .ok()
        .map(|timeline| timeline_bounds(&timeline))
        .unwrap_or((0, 0));

    let art_rev = cache.sync(&title, &artist, properties.Thumbnail().ok());

    MediaState {
        active: true,
        title,
        artist,
        playing,
        position_ms,
        duration_ms,
        art: cache.art.clone(),
        art_rev,
    }
}

fn timeline_bounds(
    timeline: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionTimelineProperties,
) -> (u64, u64) {
    let ticks = |value: windows::Foundation::TimeSpan| value.Duration.max(0) as u64 / 10_000;
    let start = timeline.StartTime().map(ticks).unwrap_or(0);
    let position = timeline.Position().map(ticks).unwrap_or(0);
    let end = timeline.EndTime().map(ticks).unwrap_or(0);
    (position.saturating_sub(start), end.saturating_sub(start))
}

fn apply_command(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    command: MediaCommand,
) {
    let Ok(session) = manager.GetCurrentSession() else {
        return;
    };
    let _ = match command {
        MediaCommand::PlayPause => session
            .TryTogglePlayPauseAsync()
            .and_then(|operation| operation.get()),
        MediaCommand::Next => session
            .TrySkipNextAsync()
            .and_then(|operation| operation.get()),
        MediaCommand::Previous => session
            .TrySkipPreviousAsync()
            .and_then(|operation| operation.get()),
    };
}

#[derive(Default)]
struct ArtCache {
    key: String,
    art: Option<Arc<Pixmap>>,
    rev: u64,
}

impl ArtCache {
    fn reset(&mut self) {
        if self.art.is_some() || !self.key.is_empty() {
            self.key.clear();
            self.art = None;
            self.rev = self.rev.wrapping_add(1);
        }
    }

    /// Decodes the cover only when the track changes, so polling stays cheap.
    fn sync(
        &mut self,
        title: &str,
        artist: &str,
        thumbnail: Option<IRandomAccessStreamReference>,
    ) -> u64 {
        let key = format!("{title}\u{1f}{artist}");
        if key == self.key {
            return self.rev;
        }

        let decoded = thumbnail
            .and_then(|reference| read_thumbnail(&reference))
            .and_then(|bytes| decode_art(&bytes));

        self.key = key;
        self.art = decoded.map(Arc::new);
        self.rev = self.rev.wrapping_add(1);
        self.rev
    }
}

fn read_thumbnail(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as usize;
    if size == 0 || size > ART_BYTE_LIMIT {
        return None;
    }

    let stream: IRandomAccessStream = stream.cast().ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size as u32).ok()?.get().ok()?;

    let mut bytes = vec![0u8; size];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

/// Decodes and shrinks cover art, then converts it to the premultiplied RGBA
/// that tiny-skia composites.
fn decode_art(bytes: &[u8]) -> Option<Pixmap> {
    let decoded = image::load_from_memory(bytes).ok()?;
    // Windows thumbnails are rarely square. Centre-crop first so covers are
    // not stretched when the island samples them into a square slot.
    let edge = decoded.width().min(decoded.height());
    let cropped = decoded.crop_imm(
        (decoded.width() - edge) / 2,
        (decoded.height() - edge) / 2,
        edge,
        edge,
    );
    let resized = cropped.resize(ART_EDGE, ART_EDGE, image::imageops::FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut pixmap = Pixmap::new(width, height)?;

    let data = pixmap.data_mut();
    for (index, pixel) in rgba.pixels().enumerate() {
        let [red, green, blue, alpha] = pixel.0;
        let alpha = alpha as u32;
        data[index * 4] = ((red as u32 * alpha) / 255) as u8;
        data[index * 4 + 1] = ((green as u32 * alpha) / 255) as u8;
        data[index * 4 + 2] = ((blue as u32 * alpha) / 255) as u8;
        data[index * 4 + 3] = alpha as u8;
    }

    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    #[test]
    fn identical_states_do_not_force_a_redraw() {
        let state = MediaState {
            active: true,
            title: "Song".into(),
            artist: "Artist".into(),
            playing: true,
            position_ms: 1000,
            duration_ms: 200_000,
            art: None,
            art_rev: 1,
        };
        assert!(!state.differs(&state.clone()));
    }

    #[test]
    fn a_new_track_or_progress_forces_a_redraw() {
        let state = MediaState {
            active: true,
            title: "Song".into(),
            artist: "Artist".into(),
            playing: true,
            position_ms: 1000,
            duration_ms: 200_000,
            art: None,
            art_rev: 1,
        };

        let mut renamed = state.clone();
        renamed.title = "Another".into();
        assert!(state.differs(&renamed));

        let mut advanced = state.clone();
        advanced.position_ms = 1250;
        assert!(state.differs(&advanced));

        let mut stopped = state.clone();
        stopped.playing = false;
        assert!(state.differs(&stopped));

        let mut new_art = state.clone();
        new_art.art_rev = 2;
        assert!(state.differs(&new_art));
    }

    #[test]
    fn decoded_art_is_premultiplied_and_fits_the_cache_edge() {
        let mut source = RgbaImage::new(256, 128);
        for (x, _, pixel) in source.enumerate_pixels_mut() {
            let alpha = if x % 2 == 0 { 255 } else { 128 };
            *pixel = Rgba([200, 100, 50, alpha]);
        }

        let mut png = Cursor::new(Vec::new());
        source.write_to(&mut png, ImageFormat::Png).unwrap();

        let pixmap = decode_art(png.get_ref()).expect("decode should succeed");
        assert!(pixmap.width() <= ART_EDGE);
        assert!(pixmap.height() <= ART_EDGE);
        assert_eq!(pixmap.width(), pixmap.height(), "cover art stays square");
        assert!(pixmap.width() > 0 && pixmap.height() > 0);

        // Every pixel is premultiplied: colour never exceeds its alpha.
        for chunk in pixmap.data().chunks_exact(4) {
            let alpha = chunk[3];
            assert!(chunk[0] <= alpha && chunk[1] <= alpha && chunk[2] <= alpha);
        }
    }

    #[test]
    fn garbage_bytes_do_not_panic() {
        assert!(decode_art(b"not an image").is_none());
        assert!(decode_art(&[]).is_none());
    }
}
