//! kdx-frames: the KDX window frame around the windows of the programs named on the command line.
//!
//!     kdx-frames.exe                            decorates the programs of the list in %APPDATA%\KDX-Frames\programmes.txt
//!     kdx-frames.exe programme.exe ...          decorates these programs instead of the list
//!     kdx-frames.exe --console ...              keeps the console window (Ctrl+C then puts the windows back as they were)
//!     kdx-frames.exe --preview image.png        draws a sample frame to an image, without touching any window
//!
//! Only the windows of the listed programs are touched: nothing else on the desktop changes.

mod config;
mod paint;
#[cfg(windows)]
mod overlay;

use paint::{draw_frame, palette, Canvas, Font, Layout, BLACK};

/// Both frames (active and not) on a sample window, to look at the result without any window.
fn preview(path: &str) {
    let font = Font::load(include_str!("../assets/ui-font.json"));
    let (ww, wh) = (360, 150);
    let l = Layout::around(ww, wh, 31, 1);
    let mut sheet = Canvas::new(l.w * 2 + 90, l.h + 60);
    let wall = 0xFF3A0000;
    sheet.rect(0, 0, sheet.w, sheet.h, wall);
    for (i, active) in [true, false].into_iter().enumerate() {
        let mut frame = Canvas::new(l.w, l.h);
        draw_frame(&mut frame, &l, &palette(active), if active { "Bloc-notes" } else { "Terminal — cargo test" }, &font, if active { paint::Hit::Zoom } else { paint::Hit::None });
        let (ox, oy) = (30 + i as i32 * (l.w + 30), 30);
        // a window behind the frame (dark app content), then the frame over it
        sheet.rect(ox + paint::RING, oy + 1, ww, wh, 0xFF202020);
        for y in 0..frame.h {
            for x in 0..frame.w {
                let c = frame.get(x, y);
                if c != 0 {
                    sheet.put(ox + x, oy + y, c);
                }
            }
        }
        let _ = BLACK;
    }
    let file = std::fs::File::create(path).expect("image");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), sheet.w as u32, sheet.h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("png");
    let mut data = Vec::with_capacity(sheet.px.len() * 4);
    for p in &sheet.px {
        data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
    }
    w.write_image_data(&data).expect("png data");
    println!("{path}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--preview") {
        preview(args.get(1).map_or("kdx-frames-preview.png", String::as_str));
        return;
    }
    #[cfg(windows)]
    {
        let keep_console = args.iter().any(|a| a == "--console");
        let programs: Vec<String> = config::parse(&args.iter().filter(|a| *a != "--console").cloned().collect::<Vec<_>>().join("
"));
        overlay::run(programs, keep_console);
    }
    #[cfg(not(windows))]
    eprintln!("kdx-frames only draws on Windows (use --preview to see the frame)");
}
