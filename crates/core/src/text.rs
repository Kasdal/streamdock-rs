//! A 5x7 bitmap font for key labels.
//!
//! The product renderer with SVG and a real text shaper arrives in M4. This
//! font is enough for readable key labels and needs no font file.

/// Width of one glyph in cells.
pub const WIDTH: usize = 5;

/// Height of one glyph in cells.
pub const HEIGHT: usize = 7;

/// Bitmap of one glyph. Bit 4 is the left column and bit 3 of the last row is
/// the bottom row.
pub type Glyph = [u8; HEIGHT];

const BLANK: Glyph = [0; 7];

/// Glyph for one character. Lowercase letters map to their uppercase form.
/// An unknown character returns `None`.
pub fn glyph(ch: char) -> Option<Glyph> {
    let c = ch.to_ascii_uppercase();
    let g = match c {
        ' ' => BLANK,
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        ',' => [0, 0, 0, 0, 0b01100, 0b00100, 0b01000],
        ':' => [0, 0, 0b01100, 0b01100, 0, 0b01100, 0b01100],
        ';' => [0, 0b01100, 0b01100, 0, 0b01100, 0b00100, 0b01000],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        '\\' => [0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001],
        '+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        '=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
        '?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100],
        '%' => [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '#' => [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010],
        '*' => [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],
        '\'' => [0b00100, 0b00100, 0, 0, 0, 0, 0],
        '<' => [0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010],
        '>' => [0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000],
        _ => return None,
    };
    Some(g)
}

/// Break text into lines that fit `max_chars` characters each.
///
/// Words are kept whole when possible. A word that is longer than one line is
/// split. A newline in the input forces a break.
pub fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut lines = Vec::new();

    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word.to_string();
            loop {
                let room = max_chars.saturating_sub(line.chars().count());
                let need_space = !line.is_empty();
                let room = if need_space { room.saturating_sub(1) } else { room };

                if word.chars().count() <= room {
                    if need_space {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }

                if line.is_empty() {
                    // Split a word that is longer than a whole line.
                    let head: String = word.chars().take(max_chars).collect();
                    let tail: String = word.chars().skip(max_chars).collect();
                    lines.push(head);
                    word = tail;
                    if word.is_empty() {
                        break;
                    }
                } else {
                    lines.push(std::mem::take(&mut line));
                }
            }
        }
        lines.push(line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_characters_have_a_glyph() {
        for ch in "ABCXYZ0123456789 .,:;-_/\\+=!?%()#*'".chars() {
            assert!(glyph(ch).is_some(), "missing glyph for {ch:?}");
        }
    }

    #[test]
    fn lowercase_uses_the_uppercase_glyph() {
        assert_eq!(glyph('a'), glyph('A'));
        assert_eq!(glyph('z'), glyph('Z'));
    }

    #[test]
    fn an_unknown_character_has_no_glyph() {
        assert!(glyph('\u{263A}').is_none());
    }

    #[test]
    fn wrap_keeps_words_whole() {
        assert_eq!(wrap("MUTE THE MIC", 7), vec!["MUTE", "THE MIC"]);
        assert_eq!(wrap("MUTE THE MIC", 6), vec!["MUTE", "THE", "MIC"]);
    }

    #[test]
    fn wrap_splits_a_long_word() {
        assert_eq!(wrap("ABCDEFGH", 3), vec!["ABC", "DEF", "GH"]);
    }

    #[test]
    fn wrap_honours_newlines() {
        assert_eq!(wrap("A\nB", 5), vec!["A", "B"]);
    }

    #[test]
    fn wrap_of_empty_text_is_one_empty_line() {
        assert_eq!(wrap("", 5), vec![""]);
    }

    #[test]
    fn glyph_rows_use_five_bits() {
        for ch in "A0Z?".chars() {
            let g = glyph(ch).expect("glyph");
            for row in g {
                assert_eq!(row & 0b1110_0000, 0, "row {row:#010b} of {ch}");
            }
        }
    }
}
