//! Embed the application icon and version information in the daemon.
//!
//! The resource compiler comes from the GNU toolchain. When it is missing the
//! build continues, and the tray falls back to the system icon.

fn main() {
    println!("cargo:rerun-if-changed=assets/streamdock.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winres::WindowsResource::new();
    resource.set_icon("assets/streamdock.ico");
    resource.set("FileDescription", "Stream Dock panel daemon");
    resource.set("ProductName", "Stream Dock");
    resource.set("OriginalFilename", "streamdockd.exe");
    resource.set("LegalCopyright", "MIT OR Apache-2.0");

    if let Err(error) = resource.compile() {
        println!("cargo:warning=winres: {error}");
    }
}
