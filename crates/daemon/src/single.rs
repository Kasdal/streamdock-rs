//! One daemon per machine.
//!
//! A named mutex stops a second copy. Without it, two processes would write
//! reports to the same panel and the key images would fight each other.

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// Default name of the guard mutex.
pub const MUTEX_NAME: &str = "Local\\StreamDockDaemon";

/// Holds the mutex handle for the life of the process.
pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Take the guard. Fails when another process holds it.
    pub fn acquire(name: &str) -> std::io::Result<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY: the name is null terminated and the handle is kept.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        let error = unsafe { GetLastError() };
        if error == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "another streamdockd is already running",
            ));
        }

        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
