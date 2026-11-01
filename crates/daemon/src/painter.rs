//! Images and animation frames for one page.
//!
//! The painter encodes tiles on the coordinator thread and queues them for the
//! panel thread. Animations are pre-encoded, so a frame is only a queue write.
//!
//! A `Painter` keeps a small cache of loaded animations, so a repaint after a
//! reconnect does not decode the same GIF again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use streamdock_core::model;
use streamdock_render as render;
use streamdock_store::profile::{ImageSpec, Page};

use crate::panel::Panel;

/// Loaded animations kept in memory.
pub const MAX_CACHE: usize = 64;

/// Largest total encoded size of the animation cache.
pub const MAX_CACHE_BYTES: usize = 24_000_000;

/// Automatic start offset per key, so a page does not blink in lockstep.
pub const AUTO_PHASE_MS: u64 = 120;

/// Longest sleep of the coordinator loop when nothing happens.
pub const IDLE_TICK_MS: u64 = 8;

/// Shortest delay between two animation frames.
pub const MIN_FRAME_MS: u64 = 20;

struct Running {
    key: u8,
    frames: Arc<render::Animation>,
    index: usize,
    next: Instant,
}

/// Turns a page into panel writes and drives the animation clocks.
pub struct Painter {
    base: PathBuf,
    cache: HashMap<(PathBuf, u16), Arc<render::Animation>>,
    cache_bytes: usize,
    running: Vec<Running>,
}

impl Painter {
    /// A painter that resolves relative paths against `base`.
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            cache: HashMap::new(),
            cache_bytes: 0,
            running: Vec::new(),
        }
    }

    /// True when at least one key is animated.
    pub fn is_animated(&self) -> bool {
        !self.running.is_empty()
    }

    /// Stop every animation. The keys keep their last frame.
    pub fn stop_animations(&mut self) {
        self.running.clear();
    }

    /// Queue every key of a page. Returns the number of keys queued.
    ///
    /// A fault is reported through `warn` and the key gets a red ERR tile.
    pub fn paint(
        &mut self,
        panel: &Panel,
        page: &Page,
        warn: &mut dyn FnMut(String),
    ) -> usize {
        self.running.clear();
        let mut queued = 0;

        for key in 1..=model::KEY_COUNT {
            let size = model::key_size(key);

            if let Some((path, phase_ms)) = animation_spec(page, key) {
                match self.animation(path, size) {
                    Ok(animation) => {
                        // Each key starts at its own point in the loop, unless
                        // the profile asks for a phase.
                        let phase = phase_ms.unwrap_or_else(|| (key as u64 - 1) * AUTO_PHASE_MS);
                        let (index, wait) = frame_at(&animation, phase);

                        panel.key(key, Arc::clone(&animation.frames[index]));

                        if animation.frames.len() > 1 {
                            self.running.push(Running {
                                key,
                                frames: Arc::clone(&animation),
                                index,
                                next: Instant::now()
                                    + Duration::from_millis(wait.max(MIN_FRAME_MS)),
                            });
                        }
                        queued += 1;
                        continue;
                    }
                    Err(message) => warn(format!("key {key}: {message}")),
                }
            }

            if self.send_static(panel, page, key, size, warn) {
                queued += 1;
            }
        }

        queued
    }

    /// Queue every frame that is due. Returns true when something was sent.
    pub fn tick(&mut self, panel: &Panel) -> bool {
        if self.running.is_empty() {
            return false;
        }

        let now = Instant::now();
        let mut sent = false;

        for animation in &mut self.running {
            let count = animation.frames.frames.len();
            if count == 0 {
                continue;
            }

            while now >= animation.next {
                animation.index = (animation.index + 1) % count;
                let frame = Arc::clone(&animation.frames.frames[animation.index]);
                panel.key(animation.key, frame);

                let delay = (animation.frames.delays_ms[animation.index] as u64).max(MIN_FRAME_MS);
                animation.next += Duration::from_millis(delay);
                sent = true;

                // After a long stall, continue from now instead of racing.
                if animation.next < now {
                    animation.next = now + Duration::from_millis(delay);
                    break;
                }
            }
        }

        sent
    }

    fn send_static(
        &self,
        panel: &Panel,
        page: &Page,
        key: u8,
        size: (u16, u16),
        warn: &mut dyn FnMut(String),
    ) -> bool {
        match render::encode_key_in(page, key, &self.base) {
            Ok(jpeg) => panel.key(key, Arc::from(jpeg)),
            Err(message) => {
                warn(format!("key {key}: {message}"));
                match render::fault_tile(size) {
                    Ok(jpeg) => panel.key(key, Arc::from(jpeg)),
                    Err(message) => {
                        warn(format!("key {key}: cannot draw the fault tile: {message}"));
                        false
                    }
                }
            }
        }
    }

    fn animation(
        &mut self,
        path: &str,
        size: (u16, u16),
    ) -> Result<Arc<render::Animation>, String> {
        let resolved = render::resolve_path(&self.base, path);
        let cache_key = (resolved.clone(), size.0);

        if let Some(found) = self.cache.get(&cache_key) {
            return Ok(Arc::clone(found));
        }

        let animation = Arc::new(render::load_animation(&resolved, size)?);
        let bytes = animation.bytes();

        if self.cache.len() >= MAX_CACHE || self.cache_bytes + bytes > MAX_CACHE_BYTES {
            self.cache.clear();
            self.cache_bytes = 0;
        }

        self.cache_bytes += bytes;
        self.cache.insert(cache_key, Arc::clone(&animation));
        Ok(animation)
    }
}

/// What the profile asks for on this key: the file and an optional phase.
fn animation_spec(page: &Page, key: u8) -> Option<(&str, Option<u64>)> {
    match page.key(key).and_then(|config| config.image.as_ref()) {
        Some(ImageSpec::Animation { path, phase_ms }) => Some((path.as_str(), *phase_ms)),
        _ => None,
    }
}

/// Frame that is on screen after `offset_ms`, and the time until the next one.
pub fn frame_at(animation: &render::Animation, offset_ms: u64) -> (usize, u64) {
    let duration = animation.duration_ms() as u64;
    if duration == 0 || animation.frames.is_empty() {
        return (0, 0);
    }

    let mut left = offset_ms % duration;
    for (index, delay) in animation.delays_ms.iter().enumerate() {
        let delay = (*delay as u64).max(MIN_FRAME_MS);
        if left < delay {
            return (index, delay - left);
        }
        left -= delay;
    }

    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animation() -> render::Animation {
        render::Animation {
            frames: (0..4).map(|_| Arc::from(vec![0u8; 10])).collect(),
            delays_ms: vec![100, 100, 100, 100],
        }
    }

    #[test]
    fn the_first_frame_is_shown_at_zero() {
        assert_eq!(frame_at(&animation(), 0), (0, 100));
    }

    #[test]
    fn a_phase_selects_the_frame_and_the_wait() {
        assert_eq!(frame_at(&animation(), 120), (1, 80));
        assert_eq!(frame_at(&animation(), 250), (2, 50));
    }

    #[test]
    fn a_phase_beyond_the_loop_wraps() {
        assert_eq!(frame_at(&animation(), 450), (0, 50));
        // 1000 ms is 200 ms into the 400 ms loop, so frame 2 is on screen.
        assert_eq!(frame_at(&animation(), 1000), (2, 100));
    }

    #[test]
    fn an_empty_animation_returns_the_first_frame() {
        let empty = render::Animation {
            frames: Vec::new(),
            delays_ms: Vec::new(),
        };
        assert_eq!(frame_at(&empty, 500), (0, 0));
    }

    #[test]
    fn an_automatic_phase_spreads_the_keys() {
        assert_eq!((1u64 - 1) * AUTO_PHASE_MS, 0);
        assert_eq!((2u64 - 1) * AUTO_PHASE_MS, 120);
        assert_eq!((5u64 - 1) * AUTO_PHASE_MS, 480);
    }
}
