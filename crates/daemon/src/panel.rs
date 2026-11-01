//! The thread that owns the device.
//!
//! The panel thread is the only owner of the `Session`. It drains a bounded
//! command queue with a short time budget, then reads key reports. That keeps
//! the heartbeat and the key reader alive while images and animation frames
//! arrive.
//!
//! The coordinator thread encodes images and decides when a frame is due. It
//! never blocks on the device.

use std::sync::mpsc::{channel, sync_channel, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use streamdock_core::proto::KeyEvent;
use streamdock_core::session::{Session, SessionPoll};

/// Commands that may wait in the queue.
pub const QUEUE_LENGTH: usize = 1024;

/// Longest time the thread spends on queued commands before it reads keys.
pub const COMMAND_BUDGET_MS: u64 = 40;

/// Longest single read slice.
pub const READ_SLICE_MS: u64 = 20;

/// One write request for the panel.
pub enum PanelCommand {
    /// Upload one JPEG to one key number.
    Key { key: u8, jpeg: Arc<[u8]> },
    /// Set the panel brightness.
    Brightness(u8),
    /// Clear every key.
    Blank,
    /// Stop the thread.
    Stop,
}

/// One report from the panel thread.
#[derive(Debug)]
pub enum PanelEvent {
    /// The panel became available.
    Connected,
    /// The panel became unavailable.
    Disconnected(String),
    /// A key report.
    Key(KeyEvent),
}

/// Handle to the panel thread.
pub struct Panel {
    commands: SyncSender<PanelCommand>,
    events: Receiver<PanelEvent>,
    thread: Option<JoinHandle<()>>,
}

impl Panel {
    /// Start the thread. It connects on its own and reports `Connected`.
    pub fn start(pid: u16) -> std::io::Result<Self> {
        let (commands, incoming) = sync_channel(QUEUE_LENGTH);
        let (outgoing, events) = channel();

        let thread = thread::Builder::new()
            .name("streamdock-panel".into())
            .spawn(move || run(pid, incoming, outgoing))?;

        Ok(Self {
            commands,
            events,
            thread: Some(thread),
        })
    }

    /// Queue one image. Returns false when the queue is full.
    pub fn key(&self, key: u8, jpeg: Arc<[u8]>) -> bool {
        self.commands
            .try_send(PanelCommand::Key { key, jpeg })
            .is_ok()
    }

    /// Queue a brightness change.
    pub fn brightness(&self, value: u8) -> bool {
        self.commands
            .try_send(PanelCommand::Brightness(value))
            .is_ok()
    }

    /// Queue a clear of every key.
    pub fn blank(&self) -> bool {
        self.commands.try_send(PanelCommand::Blank).is_ok()
    }

    /// Take one event without blocking.
    pub fn try_event(&self) -> Option<PanelEvent> {
        self.events.try_recv().ok()
    }

    /// Stop the thread and wait for it.
    pub fn stop(&mut self) {
        // A blocking send is fine here: the thread drains the queue.
        let _ = self.commands.send(PanelCommand::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(pid: u16, commands: Receiver<PanelCommand>, events: Sender<PanelEvent>) {
    let mut session = Session::new(pid);
    let mut connected = false;

    loop {
        let deadline = Instant::now() + Duration::from_millis(COMMAND_BUDGET_MS);

        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }

            match commands.recv_timeout(left) {
                Ok(PanelCommand::Stop) => return,
                Ok(PanelCommand::Key { key, jpeg }) => {
                    if connected && session.with_device(|dev| dev.send_image(key, &jpeg)).is_err() {
                        connected = false;
                        let _ = events.send(PanelEvent::Disconnected(
                            "write failed, device lost".into(),
                        ));
                    }
                }
                Ok(PanelCommand::Brightness(value)) => {
                    if connected {
                        let _ = session.with_device(|dev| dev.set_brightness(value));
                    }
                }
                Ok(PanelCommand::Blank) => {
                    if connected {
                        let _ = session.with_device(|dev| dev.blank());
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        match session.poll(Duration::from_millis(READ_SLICE_MS)) {
            SessionPoll::Connected => {
                connected = true;
                let _ = events.send(PanelEvent::Connected);
            }
            SessionPoll::Disconnected => {
                if connected {
                    let _ = events.send(PanelEvent::Disconnected("device lost".into()));
                }
                connected = false;
            }
            SessionPoll::Event(event) => {
                let _ = events.send(PanelEvent::Key(event));
            }
            SessionPoll::Idle => {}
        }
    }
}
