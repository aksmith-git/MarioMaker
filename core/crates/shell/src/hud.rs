//! Minimal HUD: a text strip across the top of the view (lives, coins, score, time) and a centred
//! banner for pause / clear / game over, drawn with a tiny built-in 5×7 bitmap font (original;
//! the game's own HUD layout and font come later). Output is an RGBA8 image the size of the
//! game view, uploaded when its text changes.

use mmk_sim::World;

use crate::session::Phase;

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;
/// Horizontal advance per character (glyph + 1 px gap).
pub const ADVANCE: usize = GLYPH_W + 1;
/// Height of the strip, in view pixels (as in the Swift app).
pub const STRIP_H: usize = 18;

/// The glyph of `c`: 7 rows, bit 4 = leftmost pixel. Unknown characters draw as a box.
pub fn glyph(c: char) -> [u8; GLYPH_H] {
    const fn g(rows: [&str; GLYPH_H]) -> [u8; GLYPH_H] {
        let mut out = [0u8; GLYPH_H];
        let mut r = 0;
        while r < GLYPH_H {
            let b = rows[r].as_bytes();
            let mut x = 0;
            while x < GLYPH_W {
                if b[x] == b'#' {
                    out[r] |= 1 << (GLYPH_W - 1 - x);
                }
                x += 1;
            }
            r += 1;
        }
        out
    }
    match c.to_ascii_uppercase() {
        ' ' => [0; GLYPH_H],
        '0' => g([
            ".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###.",
        ]),
        '1' => g([
            "..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ]),
        '2' => g([
            ".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####",
        ]),
        '3' => g([
            "####.", "....#", "....#", ".###.", "....#", "....#", "####.",
        ]),
        '4' => g([
            "...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#.",
        ]),
        '5' => g([
            "#####", "#....", "####.", "....#", "....#", "#...#", ".###.",
        ]),
        '6' => g([
            "..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###.",
        ]),
        '7' => g([
            "#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#...",
        ]),
        '8' => g([
            ".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###.",
        ]),
        '9' => g([
            ".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##..",
        ]),
        'A' => g([
            "..#..", ".#.#.", "#...#", "#...#", "#####", "#...#", "#...#",
        ]),
        'B' => g([
            "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.",
        ]),
        'C' => g([
            ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.",
        ]),
        'D' => g([
            "###..", "#..#.", "#...#", "#...#", "#...#", "#..#.", "###..",
        ]),
        'E' => g([
            "#####", "#....", "#....", "####.", "#....", "#....", "#####",
        ]),
        'F' => g([
            "#####", "#....", "#....", "####.", "#....", "#....", "#....",
        ]),
        'G' => g([
            ".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####",
        ]),
        'H' => g([
            "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ]),
        'I' => g([
            ".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ]),
        'J' => g([
            "..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##..",
        ]),
        'K' => g([
            "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
        ]),
        'L' => g([
            "#....", "#....", "#....", "#....", "#....", "#....", "#####",
        ]),
        'M' => g([
            "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#",
        ]),
        'N' => g([
            "#...#", "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#",
        ]),
        'O' => g([
            ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ]),
        'P' => g([
            "####.", "#...#", "#...#", "####.", "#....", "#....", "#....",
        ]),
        'Q' => g([
            ".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#",
        ]),
        'R' => g([
            "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#",
        ]),
        'S' => g([
            ".####", "#....", "#....", ".###.", "....#", "....#", "####.",
        ]),
        'T' => g([
            "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..",
        ]),
        'U' => g([
            "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ]),
        'V' => g([
            "#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#..",
        ]),
        'W' => g([
            "#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#.",
        ]),
        'X' => g([
            "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#",
        ]),
        'Y' => g([
            "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#..",
        ]),
        'Z' => g([
            "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
        ]),
        '×' => g([
            ".....", ".....", "#...#", ".#.#.", "..#..", ".#.#.", "#...#",
        ]),
        '!' => g([
            "..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#..",
        ]),
        '?' => g([
            ".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#..",
        ]),
        '-' => g([
            ".....", ".....", ".....", ".###.", ".....", ".....", ".....",
        ]),
        ':' => g([
            ".....", "..#..", "..#..", ".....", "..#..", "..#..", ".....",
        ]),
        '.' => g([
            ".....", ".....", ".....", ".....", ".....", ".##..", ".##..",
        ]),
        ',' => g([
            ".....", ".....", ".....", ".....", ".##..", "..#..", ".#...",
        ]),
        '/' => g([
            "....#", "....#", "...#.", "..#..", ".#...", "#....", "#....",
        ]),
        '(' => g([
            "...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#.",
        ]),
        ')' => g([
            ".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#...",
        ]),
        '\'' | '’' => g([
            "..#..", "..#..", ".#...", ".....", ".....", ".....", ".....",
        ]),
        '&' => g([
            ".##..", "#..#.", ".##..", ".#...", "#.#.#", "#..#.", ".##.#",
        ]),
        '%' => g([
            "##..#", "##..#", "...#.", "..#..", ".#...", "#..##", "#..##",
        ]),
        _ => g([
            "#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#####",
        ]),
    }
}

/// An RGBA8 image (top row first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; width * height * 4],
        }
    }

    fn put(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let o = (y as usize * self.width + x as usize) * 4;
            self.rgba[o..o + 4].copy_from_slice(&c);
        }
    }

    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: [u8; 4]) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(self.width) {
                self.put(xx as i64, yy as i64, c);
            }
        }
    }

    /// Draw `text` with its top-left at (x, y), each font pixel `scale`² image pixels, with a
    /// 1-font-pixel black drop shadow.
    pub fn text(&mut self, text: &str, x: i64, y: i64, scale: usize, color: [u8; 4]) {
        for (dx, dy, c) in [(1, 1, [0, 0, 0, 255]), (0, 0, color)] {
            let mut cx = x;
            for ch in text.chars() {
                let rows = glyph(ch);
                for (r, bits) in rows.iter().enumerate() {
                    for col in 0..GLYPH_W {
                        if bits & (1 << (GLYPH_W - 1 - col)) == 0 {
                            continue;
                        }
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.put(
                                    cx + ((col + dx) * scale + sx) as i64,
                                    y + ((r + dy) * scale + sy) as i64,
                                    c,
                                );
                            }
                        }
                    }
                }
                cx += (ADVANCE * scale) as i64;
            }
        }
    }
}

/// Width in pixels of `text` at `scale`.
pub fn text_width(text: &str, scale: usize) -> usize {
    (text.chars().count() * ADVANCE).saturating_sub(1) * scale
}

/// What the HUD shows; compare to decide whether the image must be redrawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HudText {
    pub strip: String,
    pub banner: Option<(String, String)>,
}

impl HudText {
    pub fn of(world: &World, phase: Phase) -> Self {
        let h = world.hud();
        let time = if h.time < 0 {
            "---".to_string()
        } else {
            format!("{:03}", h.time)
        };
        let strip = format!(
            "LIVES×{:02}   COINS×{:02}   {:09}   TIME {time}",
            h.lives, h.coins, h.score
        );
        let banner = match phase {
            Phase::Playing => None,
            Phase::Paused => Some(("PAUSED", "ESC/P CONTINUE   R RESTART")),
            Phase::Cleared => Some(("COURSE CLEAR!", "Z/START PLAY AGAIN")),
            Phase::GameOver => Some(("GAME OVER", "Z/START TRY AGAIN")),
        }
        .map(|(a, b)| (a.to_string(), b.to_string()));
        Self { strip, banner }
    }

    /// Draw onto a transparent image of the game view's size.
    pub fn draw(&self, width: usize, height: usize) -> Image {
        let mut img = Image::new(width, height);
        let white = [255, 255, 255, 255];
        let y = ((STRIP_H - GLYPH_H) / 2) as i64;
        let x = ((width.saturating_sub(text_width(&self.strip, 1))) / 2) as i64;
        img.text(&self.strip, x, y, 1, white);
        if let Some((title, hint)) = &self.banner {
            let (tw, hw) = (text_width(title, 2), text_width(hint, 1));
            let bw = tw.max(hw) + 24;
            let bh = GLYPH_H * 2 + GLYPH_H + 6 + 20;
            let (bx, by) = (width.saturating_sub(bw) / 2, height.saturating_sub(bh) / 2);
            img.fill(bx, by, bw, bh, [0, 0, 0, 170]);
            let ty = by + 10;
            img.text(title, ((width - tw) / 2) as i64, ty as i64, 2, white);
            let hy = ty + GLYPH_H * 2 + 6;
            img.text(
                hint,
                ((width - hw) / 2) as i64,
                hy as i64,
                1,
                [200, 200, 200, 255],
            );
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_and_letters_are_distinct_and_fit_the_strip() {
        let chars: Vec<char> = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ×!-:./"
            .chars()
            .collect();
        for (i, a) in chars.iter().enumerate() {
            for b in &chars[i + 1..] {
                assert_ne!(glyph(*a), glyph(*b), "{a} and {b} look the same");
            }
        }
        let t = HudText::of(&World::new(), Phase::Playing);
        assert!(t.strip.starts_with("LIVES×"), "{}", t.strip);
        assert!(text_width(&t.strip, 1) <= 384);
        let img = t.draw(384, 216);
        assert_eq!(img.rgba.len(), 384 * 216 * 4);
        // something white in the strip, nothing below it
        let white_rows: Vec<usize> = (0..216)
            .filter(|y| (0..384).any(|x| img.rgba[(y * 384 + x) * 4] == 255))
            .collect();
        assert!(!white_rows.is_empty() && *white_rows.last().unwrap() < STRIP_H);
    }

    #[test]
    fn banner_only_when_not_playing() {
        assert!(HudText::of(&World::new(), Phase::Playing).banner.is_none());
        let t = HudText::of(&World::new(), Phase::Paused);
        assert_eq!(t.banner.as_ref().unwrap().0, "PAUSED");
        let img = t.draw(384, 216);
        assert!(
            img.rgba[(108 * 384 + 192) * 4 + 3] > 0,
            "banner box in the middle"
        );
    }
}
