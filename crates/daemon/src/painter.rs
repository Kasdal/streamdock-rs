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
    running: Vec<Running>,
}

impl Painter {
    /// A painter that resolves relative paths against `base`.
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            cache: HashMap::new(),
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

            if let Some(path) = animation_path(page, key) {
                match self.animation(path, size) {
                    Ok(animation) => {
                        let first = Arc::clone(&animation.frames[0]);
                        panel.key(key, first);

                        if animation.frames.len() > 1 {
                            let delay = animation.delays_ms[0] as u64;
                            self.running.push(Running {
                                key,
                                frames: Arc::clone(&animation),
                                index: 0,
                                next: Instant::now() + Duration::from_millis(delay.max(MIN_FRAME_MS)),
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
        if self.cache.len() >= MAX_CACHE {
            self.cache.clear();
        }
        self.cache.insert(cache_key, Arc::clone(&animation));
        Ok(animation)
    }
}

fn animation_path(page: &Page, key: u8) -> Option<&str> {
    match page.key(key).and_then(|config| config.image.as_ref()) {
        Some(ImageSpec::Animation { path }) => Some(path.as_str()),
        _ => None,
    }
}
