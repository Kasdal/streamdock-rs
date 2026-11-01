//! Connection that survives unplug, sleep and resume.
//!
//! A `Session` owns at most one `Device`. When a call fails, the session drops
//! the handle and reopens it on the next poll. Reopen tries are rate limited,
//! so a missing panel costs no CPU.
//!
//! This is the poll form of hot plug. The crate enumerates the HID interfaces
//! every `RECONNECT_INTERVAL` while the panel is absent. This needs no window
//! and no `WM_DEVICECHANGE` listener, so it also works in a service.
//!
//! Measured: an unplug and replug gives `Disconnected`, then `Connected`.
//! The panel comes back blank. The caller must upload every tile again after
//! `Connected`, because the panel loses its images when it loses power.

use std::time::{Duration, Instant};

use crate::device::{Device, Poll};
use crate::error::{Error, Result};
use crate::model;
use crate::proto::KeyEvent;

/// Shortest time between two reopen tries.
pub const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);

/// One step of a session loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPoll {
    /// The panel sent a key event.
    Event(KeyEvent),
    /// Nothing to report.
    Idle,
    /// The panel became available in this step.
    Connected,
    /// The panel became unavailable in this step. Reported once per loss.
    Disconnected,
}

/// A device that reconnects itself.
pub struct Session {
    pid: u16,
    device: Option<Device>,
    next_attempt: Instant,
    last_error: Option<Error>,
    announced: Option<bool>,
}

impl Session {
    /// Make a session for one product ID. It does not open the device yet.
    pub fn new(pid: u16) -> Self {
        Self {
            pid,
            device: None,
            next_attempt: Instant::now(),
            last_error: None,
            announced: None,
        }
    }

    /// Make a session for the known model.
    pub fn for_model() -> Self {
        Self::new(model::PID)
    }

    /// Product ID that this session tracks.
    pub fn product_id(&self) -> u16 {
        self.pid
    }

    /// True when a handle is open.
    pub fn is_connected(&self) -> bool {
        self.device.is_some()
    }

    /// The last transport error, if any.
    pub fn last_error(&self) -> Option<&Error> {
        self.last_error.as_ref()
    }

    /// Open the panel now, ignoring the rate limit.
    ///
    /// A successful open also starts the session, because the panel sends no
    /// key report until the host does so.
    pub fn connect(&mut self) -> Result<&mut Device> {
        if self.device.is_none() {
            let mut dev = Device::open_pid(self.pid)?;
            dev.start_session()?;
            self.device = Some(dev);
            self.last_error = None;
        }
        Ok(self.device.as_mut().expect("device was just set"))
    }

    /// Drop the handle. The next poll tries to open it again.
    pub fn disconnect(&mut self) {
        self.device = None;
        self.announced = Some(false);
    }

    /// Open the panel unless the rate limit blocks it.
    fn try_connect(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next_attempt {
            return false;
        }
        match self.connect() {
            Ok(_) => true,
            Err(e) => {
                self.last_error = Some(e);
                self.next_attempt = now + RECONNECT_INTERVAL;
                false
            }
        }
    }

    /// Run one operation on the open device.
    ///
    /// If the operation fails with a transport error, the session drops the
    /// handle, reopens the panel and runs the operation once more. A logic
    /// error, such as a bad key number, is returned at once.
    pub fn with_device<T>(&mut self, mut f: impl FnMut(&mut Device) -> Result<T>) -> Result<T> {
        let dev = match self.connect() {
            Ok(d) => d,
            Err(e) => {
                self.next_attempt = Instant::now() + RECONNECT_INTERVAL;
                self.last_error = Some(e.duplicate());
                return Err(e);
            }
        };

        match f(dev) {
            Ok(value) => Ok(value),
            Err(e @ Error::Io(_)) => {
                self.device = None;
                self.last_error = Some(e.duplicate());
                self.next_attempt = Instant::now();
                let dev = self.connect()?;
                f(dev)
            }
            Err(e) => Err(e),
        }
    }

    /// Wait up to `timeout` for one key event and keep the session alive.
    pub fn poll(&mut self, timeout: Duration) -> SessionPoll {
        if self.device.is_none() {
            if self.try_connect() {
                self.announced = Some(true);
                return SessionPoll::Connected;
            }
            self.wait_for_retry(timeout);
            if self.announced == Some(false) {
                return SessionPoll::Idle;
            }
            self.announced = Some(false);
            return SessionPoll::Disconnected;
        }

        let outcome = self
            .device
            .as_mut()
            .expect("device is open")
            .poll_event(timeout);

        match outcome {
            Ok(Poll::Event(e)) => SessionPoll::Event(e),
            Ok(Poll::Idle) => SessionPoll::Idle,
            Err(e) => {
                self.last_error = Some(e);
                self.device = None;
                self.next_attempt = Instant::now() + RECONNECT_INTERVAL;
                self.announced = Some(false);
                SessionPoll::Disconnected
            }
        }
    }

    fn wait_for_retry(&mut self, timeout: Duration) {
        let wait = self
            .next_attempt
            .saturating_duration_since(Instant::now())
            .min(timeout);
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}
