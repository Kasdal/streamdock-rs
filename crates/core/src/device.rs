//! High level device handle.
//!
//! One `Device` owns one vendor collection. It sends commands, uploads images
//! and reads key events with a heartbeat. A failed call returns an error and
//! the caller decides whether to reopen the handle.
//!
//! This type is not `Sync`. Keep one owner thread and pass work through a
//! queue, so a large image cannot starve the key reader.
//!
//! The panel holds no image storage across a power loss. After a replug, the
//! panel shows only its start logo until the host uploads the images again.
//! A daemon must keep a copy of every tile and repaint on reconnect.

use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::hid;
use crate::model;
use crate::proto::{self, KeyEvent};

/// How often the host must send `CONNECT`. The panel stops sending key
/// reports when the heartbeat stops. The vendor driver uses 8 seconds.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(8);

/// Longest single read slice inside one `poll_event` call.
const READ_SLICE_MS: u32 = 200;

/// Number of tries for one image upload. The vendor driver tries three times.
pub const IMAGE_ATTEMPTS: u32 = 3;

/// Pause between image upload tries. The vendor driver uses 20 ms.
pub const IMAGE_RETRY_PAUSE: Duration = Duration::from_millis(20);

/// Result of one non-blocking poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    /// The panel sent a key event.
    Event(KeyEvent),
    /// No key event arrived before the timeout.
    Idle,
}

/// One open panel.
pub struct Device {
    link: hid::Device,
    buf: Vec<u8>,
    last_beat: Option<Instant>,
}

impl Device {
    /// Open the vendor collection of the known model.
    pub fn open_first() -> Result<Self> {
        Self::open_pid(model::PID)
    }

    /// Open the vendor collection with this product ID.
    pub fn open_pid(pid: u16) -> Result<Self> {
        let info = hid::find_vendor(pid).ok_or(Error::NotFound { product_id: pid })?;
        Self::open_info(info)
    }

    /// Open a vendor collection that was already enumerated.
    pub fn open_info(info: hid::Info) -> Result<Self> {
        let link = hid::Device::open(info.clone())?;
        let len = info.input_len.max(1) as usize;
        Ok(Self {
            link,
            buf: vec![0u8; len],
            last_beat: None,
        })
    }

    /// Interface facts of the open collection.
    pub fn info(&self) -> &hid::Info {
        &self.link.info
    }

    /// True when the device is still present in device manager.
    pub fn is_present(&self) -> bool {
        hid::is_present(self.link.info.product_id)
    }

    /// Send one command payload as one 1025 byte report.
    pub fn send_command(&mut self, command: &[u8]) -> Result<()> {
        let report = proto::frame(command);
        let wrote = self.link.write_report(&report)?;
        if wrote != report.len() {
            return Err(Error::ShortWrite {
                wrote,
                expected: report.len(),
            });
        }
        Ok(())
    }

    /// Wake the panel.
    pub fn wake(&mut self) -> Result<()> {
        self.send_command(proto::cmd::WAKE)
    }

    /// Send one heartbeat and record the time.
    pub fn heartbeat(&mut self) -> Result<()> {
        self.send_command(proto::cmd::HEARTBEAT)?;
        self.last_beat = Some(Instant::now());
        Ok(())
    }

    /// Send a heartbeat if one is due.
    pub fn ensure_heartbeat(&mut self) -> Result<()> {
        let due = match self.last_beat {
            None => true,
            Some(t) => t.elapsed() >= HEARTBEAT_INTERVAL,
        };
        if due {
            self.heartbeat()?;
        }
        Ok(())
    }

    /// Wake the panel, start the session and show the stored images.
    ///
    /// The panel sends no key report until the host does this.
    pub fn start_session(&mut self) -> Result<()> {
        self.wake()?;
        self.heartbeat()?;
        self.refresh()
    }

    /// Ask the panel to show the images that the host uploaded.
    pub fn refresh(&mut self) -> Result<()> {
        self.send_command(proto::cmd::REFRESH)
    }

    /// Set the panel brightness, 0 to 100, as a raw value.
    pub fn set_brightness(&mut self, percent: u8) -> Result<()> {
        self.send_command(&proto::cmd::brightness(percent))
    }

    /// Set the panel brightness through the curve that the vendor driver uses.
    pub fn set_brightness_curved(&mut self, percent: u8) -> Result<()> {
        self.send_command(&proto::cmd::brightness_curved(percent))
    }

    /// Set the LED ring brightness, 0 to 100. Not verified on this unit.
    pub fn set_led_brightness(&mut self, percent: u8) -> Result<()> {
        self.send_command(&proto::cmd::led_brightness(percent))
    }

    /// Set the LED array. Not verified on this unit.
    pub fn set_led_array(&mut self, values: &[u8]) -> Result<()> {
        self.send_command(&proto::cmd::led_array(values))
    }

    /// Clear one key by key number.
    pub fn clear_key(&mut self, key: u8) -> Result<()> {
        let id = self.key_id(key)?;
        self.send_command(&proto::cmd::clear_key(id))
    }

    /// Clear every key.
    pub fn clear_all(&mut self) -> Result<()> {
        self.send_command(&proto::cmd::clear_panel())
    }

    /// Clear every key and show the result.
    pub fn blank(&mut self) -> Result<()> {
        self.clear_all()?;
        self.refresh()
    }

    /// Upload one JPEG to one key by key number. Retries on failure.
    pub fn send_image(&mut self, key: u8, jpeg: &[u8]) -> Result<()> {
        let id = self.key_id(key)?;
        if jpeg.len() > proto::MAX_IMAGE_BYTES {
            return Err(Error::ImageTooLarge {
                size: jpeg.len(),
                limit: proto::MAX_IMAGE_BYTES,
            });
        }
        if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
            return Err(Error::Message("image is not a JPEG file".into()));
        }

        let mut last = None;
        for attempt in 0..IMAGE_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(IMAGE_RETRY_PAUSE);
            }
            match self.transfer(id, jpeg) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| Error::Message("image upload failed".into())))
    }

    /// One upload attempt: header, data chunks and refresh.
    fn transfer(&mut self, id: u8, jpeg: &[u8]) -> Result<()> {
        self.send_command(&proto::cmd::image_header(id, jpeg.len()))?;

        for part in jpeg.chunks(proto::PAYLOAD) {
            let report = proto::chunk(part);
            let wrote = self.link.write_report(&report)?;
            if wrote != report.len() {
                return Err(Error::ShortWrite {
                    wrote,
                    expected: report.len(),
                });
            }
        }

        self.refresh()
    }

    /// Read key reports for at most `timeout`.
    ///
    /// This sends the heartbeat when it is due, so a caller can loop on this
    /// method and never lose the session.
    pub fn poll_event(&mut self, timeout: Duration) -> Result<Poll> {
        let deadline = Instant::now() + timeout;
        loop {
            self.ensure_heartbeat()?;

            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Poll::Idle);
            }
            let slice = left.as_millis().clamp(1, READ_SLICE_MS as u128) as u32;

            match self.link.read_report_timeout(&mut self.buf, slice)? {
                hid::ReadOutcome::Data(n) => {
                    let report = &self.buf[..n.min(self.buf.len())];
                    if let Some(event) = proto::parse_input(report) {
                        return Ok(Poll::Event(event));
                    }
                }
                hid::ReadOutcome::Timeout => {}
            }
        }
    }

    /// Upload one pre-encoded JPEG by hardware key ID.
    ///
    /// This does not check the size limit, because the real firmware limit is
    /// not measured yet. Callers that work with key numbers use `send_image`.
    pub fn send_image_raw(&mut self, id: u8, jpeg: &[u8]) -> Result<()> {
        self.transfer(id, jpeg)
    }

    /// Write one raw report, report ID byte included.
    ///
    /// This bypasses the command framing. Use it for protocol work only.
    pub fn write_report(&mut self, report: &[u8]) -> Result<usize> {
        Ok(self.link.write_report(report)?)
    }

    /// Read one raw input report with the heartbeat kept alive.
    pub fn read_raw(&mut self, timeout: Duration) -> Result<hid::ReadOutcome> {
        self.ensure_heartbeat()?;
        let ms = timeout.as_millis().clamp(1, 1000) as u32;
        Ok(self.link.read_report_timeout(&mut self.buf, ms)?)
    }

    /// The last report that `read_raw` returned.
    pub fn last_report(&self, len: usize) -> &[u8] {
        &self.buf[..len.min(self.buf.len())]
    }

    /// Convert a key number to the hardware key ID.
    pub fn key_id(&self, key: u8) -> Result<u8> {
        if key == 0 || key > model::KEY_COUNT {
            return Err(Error::BadKey {
                key,
                max: model::KEY_COUNT,
            });
        }
        Ok(model::key_id(key))
    }
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("product_id", &self.link.info.product_id)
            .field("path", &self.link.info.path)
            .finish()
    }
}
