//! The CDX window frame, drawn pixel by pixel (the same measures as cdx-ui/cdx-ui.css: `.win`, `.tb`, `.tb-btn`).
//!
//! Everything here is plain arithmetic on a buffer of 0xAARRGGBB pixels, so it can be tested and previewed without a
//! window. A transparent pixel (alpha 0) lets the mouse and the eye through to the window underneath.

use serde_json::Value;
use std::collections::HashMap;

pub type Color = u32;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}
pub const BLACK: Color = rgb(0, 0, 0);
pub const WHITE: Color = rgb(255, 255, 255);

/// Frame thickness on the sides and at the bottom, in font pixels (1 outline + 5 of rings).
pub const RING: i32 = 6;
/// Height of the title bar, outline included.
pub const TITLE: i32 = 21;
/// Side of a title bar button.
pub const BUTTON: i32 = 14;

// ------------------------------------------------------------------ layout

/// The measures of the frame around one window, in screen pixels (`s` screen pixels per font pixel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub s: i32,
    /// Size of the whole overlay.
    pub w: i32,
    pub h: i32,
    /// Height of the native title bar that the frame covers, in screen pixels (at least the CDX title bar).
    pub caption: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    None,
    Close,
    Menu,
    Zoom,
    Shade,
    Title,
    /// A side of the frame: the direction of `SC_SIZE` (1 left, 2 right, 3 top, 4 top-left, 5 top-right, 6 bottom, 7 bottom-left, 8 bottom-right).
    Edge(u8),
}

impl Layout {
    /// The overlay around a window of `win_w` x `win_h` visible pixels whose native title bar is `caption` high.
    pub fn around(win_w: i32, win_h: i32, caption: i32, s: i32) -> Layout {
        Layout { s, w: win_w + 2 * RING * s, h: win_h + (1 + RING) * s, caption: caption.max(TITLE * s) }
    }

    /// Where the title bar is, in overlay pixels: (x, y, width, height) with the black outline row on top.
    fn title_box(&self) -> (i32, i32, i32, i32) {
        (self.s, self.s, self.w - 2 * self.s, (TITLE - 1) * self.s + self.s)
    }

    fn button_box(&self, which: Hit) -> (i32, i32, i32, i32) {
        let (tx, ty, tw, _) = self.title_box();
        let s = self.s;
        let (x, w) = match which {
            Hit::Close => (4, BUTTON),
            Hit::Menu => (26, 32),
            Hit::Zoom => ((tw / s) - 22 - BUTTON, BUTTON),
            _ => ((tw / s) - 4 - BUTTON, BUTTON), // Shade
        };
        (tx + x * s, ty + 3 * s, w * s, BUTTON * s)
    }

    /// What is under the point (x, y) of the overlay.
    pub fn hit(&self, x: i32, y: i32) -> Hit {
        let s = self.s;
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return Hit::None;
        }
        for b in [Hit::Close, Hit::Menu, Hit::Zoom, Hit::Shade] {
            let (bx, by, bw, bh) = self.button_box(b);
            if x >= bx && x < bx + bw && y >= by && y < by + bh {
                return b;
            }
        }
        let corner = 2 * RING * s;
        let (left, right) = (x < RING * s, x >= self.w - RING * s);
        let bottom = y >= self.h - RING * s;
        let title_end = (TITLE + 1) * s;
        if y < 3 * s {
            return Hit::Edge(if x < corner { 4 } else if x >= self.w - corner { 5 } else { 3 });
        }
        if bottom {
            return Hit::Edge(if x < corner { 7 } else if x >= self.w - corner { 8 } else { 6 });
        }
        if y >= title_end && left {
            return Hit::Edge(1);
        }
        if y >= title_end && right {
            return Hit::Edge(2);
        }
        if y < title_end {
            if left || right {
                return Hit::Edge(if left { 1 } else { 2 });
            }
            return Hit::Title;
        }
        // the strip under the title bar that hides the rest of the native one: it drags like a title
        if y < s + self.caption && x >= RING * s && x < self.w - RING * s {
            return Hit::Title;
        }
        Hit::None
    }
}

// ------------------------------------------------------------------ colours

/// The colours of a frame, active (accent) or not (grey), as in cdx-ui.css.
#[derive(Clone, Debug)]
pub struct Palette {
    /// Rings, top-left side: outer, middle, inner; bottom-right side: inner, middle, outer.
    pub ring_tl: [Color; 3],
    pub ring_br: [Color; 3],
    pub tb_edge: Color,
    pub tb_low: Color,
    /// Gradient of the title bar, top to bottom: (position 0..1, colour).
    pub tb_grad: Vec<(f32, Color)>,
    pub btn_fill: Color,
    pub btn_hi: Color,
    pub btn_lo: Color,
}

pub const PANEL: Color = rgb(0x33, 0x33, 0x33);

pub fn palette(active: bool) -> Palette {
    if active {
        // the CDX red ramp (--a-edge #cc0000, --a-mid #880000, --a-dark #440000, title #320000 -> #c30000)
        let (edge, mid, dark) = (rgb(0xcc, 0, 0), rgb(0x88, 0, 0), rgb(0x44, 0, 0));
        Palette {
            ring_tl: [edge, mid, dark],
            ring_br: [dark, mid, edge],
            tb_edge: edge,
            tb_low: dark,
            tb_grad: vec![(0.0, rgb(0x32, 0, 0)), (1.0, rgb(0xc3, 0, 0))],
            btn_fill: mid,
            btn_hi: edge,
            btn_lo: dark,
        }
    } else {
        Palette {
            ring_tl: [rgb(0x55, 0x55, 0x55), rgb(0x22, 0x22, 0x22), rgb(0x11, 0x11, 0x11)],
            ring_br: [rgb(0x11, 0x11, 0x11), rgb(0x22, 0x22, 0x22), rgb(0x55, 0x55, 0x55)],
            tb_edge: rgb(0x55, 0x55, 0x55),
            tb_low: rgb(0x22, 0x22, 0x22),
            tb_grad: vec![(0.0, rgb(0x0c, 0x0c, 0x0c)), (0.47, rgb(0x22, 0x22, 0x22)), (1.0, rgb(0x8f, 0x8f, 0x8f))],
            btn_fill: rgb(0x44, 0x44, 0x44),
            btn_hi: rgb(0x8a, 0x8a, 0x8a),
            btn_lo: rgb(0x22, 0x22, 0x22),
        }
    }
}

/// A colour made lighter (the button under the mouse: CSS `filter: brightness(1.35)`).
pub fn bright(c: Color) -> Color {
    let ch = |sh: u32| ((((c >> sh) & 0xff) as f32 * 1.35).round() as u32).min(255);
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    let ch = |sh: u32| {
        let (x, y) = (((a >> sh) & 0xff) as f32, ((b >> sh) & 0xff) as f32);
        (x + (y - x) * t).round() as u32
    };
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn gradient_at(stops: &[(f32, Color)], t: f32) -> Color {
    for w in stops.windows(2) {
        let ((p0, c0), (p1, c1)) = (w[0], w[1]);
        if t <= p1 {
            return lerp(c0, c1, ((t - p0) / (p1 - p0)).clamp(0.0, 1.0));
        }
    }
    stops.last().map_or(BLACK, |s| s.1)
}

// ------------------------------------------------------------------ canvas

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { w, h, px: vec![0; (w.max(0) * h.max(0)) as usize] }
    }

    pub fn put(&mut self, x: i32, y: i32, c: Color) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Color {
        if x >= 0 && y >= 0 && x < self.w && y < self.h { self.px[(y * self.w + x) as usize] } else { 0 }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.px[(yy * self.w + xx) as usize] = c;
            }
        }
    }
}

// ------------------------------------------------------------------ the font

pub struct Font {
    spacing: i32,
    space: i32,
    glyphs: HashMap<char, (i32, Vec<Vec<bool>>)>,
}

impl Font {
    pub fn load(json: &str) -> Font {
        let v: Value = serde_json::from_str(json).expect("ui-font.json");
        let mut glyphs = HashMap::new();
        for (k, g) in v["glyphs"].as_object().expect("glyphs") {
            let Some(ch) = k.chars().next() else { continue };
            let top = g[0].as_i64().unwrap_or(0) as i32;
            let rows = g[1].as_array().map(|r| r.iter().map(|row| row.as_str().unwrap_or("").chars().map(|c| c == '#').collect()).collect()).unwrap_or_default();
            glyphs.insert(ch, (top, rows));
        }
        Font { spacing: v["spacing"].as_i64().unwrap_or(2) as i32, space: v["space"].as_i64().unwrap_or(4) as i32, glyphs }
    }

    /// Width of a text, in font pixels.
    pub fn width(&self, text: &str) -> i32 {
        let mut w = 0;
        for ch in text.chars() {
            w += self.advance(ch);
        }
        (w - self.spacing).max(0)
    }

    fn advance(&self, ch: char) -> i32 {
        if ch == ' ' {
            return self.space;
        }
        let g = self.glyphs.get(&ch).or_else(|| self.glyphs.get(&'?'));
        g.map_or(self.space, |(_, rows)| rows.iter().map(Vec::len).max().unwrap_or(0) as i32 + self.spacing)
    }

    /// Draws `text` with its baseline grid starting at (x, y), every font pixel being `s` screen pixels.
    pub fn draw(&self, c: &mut Canvas, x: i32, y: i32, text: &str, color: Color, s: i32) {
        let mut cx = x;
        for ch in text.chars() {
            if ch != ' ' {
                if let Some((top, rows)) = self.glyphs.get(&ch).or_else(|| self.glyphs.get(&'?')) {
                    for (r, row) in rows.iter().enumerate() {
                        for (col, on) in row.iter().enumerate() {
                            if *on {
                                c.rect(cx + col as i32 * s, y + (top + r as i32) * s, s, s, color);
                            }
                        }
                    }
                }
            }
            cx += self.advance(ch) * s;
        }
    }
}

// ------------------------------------------------------------------ the frame

const X: [&str; 8] = ["##....##", "###..###", ".######.", "..####..", "..####..", ".######.", "###..###", "##....##"];
const MINUS: [&str; 8] = ["........", "........", "........", "########", "########", "........", "........", "........"];
const PLUS: [&str; 8] = ["...##...", "...##...", "...##...", "########", "########", "...##...", "...##...", "...##..."];

fn glyph(c: &mut Canvas, x: i32, y: i32, rows: &[&str; 8], s: i32) {
    for (r, row) in rows.iter().enumerate() {
        for (col, ch) in row.chars().enumerate() {
            if ch == '#' {
                c.rect(x + col as i32 * s, y + r as i32 * s, s, s, WHITE);
            }
        }
    }
}

/// A title bar button: black outline, light top-left edge, fill, dark bottom-right edge (14 x 14 font pixels).
fn button(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), p: &Palette, s: i32) {
    c.rect(x, y, w, h, BLACK);
    c.rect(x + s, y + s, w - 2 * s, h - 2 * s, p.btn_fill);
    c.rect(x + s, y + s, w - 2 * s, s, p.btn_hi);
    c.rect(x + s, y + s, s, h - 2 * s, p.btn_hi);
    c.rect(x + s, y + h - 2 * s, w - 2 * s, s, p.btn_lo);
    c.rect(x + w - 2 * s, y + s, s, h - 2 * s, p.btn_lo);
}

/// Draws the whole frame: rings along the sides and the bottom, the title bar, and the strip that hides the rest of the
/// native title bar. The middle stays transparent: that is the window itself.
pub fn draw_frame(c: &mut Canvas, l: &Layout, p: &Palette, title: &str, font: &Font, hover: Hit) {
    let s = l.s;
    let (w, h) = (l.w, l.h);
    let below = (TITLE + 1) * s; // where the side rings start (under the title bar)

    // the sides and the bottom: outline, then four lines, then the black underlay
    for i in 0..RING {
        let (x_l, x_r) = (i * s, w - (i + 1) * s);
        let layer_tl = match i { 0 => BLACK, 1 => p.ring_tl[0], 2 | 3 => p.ring_tl[1], 4 => p.ring_tl[2], _ => BLACK };
        let layer_br = match i { 0 => BLACK, 1 => p.ring_br[0], 2 | 3 => p.ring_br[1], 4 => p.ring_br[2], _ => BLACK };
        c.rect(x_l, below, s, h - below, layer_tl);
        c.rect(x_r, below, s, h - below, layer_br);
        let y_b = h - (i + 1) * s;
        let (lx, rx) = (i * s, w - i * s);
        c.rect(lx, y_b, rx - lx, s, layer_br);
    }
    // the bottom corners take the colours of the left side down to the diagonal (a simple mitre)
    for i in 0..RING {
        for j in 0..RING {
            let (x, y) = (i * s, h - (j + 1) * s);
            let layer = |k: i32, set: &[Color; 3]| match k { 0 => BLACK, 1 => set[0], 2 | 3 => set[1], 4 => set[2], _ => BLACK };
            let col = if i <= j { layer(i, &p.ring_tl) } else { layer(j, &p.ring_br) };
            c.rect(x, y, s, s, col);
            let (x2, y2) = (w - (i + 1) * s, h - (j + 1) * s);
            let col2 = if i <= j { layer(i, &p.ring_br) } else { layer(j, &p.ring_br) };
            c.rect(x2, y2, s, s, col2);
        }
    }

    // the title bar: outline on top, then edge line, gradient, low line, black line
    let (tx, ty, tw, _) = (s, s, w - 2 * s, 0);
    c.rect(0, 0, w, s, BLACK); // top outline
    c.rect(0, 0, s, below, BLACK);
    c.rect(w - s, 0, s, below, BLACK);
    let rows = TITLE - 1; // 20 rows between the outline and the bottom black line
    for r in 0..rows {
        let color = if r == 0 {
            p.tb_edge
        } else if r == rows - 1 {
            p.tb_low
        } else {
            gradient_at(&p.tb_grad, (r - 1) as f32 / (rows - 3).max(1) as f32)
        };
        c.rect(tx, ty + r * s, tw, s, color);
        c.rect(tx, ty + r * s, s, s, p.tb_edge); // light left edge
        c.rect(tx + tw - s, ty + r * s, s, s, p.tb_low); // dark right edge
    }
    c.rect(tx, ty + rows * s, tw, s, BLACK); // the black line under the bar

    // the strip that covers what is left of the native title bar
    let strip_end = s + l.caption;
    if strip_end > below {
        c.rect(RING * s, below, w - 2 * RING * s, strip_end - below, PANEL);
    }

    // buttons and title
    for b in [Hit::Close, Hit::Menu, Hit::Zoom, Hit::Shade] {
        let bx = l.button_box(b);
        if hover == b {
            let lit = Palette { btn_fill: bright(p.btn_fill), btn_hi: bright(p.btn_hi), btn_lo: bright(p.btn_lo), ..p.clone() };
            button(c, bx, &lit, s);
        } else {
            button(c, bx, p, s);
        }
        let (x, y, bw, bh) = bx;
        match b {
            Hit::Close => glyph(c, x + 3 * s, y + 3 * s, &X, s),
            Hit::Zoom => glyph(c, x + 3 * s, y + 3 * s, &PLUS, s),
            Hit::Shade => glyph(c, x + 3 * s, y + 3 * s, &MINUS, s),
            _ => {
                // the window menu: white diagonal stripes on the fill
                for yy in s..bh - s {
                    for xx in s..bw - s {
                        if ((xx / s + yy / s) % 7) < 3 {
                            c.rect(x + xx, y + yy, 1, 1, WHITE);
                        }
                    }
                }
                for yy in 0..bh {
                    for xx in 0..bw {
                        if (xx < s || yy < s || xx >= bw - s || yy >= bh - s) && c.get(x + xx, y + yy) == WHITE {
                            c.put(x + xx, y + yy, BLACK);
                        }
                    }
                }
            }
        }
    }
    let text_w = font.width(title) * s;
    let left = l.button_box(Hit::Menu).0 + 32 * s + 6 * s;
    let right = l.button_box(Hit::Zoom).0 - 6 * s;
    let mut shown = title.to_string();
    let mut tw_px = text_w;
    while tw_px > right - left && shown.chars().count() > 1 {
        shown.pop();
        tw_px = font.width(&format!("{shown}…")) * s;
    }
    if shown != title {
        shown.push('…');
    }
    let x = (left + (right - left - font.width(&shown) * s) / 2).max(left);
    let y = ty + 3 * s;
    font.draw(c, x, y + s, &shown, BLACK, s); // the shadow, one pixel down
    font.draw(c, x, y, &shown, WHITE, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Font {
        Font::load(include_str!("../assets/ui-font.json"))
    }

    fn frame(active: bool) -> (Canvas, Layout) {
        let l = Layout::around(300, 200, 31, 1);
        let mut c = Canvas::new(l.w, l.h);
        draw_frame(&mut c, &l, &palette(active), "Bloc-notes", &font(), Hit::None);
        (c, l)
    }

    #[test]
    fn the_window_itself_stays_transparent() {
        let (c, l) = frame(true);
        assert_eq!(c.get(l.w / 2, l.h / 2), 0, "the middle is the window");
        assert_eq!(c.get(RING + 5, TITLE + 40), 0);
    }

    #[test]
    fn the_rings_are_the_four_lines_of_the_css_frame() {
        // from the left edge inwards: outline, light line, two mid lines, dark line, black underlay
        let (c, l) = frame(false);
        let y = l.h / 2;
        let got: Vec<Color> = (0..RING).map(|x| c.get(x, y)).collect();
        assert_eq!(got, [BLACK, rgb(0x55, 0x55, 0x55), rgb(0x22, 0x22, 0x22), rgb(0x22, 0x22, 0x22), rgb(0x11, 0x11, 0x11), BLACK]);
        // from the right edge inwards the light and the dark swap sides
        let got: Vec<Color> = (0..RING).map(|i| c.get(l.w - 1 - i, y)).collect();
        assert_eq!(got, [BLACK, rgb(0x11, 0x11, 0x11), rgb(0x22, 0x22, 0x22), rgb(0x22, 0x22, 0x22), rgb(0x55, 0x55, 0x55), BLACK]);
    }

    #[test]
    fn the_active_frame_is_red() {
        let (c, l) = frame(true);
        assert_eq!(c.get(1, l.h / 2), rgb(0xcc, 0, 0));
        assert_eq!(c.get(3, l.h / 2), rgb(0x88, 0, 0));
        assert_eq!(c.get(4, l.h / 2), rgb(0x44, 0, 0));
    }

    #[test]
    fn the_title_bar_has_its_edge_gradient_and_low_line() {
        let (c, l) = frame(false);
        let x = l.w / 2 + 70; // away from the title text
        assert_eq!(c.get(x, 0), BLACK, "outline");
        assert_eq!(c.get(x, 1), rgb(0x55, 0x55, 0x55), "light line");
        assert_eq!(c.get(x, 2), rgb(0x0c, 0x0c, 0x0c), "the gradient starts dark");
        assert_eq!(c.get(x, TITLE - 1), rgb(0x22, 0x22, 0x22), "low line");
        assert_eq!(c.get(x, TITLE), BLACK, "black line under the bar");
        let lit = c.get(x, TITLE - 2);
        assert!(lit >> 16 & 0xff > 0x70, "and ends light grey, got {lit:08x}");
    }

    #[test]
    fn the_buttons_are_14_pixels_with_a_light_top_left_edge() {
        let (c, l) = frame(false);
        let (x, y, w, h) = l.button_box(Hit::Close);
        assert_eq!((w, h), (BUTTON, BUTTON));
        assert_eq!(c.get(x, y), BLACK);
        assert_eq!(c.get(x + 1, y + 1), rgb(0x8a, 0x8a, 0x8a));
        assert_eq!(c.get(x + w - 2, y + h - 2), rgb(0x22, 0x22, 0x22));
        assert_eq!(c.get(x + 6, y + 2), rgb(0x44, 0x44, 0x44), "the fill");
        assert_eq!(c.get(x + 3, y + 3), WHITE, "the cross");
    }

    #[test]
    fn the_button_under_the_mouse_is_lighter() {
        let l = Layout::around(300, 200, 31, 1);
        let mut still = Canvas::new(l.w, l.h);
        draw_frame(&mut still, &l, &palette(false), "x", &font(), Hit::None);
        let mut over = Canvas::new(l.w, l.h);
        draw_frame(&mut over, &l, &palette(false), "x", &font(), Hit::Close);
        let (x, y, ..) = l.button_box(Hit::Close);
        assert_eq!(still.get(x + 6, y + 2), rgb(0x44, 0x44, 0x44));
        assert_eq!(over.get(x + 6, y + 2), bright(rgb(0x44, 0x44, 0x44)));
        let z = l.button_box(Hit::Zoom);
        assert_eq!(still.get(z.0 + 6, z.1 + 2), over.get(z.0 + 6, z.1 + 2), "the other buttons do not change");
    }

    #[test]
    fn the_title_is_drawn_in_white_with_a_shadow() {
        let (c, l) = frame(true);
        let mut white = 0;
        for y in 2..TITLE {
            for x in 70..l.w - 70 {
                if c.get(x, y) == WHITE {
                    white += 1;
                }
            }
        }
        assert!(white > 30, "{white} white pixels of text");
    }

    #[test]
    fn the_strip_under_the_title_hides_the_rest_of_the_native_bar() {
        let (c, l) = frame(false);
        assert_eq!(c.get(l.w / 2, TITLE + 2), PANEL);
        assert_eq!(c.get(l.w / 2, 1 + l.caption), 0, "and no further");
    }

    #[test]
    fn what_is_under_the_mouse() {
        let l = Layout::around(300, 200, 31, 1);
        let b = l.button_box(Hit::Close);
        assert_eq!(l.hit(b.0 + 5, b.1 + 5), Hit::Close);
        let z = l.button_box(Hit::Zoom);
        assert_eq!(l.hit(z.0 + 5, z.1 + 5), Hit::Zoom);
        assert_eq!(l.hit(l.w / 2, 10), Hit::Title);
        assert_eq!(l.hit(l.w / 2, TITLE + 5), Hit::Title, "the strip drags too");
        assert_eq!(l.hit(1, l.h / 2), Hit::Edge(1));
        assert_eq!(l.hit(l.w - 2, l.h / 2), Hit::Edge(2));
        assert_eq!(l.hit(l.w / 2, l.h - 2), Hit::Edge(6));
        assert_eq!(l.hit(2, l.h - 2), Hit::Edge(7));
        assert_eq!(l.hit(l.w - 2, l.h - 2), Hit::Edge(8));
        assert_eq!(l.hit(l.w / 2, 1), Hit::Edge(3));
        assert_eq!(l.hit(l.w / 2, l.h / 2), Hit::None, "the window itself");
        assert_eq!(l.hit(-1, 5), Hit::None);
    }

    #[test]
    fn scaled_up_the_frame_keeps_its_proportions() {
        let l = Layout::around(300, 200, 62, 2);
        assert_eq!((l.w, l.h), (300 + 24, 200 + 14));
        let mut c = Canvas::new(l.w, l.h);
        draw_frame(&mut c, &l, &palette(true), "x", &font(), Hit::None);
        assert_eq!(c.get(2, l.h / 2), rgb(0xcc, 0, 0), "a font pixel is two screen pixels");
        assert_eq!(c.get(3, l.h / 2), rgb(0xcc, 0, 0));
        assert_eq!(c.get(4, l.h / 2), rgb(0x88, 0, 0));
    }

    #[test]
    fn the_font_measures_like_the_css_one() {
        let f = font();
        assert_eq!(f.width(""), 0);
        assert!(f.width("Bloc-notes") > f.width("Bloc"));
        assert_eq!(f.width("A B"), f.width("A") + 2 + 4 + f.width("B"), "letters two apart, a space four wide");
    }
}
