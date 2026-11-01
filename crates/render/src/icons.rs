//! Icons that ship with the renderer.
//!
//! A profile can override any icon with `<profile folder>/icons/<name>.svg`.
//! The files here are simple white shapes, so they read on a dark tile.

macro_rules! icon_set {
    ($($name:literal => $file:literal),* $(,)?) => {
        /// Every bundled icon name, sorted.
        pub const NAMES: &[&str] = &[$($name),*];

        /// The SVG text of one bundled icon.
        pub fn bundled(name: &str) -> Option<&'static str> {
            match name {
                $($name => Some(include_str!(concat!("../assets/icons/", $file))),)*
                _ => None,
            }
        }
    };
}

icon_set! {
    "browser" => "browser.svg",
    "brush" => "brush.svg",
    "calculator" => "calculator.svg",
    "camera" => "camera.svg",
    "check" => "check.svg",
    "copy" => "copy.svg",
    "cross" => "cross.svg",
    "edit" => "edit.svg",
    "folder" => "folder.svg",
    "gear" => "gear.svg",
    "grid" => "grid.svg",
    "home" => "home.svg",
    "lock" => "lock.svg",
    "mail" => "mail.svg",
    "mic" => "mic.svg",
    "mic-off" => "mic-off.svg",
    "minus" => "minus.svg",
    "mute" => "mute.svg",
    "next" => "next.svg",
    "paste" => "paste.svg",
    "pause" => "pause.svg",
    "play" => "play.svg",
    "plus" => "plus.svg",
    "prev" => "prev.svg",
    "redo" => "redo.svg",
    "refresh" => "refresh.svg",
    "save" => "save.svg",
    "screen" => "screen.svg",
    "select" => "select.svg",
    "stop" => "stop.svg",
    "terminal" => "terminal.svg",
    "undo" => "undo.svg",
    "volume" => "volume.svg",
    "volume-down" => "volume-down.svg",
    "wrench" => "wrench.svg",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_has_a_file() {
        for name in NAMES {
            let svg = bundled(name).unwrap_or_else(|| panic!("no file for {name}"));
            assert!(svg.contains("<svg"), "{name} is not an SVG");
        }
    }

    #[test]
    fn an_unknown_name_has_no_file() {
        assert!(bundled("no-such-icon").is_none());
    }

    #[test]
    fn every_bundled_icon_renders() {
        for name in NAMES {
            let svg = bundled(name).expect("svg");
            let tile = crate::svg::render_svg(svg.as_bytes(), (96, 96), [0, 0, 0])
                .unwrap_or_else(|e| panic!("{name} did not render: {e}"));
            let lit = tile
                .data
                .chunks_exact(3)
                .filter(|pixel| *pixel != [0, 0, 0])
                .count();
            assert!(lit > 20, "{name} drew only {lit} pixels");
        }
    }
}
