//! Tray icon and its menu.
//!
//! The icon lives on its own thread with a hidden window. The menu can open
//! the editor, reload the profile or stop the daemon. A daemon that stops from
//! the menu removes the icon first, so no dead icon stays in the tray.

use std::mem::size_of;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW, HICON, LoadCursorW, LoadIconW,
    PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetWindowLongPtrW,
    TrackPopupMenu, TranslateMessage, GWLP_USERDATA, IDC_ARROW, IDI_APPLICATION, MF_SEPARATOR,
    MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

use streamdock_ipc::Command;

/// Menu command ids.
const MENU_OPEN: usize = 1;
const MENU_RELOAD: usize = 2;
const MENU_QUIT: usize = 3;

/// Message that the shell sends for the icon.
const WM_TRAY: u32 = WM_APP + 1;

struct Context {
    url: String,
    commands: Sender<Command>,
    on_quit: fn(),
}

/// A running tray icon.
pub struct Tray {
    window: Arc<Mutex<isize>>,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    /// Create the icon on its own thread.
    pub fn spawn(url: String, commands: Sender<Command>, on_quit: fn()) -> std::io::Result<Self> {
        let window = Arc::new(Mutex::new(0isize));
        let shared = Arc::clone(&window);

        let thread = thread::Builder::new()
            .name("streamdock-tray".into())
            .spawn(move || run(url, commands, on_quit, shared))?;

        Ok(Self {
            window,
            thread: Some(thread),
        })
    }

    /// Remove the icon and stop the thread.
    pub fn stop(&mut self) {
        let handle = *self.window.lock().expect("tray window lock");
        if handle != 0 {
            // SAFETY: the handle is a window that this thread created.
            unsafe {
                PostMessageW(handle as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The icon from the executable resource, or the system icon.
fn app_icon(instance: windows_sys::Win32::Foundation::HINSTANCE) -> HICON {
    // Resource id 1 is the icon that `build.rs` embeds.
    let icon = unsafe { LoadIconW(instance, 1 as *const u16) };
    if icon.is_null() {
        unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
    } else {
        icon
    }
}

fn run(url: String, commands: Sender<Command>, on_quit: fn(), window: Arc<Mutex<isize>>) {
    // SAFETY: every pointer below comes from Win32 and lives until the loop
    // ends. The context box is released after the message loop.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = wide("StreamDockTrayWindow");
        let title = wide("Stream Dock");
        let icon_handle = app_icon(instance);

        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: icon_handle,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&class) == 0 {
            return;
        }

        let handle = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if handle.is_null() {
            return;
        }

        let context = Box::into_raw(Box::new(Context {
            url,
            commands,
            on_quit,
        }));
        SetWindowLongPtrW(handle, GWLP_USERDATA, context as isize);

        if let Ok(mut guard) = window.lock() {
            *guard = handle as isize;
        }

        let mut icon: NOTIFYICONDATAW = std::mem::zeroed();
        icon.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        icon.hWnd = handle;
        icon.uID = 1;
        icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        icon.uCallbackMessage = WM_TRAY;
        icon.hIcon = icon_handle;
        let tip = wide("Stream Dock");
        let count = tip.len().min(icon.szTip.len());
        icon.szTip[..count].copy_from_slice(&tip[..count]);

        Shell_NotifyIconW(NIM_ADD, &icon);

        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        Shell_NotifyIconW(NIM_DELETE, &icon);
        drop(Box::from_raw(context));

        if let Ok(mut guard) = window.lock() {
            *guard = 0;
        }
    }
}

unsafe extern "system" fn window_proc(
    handle: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TRAY => {
            let event = (lparam as u32) & 0xFFFF;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU || event == WM_LBUTTONUP {
                show_menu(handle);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(handle);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(handle, message, wparam, lparam),
    }
}

unsafe fn show_menu(handle: HWND) {
    let pointer = GetWindowLongPtrW(handle, GWLP_USERDATA) as *mut Context;
    if pointer.is_null() {
        return;
    }
    let context = &*pointer;

    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }

    AppendMenuW(menu, MF_STRING, MENU_OPEN, wide("Open editor").as_ptr());
    AppendMenuW(menu, MF_STRING, MENU_RELOAD, wide("Reload profile").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(menu, MF_STRING, MENU_QUIT, wide("Quit").as_ptr());

    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    SetForegroundWindow(handle);

    let picked = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD,
        point.x,
        point.y,
        0,
        handle,
        std::ptr::null(),
    );
    DestroyMenu(menu);

    match picked as usize {
        MENU_OPEN => {
            let _ = streamdock_actions::launch(&context.url);
        }
        MENU_RELOAD => {
            let (reply, answer) = channel();
            let _ = context.commands.send(Command::Reload { reply });
            drop(answer);
        }
        MENU_QUIT => {
            (context.on_quit)();
            DestroyWindow(handle);
        }
        _ => {}
    }

    // Required by the shell: a posted message lets the menu close.
    PostMessageW(handle, WM_NULL, 0, 0);
}
