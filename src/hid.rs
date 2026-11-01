//! Windows HID access for Stream Dock style devices.
//!
//! The device exposes two interfaces:
//!   * vendor collection  (usage page 0xFFA0, usage 0x0001) - command and image channel
//!   * boot keyboard      (usage page 0x0001, usage 0x0006) - key events
//!
//! This module uses the Win32 HID API directly. It needs no C toolchain and
//! no third party driver.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
    HidD_GetProductString, HidP_GetButtonCaps, HidP_GetCaps, HidP_GetValueCaps, HidP_Feature,
    HidP_Input, HidP_Output, HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDD_ATTRIBUTES,
    HIDP_VALUE_CAPS,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// Vendor ID used by the Stream Dock family.
pub const VENDOR_ID: u16 = 0x0300;

/// A device interface found on the system.
#[derive(Debug, Clone)]
pub struct Info {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
    pub product: String,
    pub usage_page: u16,
    pub usage: u16,
    pub input_len: u16,
    pub output_len: u16,
    pub feature_len: u16,
}

impl Info {
    /// The vendor collection carries commands and images.
    pub fn is_vendor_collection(&self) -> bool {
        self.usage_page >= 0xFF00
    }

    /// The keyboard collection carries key events.
    pub fn is_keyboard(&self) -> bool {
        self.usage_page == 0x0001 && self.usage == 0x0006
    }
}

/// Result of one timed read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The device sent a report with this many bytes.
    Data(usize),
    /// No report arrived before the timeout.
    Timeout,
}

/// An open HID handle.
pub struct Device {
    handle: HANDLE,
    event: HANDLE,
    pub info: Info,
}

// The handle is only used from one thread at a time by the caller.
unsafe impl Send for Device {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn cb_size() -> u32 {
    std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32
}

/// Enumerate every HID interface whose vendor ID matches `vendor_id`.
pub fn enumerate(vendor_id: u16) -> Vec<Info> {
    let mut out = Vec::new();

    // SAFETY: all pointers below come from the Windows API and are freed in order.
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);

        let set = SetupDiGetClassDevsW(
            &guid,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        if set == INVALID_HANDLE_VALUE as isize {
            return out;
        }

        let mut index = 0u32;
        loop {
            let mut did: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            did.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, std::ptr::null_mut(), &guid, index, &mut did) == 0 {
                break;
            }
            index += 1;

            let mut needed = 0u32;
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &did,
                std::ptr::null_mut(),
                0,
                &mut needed,
                std::ptr::null_mut(),
            );
            if needed == 0 {
                continue;
            }

            let mut buf = vec![0u8; needed as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = cb_size();
            if SetupDiGetDeviceInterfaceDetailW(
                set,
                &did,
                detail,
                needed,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == 0
            {
                continue;
            }

            let path_ptr = buf.as_ptr().add(4) as *const u16;
            let mut len = 0usize;
            while *path_ptr.add(len) != 0 {
                len += 1;
            }
            let path = OsString::from_wide(std::slice::from_raw_parts(path_ptr, len))
                .to_string_lossy()
                .to_string();

            if let Some(info) = inspect_path(&path, vendor_id) {
                out.push(info);
            }
        }

        SetupDiDestroyDeviceInfoList(set);
    }

    out
}

/// Open a path briefly and read its attributes and capabilities.
fn inspect_path(path: &str, vendor_id: u16) -> Option<Info> {
    // SAFETY: the handle is closed on every return path below.
    unsafe {
        let w = wide(path);
        let h = CreateFileW(
            w.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut attr: HIDD_ATTRIBUTES = std::mem::zeroed();
        attr.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
        if !HidD_GetAttributes(h, &mut attr) {
            CloseHandle(h);
            return None;
        }
        if attr.VendorID != vendor_id {
            CloseHandle(h);
            return None;
        }

        let mut product = [0u16; 128];
        let byte_len = (product.len() * 2) as u32;
        if !HidD_GetProductString(h, product.as_mut_ptr() as *mut _, byte_len) {
            product[0] = 0;
        }

        let mut caps: HIDP_CAPS = std::mem::zeroed();
        let mut preparsed: isize = 0;
        if HidD_GetPreparsedData(h, &mut preparsed) {
            HidP_GetCaps(preparsed, &mut caps);
            HidD_FreePreparsedData(preparsed);
        }

        CloseHandle(h);

        let n = product.iter().position(|&c| c == 0).unwrap_or(product.len());
        Some(Info {
            path: path.to_string(),
            vendor_id: attr.VendorID,
            product_id: attr.ProductID,
            version: attr.VersionNumber,
            product: String::from_utf16_lossy(&product[..n]),
            usage_page: caps.UsagePage,
            usage: caps.Usage,
            input_len: caps.InputReportByteLength,
            output_len: caps.OutputReportByteLength,
            feature_len: caps.FeatureReportByteLength,
        })
    }
}

/// Report capabilities of one interface, as text lines.
///
/// This is the primary evidence about which report IDs and report sizes the
/// firmware declares. Read it before you trust any command byte.
pub fn describe(path: &str) -> std::io::Result<Vec<String>> {
    // SAFETY: every handle and preparsed buffer is released below.
    unsafe {
        let w = wide(path);
        let h = CreateFileW(
            w.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        let mut preparsed: isize = 0;
        if !HidD_GetPreparsedData(h, &mut preparsed) {
            let err = std::io::Error::last_os_error();
            CloseHandle(h);
            return Err(err);
        }

        let mut caps: HIDP_CAPS = std::mem::zeroed();
        let status = HidP_GetCaps(preparsed, &mut caps);
        if status != HIDP_STATUS_SUCCESS {
            HidD_FreePreparsedData(preparsed);
            CloseHandle(h);
            return Err(std::io::Error::other(format!(
                "HidP_GetCaps failed: 0x{status:08X}"
            )));
        }

        let mut lines = Vec::new();
        lines.push(format!(
            "usage=0x{:04X}:0x{:04X} in={} out={} feature={}",
            caps.UsagePage, caps.Usage, caps.InputReportByteLength,
            caps.OutputReportByteLength, caps.FeatureReportByteLength
        ));
        lines.push(format!(
            "caps count: input buttons={} values={} | output buttons={} values={} | feature buttons={} values={}",
            caps.NumberInputButtonCaps, caps.NumberInputValueCaps,
            caps.NumberOutputButtonCaps, caps.NumberOutputValueCaps,
            caps.NumberFeatureButtonCaps, caps.NumberFeatureValueCaps
        ));

        for (label, kind) in [
            ("input", HidP_Input),
            ("output", HidP_Output),
            ("feature", HidP_Feature),
        ] {
            let (button_count, value_count) = if kind == HidP_Input {
                (caps.NumberInputButtonCaps, caps.NumberInputValueCaps)
            } else if kind == HidP_Output {
                (caps.NumberOutputButtonCaps, caps.NumberOutputValueCaps)
            } else {
                (caps.NumberFeatureButtonCaps, caps.NumberFeatureValueCaps)
            };

            if button_count > 0 {
                let mut arr = vec![HIDP_BUTTON_CAPS::default(); button_count as usize];
                let mut len = button_count;
                if HidP_GetButtonCaps(kind, arr.as_mut_ptr(), &mut len, preparsed)
                    == HIDP_STATUS_SUCCESS
                {
                    for c in arr.iter().take(len as usize) {
                        let usage = if c.IsRange {
                            let r = c.Anonymous.Range;
                            format!("0x{:04X}-0x{:04X}", r.UsageMin, r.UsageMax)
                        } else {
                            format!("0x{:04X}", c.Anonymous.NotRange.Usage)
                        };
                        lines.push(format!(
                            "{label} button  id=0x{:02X} page=0x{:04X} usage={usage} count={}",
                            c.ReportID, c.UsagePage, c.ReportCount
                        ));
                    }
                }
            }

            if value_count > 0 {
                let mut arr = vec![HIDP_VALUE_CAPS::default(); value_count as usize];
                let mut len = value_count;
                if HidP_GetValueCaps(kind, arr.as_mut_ptr(), &mut len, preparsed)
                    == HIDP_STATUS_SUCCESS
                {
                    for c in arr.iter().take(len as usize) {
                        let usage = if c.IsRange {
                            let r = c.Anonymous.Range;
                            format!("0x{:04X}-0x{:04X}", r.UsageMin, r.UsageMax)
                        } else {
                            format!("0x{:04X}", c.Anonymous.NotRange.Usage)
                        };
                        lines.push(format!(
                            "{label} value   id=0x{:02X} page=0x{:04X} usage={usage} bits={} count={}",
                            c.ReportID, c.UsagePage, c.BitSize, c.ReportCount
                        ));
                    }
                }
            }
        }

        // IOCTL_HID_GET_REPORT_DESCRIPTOR gives the raw bytes of the
        // descriptor. Not every driver answers it from user mode.
        let mut raw = vec![0u8; 8192];
        let mut returned = 0u32;
        let ok = DeviceIoControl(
            h,
            IOCTL_HID_GET_REPORT_DESCRIPTOR,
            std::ptr::null(),
            0,
            raw.as_mut_ptr() as *mut _,
            raw.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        );
        if ok != 0 && returned > 0 {
            raw.truncate(returned as usize);
            lines.push(format!("report descriptor, {} bytes:", raw.len()));
            for chunk in raw.chunks(16) {
                lines.push(format!("  {}", sparse_hex(chunk)));
            }
        } else {
            lines.push(format!(
                "report descriptor ioctl failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        HidD_FreePreparsedData(preparsed);
        CloseHandle(h);
        Ok(lines)
    }
}

/// IOCTL_HID_GET_REPORT_DESCRIPTOR: CTL_CODE(FILE_DEVICE_KEYBOARD, 1, METHOD_NEITHER, FILE_ANY_ACCESS).
const IOCTL_HID_GET_REPORT_DESCRIPTOR: u32 = 0x000B_0007;

fn sparse_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for b in bytes {
        s.push_str(&format!("{b:02x} "));
    }
    s.trim_end().to_string()
}

impl Device {
    /// Open a device for read and write.
    pub fn open(info: Info) -> std::io::Result<Self> {
        Self::open_with_access(info, GENERIC_READ | GENERIC_WRITE)
    }

    /// Open a device for reading only. Windows denies write access to the
    /// keyboard collection, so readers must use this path.
    pub fn open_read(info: Info) -> std::io::Result<Self> {
        Self::open_with_access(info, GENERIC_READ)
    }

    fn open_with_access(info: Info, access: u32) -> std::io::Result<Self> {
        let w = wide(&info.path);
        let handle = unsafe {
            CreateFileW(
                w.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            let err = std::io::Error::last_os_error();
            unsafe { CloseHandle(handle) };
            return Err(err);
        }

        Ok(Self { handle, event, info })
    }

    /// Read one input report. Waits `timeout_ms` for data.
    pub fn read_report_timeout(
        &mut self,
        buf: &mut [u8],
        timeout_ms: u32,
    ) -> std::io::Result<ReadOutcome> {
        // SAFETY: `buf` stays valid until the pending I/O is cancelled or
        // completes, and both paths wait for that before they return.
        unsafe {
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = self.event;

            let ok = ReadFile(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                std::ptr::null_mut(),
                &mut ov,
            );
            if ok == 0 {
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    return Err(std::io::Error::from_raw_os_error(err as i32));
                }
            }

            match WaitForSingleObject(self.event, timeout_ms) {
                WAIT_OBJECT_0 => {
                    let mut read = 0u32;
                    if GetOverlappedResult(self.handle, &ov, &mut read, 0) == 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(ReadOutcome::Data(read as usize))
                }
                WAIT_TIMEOUT => {
                    if !self.cancel_and_drain(&ov) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "device did not release the pending read",
                        ));
                    }
                    Ok(ReadOutcome::Timeout)
                }
                _ => {
                    let err = std::io::Error::last_os_error();
                    self.cancel_and_drain(&ov);
                    Err(err)
                }
            }
        }
    }

    /// Read one input report. Blocks until the device sends data.
    pub fn read_report(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match self.read_report_timeout(buf, DEFAULT_TIMEOUT_MS)? {
                ReadOutcome::Data(n) => return Ok(n),
                ReadOutcome::Timeout => continue,
            }
        }
    }

    /// Write one output report. The first byte is the report ID.
    pub fn write_report(&self, report: &[u8]) -> std::io::Result<usize> {
        // SAFETY: the overlapped structure and the report buffer live for the
        // whole call, and the call waits for completion or cancellation.
        unsafe {
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = self.event;

            let ok = WriteFile(
                self.handle,
                report.as_ptr(),
                report.len() as u32,
                std::ptr::null_mut(),
                &mut ov,
            );
            if ok == 0 {
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    return Err(std::io::Error::from_raw_os_error(err as i32));
                }
            }

            match WaitForSingleObject(self.event, DEFAULT_TIMEOUT_MS) {
                WAIT_OBJECT_0 => {
                    let mut written = 0u32;
                    if GetOverlappedResult(self.handle, &ov, &mut written, 0) == 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(written as usize)
                }
                _ => {
                    let err = std::io::Error::last_os_error();
                    CancelIoEx(self.handle, &ov);
                    WaitForSingleObject(self.event, CANCEL_WAIT_MS);
                    Err(err)
                }
            }
        }
    }

    /// Cancel one pending operation and wait a bounded time for the driver.
    ///
    /// Some HID drivers do not complete a cancelled read at once. The caller
    /// must stop using the buffer when this returns `false`.
    fn cancel_and_drain(&self, ov: &OVERLAPPED) -> bool {
        unsafe {
            CancelIoEx(self.handle, ov);
            WaitForSingleObject(self.event, CANCEL_WAIT_MS) == WAIT_OBJECT_0
        }
    }

    /// Ask the driver to cancel a blocked read so a thread can stop.
    pub fn cancel_pending_io(&self) {
        unsafe {
            CancelIoEx(self.handle, std::ptr::null());
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
            CloseHandle(self.event);
        }
    }
}

/// Default wait used by the blocking helpers.
pub const DEFAULT_TIMEOUT_MS: u32 = 200;

/// How long to wait for a cancelled operation to complete.
pub const CANCEL_WAIT_MS: u32 = 2000;
