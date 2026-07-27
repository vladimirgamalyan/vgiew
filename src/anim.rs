// Animated GIF, APNG and WebP (ADR 0019). One engine written against `image`'s
// `AnimationDecoder`, so the three containers differ only in how an animation is detected;
// each frame arrives as a full canvas in RGBA, composition and disposal already applied.
//
// Frames are decoded on a worker thread into a ring buffer bounded in bytes: an animation
// that fits stays wholly resident and loops for free, one that does not streams and re-opens
// the file at each wrap — the only way back to frame 0, since frames are cumulative.
//
// Like `svg`, this module knows nothing about windows, views or the framebuffer's
// conventions beyond the pixel format. Deciding what to draw, and when, stays in main.rs.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, Frames, ImageFormat};
use winit::event_loop::EventLoopProxy;

use crate::{pack_rgba, DecodedImage, UserEvent};

// How many bytes of decoded frames are held at once. This is not "how much memory do we
// allow" but "where does the line fall between an animation that loops for free and one that
// re-decodes every round": 256 MB holds 256 frames of 500x500 — a typical meme several times
// over — while a 1080p screencast streams (ADR 0019).
const BUDGET: usize = 256 * 1024 * 1024;

// A delay of `FAST` or less is what a GIF author writes for "as fast as possible", and
// `image` passes it through unnormalised. Taken literally it would spin the timer as fast as
// the CPU allows, so it becomes `SLOW` instead — what every browser does, and therefore the
// speed such files were actually authored against (ADR 0019 point 7).
const FAST_DELAY: Duration = Duration::from_millis(10);
const SLOW_DELAY: Duration = Duration::from_millis(100);

/// An animated file. Only the file on screen has a decoder and a buffer; every other one in
/// the cache is just its first frame, so a prefetched neighbour costs no more than a still
/// image does (ADR 0019 point 10).
pub struct Anim {
    path: PathBuf,
    /// Frame 0, kept for as long as the file is cached: it is what a neighbour shows, and
    /// what the file rewinds to when it stops being current.
    first: Arc<DecodedImage>,
    /// The frame on screen. Never absent, so there is always something to draw — including
    /// while a streaming animation stalls at a loop wrap.
    shown: Arc<DecodedImage>,
    /// Frames played since playback began. It does not wrap, which is what lets the deadline
    /// be computed from one fixed origin however long the file has been looping.
    seq: u64,
    /// When `shown` fell due, measured from that origin.
    at: Duration,
    play: Option<Player>,
}

// The ring buffer as the two threads see it: the condvar carries both directions, the decoder
// waking on a playback step and the viewer on a frame it was waiting for.
type Shared = (Mutex<Ring>, Condvar);

struct Player {
    ring: Arc<Shared>,
    /// The wall-clock origin: the frame `seq` steps in is due at `start + at`. Deadlines are
    /// computed from this rather than by adding a delay per redraw, so a frame that overruns
    /// its delay does not shift the whole animation later (ADR 0019 point 6).
    start: Instant,
}

impl Drop for Player {
    fn drop(&mut self) {
        let (lock, cv) = &*self.ring;
        if let Ok(mut ring) = lock.lock() {
            ring.stop = true;
        }
        cv.notify_all();
    }
}

/// Reads `path` as an animation, or None if it does not hold one — which covers every format
/// that cannot animate, a PNG or WebP whose header says it does not, and a GIF with a single
/// frame. The caller then decodes it as the still image it is.
pub fn open(path: &Path) -> Option<Anim> {
    let mut frames = frames(path)?;
    let first = pack_rgba(frames.next()?.ok()?.buffer());
    // A second frame is what separates an animation from a still that merely sits in a
    // container able to hold one. For a GIF this is the only test there is; for APNG and
    // WebP the header flag has already narrowed it down, and this catches the leftovers —
    // an APNG whose sole frame is its own default image among them (ADR 0019 point 2).
    frames.next()?.ok()?;
    Some(Anim::new(path, first))
}

impl Anim {
    fn new(path: &Path, first: DecodedImage) -> Anim {
        let first = Arc::new(first);
        Anim {
            path: path.to_path_buf(),
            shown: first.clone(),
            first,
            seq: 0,
            at: Duration::ZERO,
            play: None,
        }
    }

    /// The canvas size, which every frame shares.
    pub fn size(&self) -> (u32, u32) {
        (self.first.w, self.first.h)
    }

    /// The frame on screen: what the renderer draws and what `Ctrl+C` copies.
    pub fn image(&self) -> &DecodedImage {
        &self.shown
    }

    /// Frames in one loop, known once the decoder has been through the file once. None until
    /// then, which for anything but a very long animation is a fraction of a second.
    pub fn total(&self) -> Option<u64> {
        self.play.as_ref()?.ring.0.lock().unwrap().total
    }

    /// Which frame of the animation is on screen, counting from zero.
    pub fn index(&self) -> u64 {
        match self.total() {
            Some(n) => self.seq % n,
            None => self.seq,
        }
    }

    /// Starts the decoder and the clock. Idempotent, so the paths that merely re-assert what
    /// is current do not restart an animation that is already running.
    pub fn play(&mut self, now: Instant, proxy: &EventLoopProxy<UserEvent>) {
        if self.play.is_some() {
            return;
        }
        let ring = Arc::new((Mutex::new(Ring::new()), Condvar::new()));
        let (path, shared, proxy) = (self.path.clone(), ring.clone(), proxy.clone());
        std::thread::spawn(move || decode(&path, &shared, &proxy));
        self.play = Some(Player { ring, start: now });
        self.rewind();
    }

    /// Tears the decoder down and rewinds to frame 0. Returning to the file starts it over,
    /// because a file that is not current keeps neither a position nor any frame to resume
    /// from (ADR 0020 point 4).
    pub fn stop(&mut self) {
        self.play = None;
        self.rewind();
    }

    fn rewind(&mut self) {
        self.seq = 0;
        self.at = Duration::ZERO;
        self.shown = self.first.clone();
    }

    /// Advances to the frame due at `now`. Frames whose moment has already passed are skipped
    /// rather than shown late; they were still decoded, because the format offers no way to
    /// skip them. Returns whether what is on screen changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some((ring, start)) = self.player() else {
            return false;
        };
        let (lock, cv) = &*ring;
        let mut buf = lock.lock().unwrap();
        let mut changed = false;
        // Stops on the first frame that is not due yet, and equally on one the decoder has
        // not produced — an underrun holds the current frame and lets the cadence sag, which
        // is the only behaviour a cumulative format allows (ADR 0019 point 8).
        while let Some((frame, at)) = buf.due(self.seq + 1) {
            if start + at > now {
                break;
            }
            self.shown = frame.img.clone();
            self.at = at;
            self.seq += 1;
            changed = true;
        }
        if changed {
            buf.pos = self.seq;
            cv.notify_all(); // frames behind the new position are free to be dropped
        }
        changed
    }

    /// When the next frame falls due, or None when there is nothing to wait for: playback has
    /// outrun the decoder, and the frame's arrival wakes the loop instead of a timer.
    pub fn deadline(&self) -> Option<Instant> {
        let player = self.play.as_ref()?;
        let (_, at) = player.ring.0.lock().unwrap().due(self.seq + 1)?;
        Some(player.start + at)
    }

    /// Steps one frame forward whatever the clock says. There is no step back: frames are
    /// cumulative, so reaching an earlier one means decoding the file from the start
    /// (ADR 0020 point 2).
    pub fn step(&mut self) -> bool {
        let Some((ring, _)) = self.player() else {
            return false;
        };
        let (lock, cv) = &*ring;
        let mut buf = lock.lock().unwrap();
        let Some((frame, at)) = buf.due(self.seq + 1) else {
            return false;
        };
        self.shown = frame.img.clone();
        self.at = at;
        self.seq += 1;
        buf.pos = self.seq;
        cv.notify_all();
        true
    }

    /// Re-bases the clock on the frame currently shown, so playback continues from there
    /// rather than racing to catch up on the time spent paused, hidden or stepping.
    pub fn resume(&mut self, now: Instant) {
        let at = self.at;
        if let Some(player) = self.play.as_mut() {
            player.start = now - at;
        }
    }

    // The buffer and the clock origin, copied out so the caller can still take `&mut self`.
    fn player(&self) -> Option<(Arc<Shared>, Instant)> {
        let player = self.play.as_ref()?;
        Some((player.ring.clone(), player.start))
    }
}

/// One decoded frame, with the moment it falls due measured from the start of playback.
struct Buffered {
    img: Arc<DecodedImage>,
    at: Duration,
}

/// The ring buffer, shared between the event loop and the decoder. `frames` is a consecutive
/// run of the decoder's output starting at `base`; frames before `pos` may be dropped to make
/// room for new ones, which is what bounds the memory a long animation occupies.
struct Ring {
    frames: VecDeque<Buffered>,
    base: u64,
    bytes: usize,
    /// The frame the viewer is showing. Written by the event loop, read by the decoder.
    pos: u64,
    /// Frames in one loop, set once the decoder has been through the file once.
    total: Option<u64>,
    /// How long one loop lasts, for the same reason.
    lap: Duration,
    /// Set when the file stops being current, so the decoder gives up its thread.
    stop: bool,
}

impl Ring {
    fn new() -> Ring {
        Ring {
            frames: VecDeque::new(),
            base: 0,
            bytes: 0,
            pos: 0,
            total: None,
            lap: Duration::ZERO,
            stop: false,
        }
    }

    /// The frame to show `seq` steps into playback, and when it falls due.
    ///
    /// While the decoder is still on its first pass — and forever after for an animation that
    /// had to stream — `seq` is exactly the decoder's own frame index and the stored moment
    /// is the answer. An animation that fit the budget is still whole when the decoder stops,
    /// so playback wraps inside the buffer instead and one loop's delays are re-applied per
    /// lap. `base == 0` is what tells the two apart: only eviction ever moves it.
    fn due(&self, seq: u64) -> Option<(&Buffered, Duration)> {
        match self.total {
            Some(n) if self.base == 0 => {
                let laps = u32::try_from(seq / n).unwrap_or(u32::MAX);
                let frame = self.get(seq % n)?;
                Some((frame, frame.at + self.lap.saturating_mul(laps)))
            }
            _ => {
                let frame = self.get(seq)?;
                Some((frame, frame.at))
            }
        }
    }

    fn get(&self, index: u64) -> Option<&Buffered> {
        self.frames
            .get(usize::try_from(index.checked_sub(self.base)?).ok()?)
    }
}

// The decoding thread. Frames go into the ring in order; when the end of an animation that
// did not fit is reached, the file is opened again and decoding continues into the next lap
// (ADR 0019 point 5). One that did fit needs no second pass, so the thread simply ends.
fn decode(path: &Path, ring: &Shared, proxy: &EventLoopProxy<UserEvent>) {
    let mut seq: u64 = 0; // the decoder's own index, continuous across passes
    let mut at = Duration::ZERO;
    loop {
        let Some(frames) = frames(path) else {
            return;
        };
        let (pass_seq, pass_at) = (seq, at);
        for frame in frames {
            let Ok(frame) = frame else {
                return; // a truncated or corrupt animation stops where it stops
            };
            let buffered = Buffered {
                img: Arc::new(pack_rgba(frame.buffer())),
                at,
            };
            if !push(ring, buffered, seq, proxy) {
                return; // the viewer has let this file go
            }
            seq += 1;
            at += delay(&frame);
        }
        let count = seq - pass_seq;
        if count == 0 {
            return;
        }
        let (lock, cv) = ring;
        let mut buf = lock.lock().unwrap();
        buf.total = Some(count);
        buf.lap = at - pass_at;
        // Nothing was ever dropped, so the whole animation is resident: playback wraps inside
        // the buffer at no cost and there is nothing left to decode.
        let resident = buf.base == 0;
        drop(buf);
        cv.notify_all();
        // The frame count is new information for the title, which is otherwise never rebuilt
        // on the animation's account.
        let _ = proxy.send_event(UserEvent::AnimCounted);
        if resident {
            return;
        }
    }
}

// Adds a frame, waiting while the budget is full. Frames the viewer has already passed are
// dropped to make room; when there are none, the decoder simply waits — which is also what
// stops it while playback is paused or the window is minimised, with no separate flag for
// either (ADR 0019 point 9, ADR 0021). Returns false once the viewer has let the file go.
fn push(
    ring: &Shared,
    frame: Buffered,
    seq: u64,
    proxy: &EventLoopProxy<UserEvent>,
) -> bool {
    let size = frame.img.px.len() * 4;
    let (lock, cv) = ring;
    let mut buf = lock.lock().unwrap();
    loop {
        if buf.stop {
            return false;
        }
        // Two frames are always accepted, whatever they cost: playback only advances onto a
        // frame that is already there, so a buffer that could hold just the current one would
        // never release it and both threads would wait on each other forever.
        if buf.frames.len() < 2 || buf.bytes + size <= BUDGET {
            break;
        }
        if buf.base < buf.pos {
            let gone = buf.frames.pop_front().expect("base < pos implies a frame");
            buf.bytes -= gone.img.px.len() * 4;
            buf.base += 1;
        } else {
            buf = cv.wait(buf).unwrap();
        }
    }
    buf.bytes += size;
    buf.frames.push_back(frame);
    // Wake the viewer only when it is actually waiting on this frame. While the decoder runs
    // ahead — the normal case — it stays silent, so filling the buffer costs no events.
    let awaited = seq <= buf.pos + 1;
    drop(buf);
    if awaited {
        let _ = proxy.send_event(UserEvent::AnimFrame);
    }
    true
}

// The frame iterator for a file that holds an animation, or None for anything else. Detection
// is by content like every other format's (ADR 0019 point 12): a header flag for APNG and
// WebP, and for GIF the frames themselves, which is why the caller checks for a second one.
fn frames(path: &Path) -> Option<Frames<'static>> {
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    match reader.format()? {
        ImageFormat::Gif => Some(GifDecoder::new(reader.into_inner()).ok()?.into_frames()),
        ImageFormat::Png => {
            let decoder = PngDecoder::new(reader.into_inner()).ok()?;
            if !decoder.is_apng().ok()? {
                return None;
            }
            Some(decoder.apng().ok()?.into_frames())
        }
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(reader.into_inner()).ok()?;
            decoder.has_animation().then(|| decoder.into_frames())
        }
        _ => None,
    }
}

// How long a frame stays on screen, normalised on the browser convention.
fn delay(frame: &image::Frame) -> Duration {
    let d = Duration::from(frame.delay());
    if d <= FAST_DELAY {
        SLOW_DELAY
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A buffer holding `count` frames of 50 ms each, starting at logical index `base`.
    fn ring(base: u64, count: u64) -> Ring {
        let mut r = Ring::new();
        r.base = base;
        r.frames = (base..base + count)
            .map(|i| Buffered {
                img: Arc::new(DecodedImage { w: 1, h: 1, px: vec![0] }),
                at: Duration::from_millis(50 * i),
            })
            .collect();
        r
    }

    #[test]
    fn a_resident_animation_wraps_and_keeps_counting_time() {
        let mut r = ring(0, 4);
        r.total = Some(4);
        r.lap = Duration::from_millis(200);
        // Second lap: frame 0 again, but 200 ms later than the first time it was shown.
        let (_, at) = r.due(4).unwrap();
        assert_eq!(at, Duration::from_millis(200));
        let (_, at) = r.due(6).unwrap();
        assert_eq!(at, Duration::from_millis(300));
        // ...and a hundred laps in, the deadline is still exact rather than accumulated.
        let (_, at) = r.due(4 * 100 + 1).unwrap();
        assert_eq!(at, Duration::from_millis(200 * 100 + 50));
    }

    #[test]
    fn a_streaming_animation_reads_the_moment_the_decoder_recorded() {
        // Frames 0 and 1 have been dropped, and one full pass was four frames long — but the
        // decoder kept going, so index and moment both continue past the end of the file.
        let mut r = ring(2, 4);
        r.total = Some(4);
        r.lap = Duration::from_millis(200);
        assert!(r.due(1).is_none(), "a dropped frame is gone, not wrapped to");
        let (_, at) = r.due(4).unwrap();
        assert_eq!(at, Duration::from_millis(200));
        let (_, at) = r.due(5).unwrap();
        assert_eq!(at, Duration::from_millis(250));
        assert!(r.due(6).is_none(), "not decoded yet");
    }

    #[test]
    fn before_the_first_pass_ends_the_index_is_the_decoders_own() {
        let r = ring(0, 3);
        assert_eq!(r.due(0).unwrap().1, Duration::ZERO);
        assert_eq!(r.due(2).unwrap().1, Duration::from_millis(100));
        assert!(r.due(3).is_none());
    }
}
