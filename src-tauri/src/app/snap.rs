//! L'aimantation des fenêtres déplacées ou redimensionnées (la même règle que Snap.cs de cdx-windows). Sorti de app.rs (audit du 3/11/2026, 4.6).

use super::*;

/// A window being dragged clings to the edges of the others (and of its screen) when it comes this close, in
/// logical pixels; two windows that cling are left `SNAP_GAP` logical pixels apart, a window and the screen edge none.
/// `SNAP_GAP` is 0 since cdx-ui 2.0.2 (`snap_gap` in tokens.json): windows cling edge to edge; the parameter stays so the rule is testable.
const SNAP_DIST: f64 = crate::cdx_tokens::SNAP_DISTANCE as f64;   // tokens.json « snap_distance », par la copie de la charte
pub(super) const SNAP_GAP: f64 = crate::cdx_tokens::SNAP_GAP as f64;

/// Where a window of size (w, h) dragged to (x, y) lands once it clings to the other windows (x, y, w, h) and to the
/// edges of the screen's work area, within `dist` pixels; windows are left `gap` pixels apart, the screen edge none.
#[allow(clippy::too_many_arguments)] // une géométrie : position, taille, voisines, zone, réglages de la charte
pub fn snap_rect(x: i32, y: i32, w: i32, h: i32, others: &[(i32, i32, i32, i32)], area: Option<(i32, i32, i32, i32)>, dist: i32, gap: i32) -> (i32, i32) {
    // (distance, new coordinate) of the closest edge found on each axis
    let mut bx: Option<(i32, i32)> = None;
    let mut by: Option<(i32, i32)> = None;
    let near = |best: &mut Option<(i32, i32)>, at: i32, cand: i32| {
        let d = (cand - at).abs();
        if d <= dist && best.is_none_or(|(bd, _)| d < bd) {
            *best = Some((d, cand));
        }
    };
    for &(ox, oy, ow, oh) in others {
        // only windows that face each other along the other axis are worth clinging to
        if y < oy + oh + dist && y + h > oy - dist {
            near(&mut bx, x, ox + ow + gap); // our left edge against their right edge
            near(&mut bx, x, ox - w - gap); // our right edge against their left edge
            near(&mut bx, x, ox); // left edges in line
            near(&mut bx, x, ox + ow - w); // right edges in line
        }
        if x < ox + ow + dist && x + w > ox - dist {
            near(&mut by, y, oy + oh + gap);
            near(&mut by, y, oy - h - gap);
            near(&mut by, y, oy);
            near(&mut by, y, oy + oh - h);
        }
    }
    if let Some((ax, ay, aw, ah)) = area {
        near(&mut bx, x, ax);
        near(&mut bx, x, ax + aw - w);
        near(&mut by, y, ay);
        near(&mut by, y, ay + ah - h);
    }
    (bx.map_or(x, |b| b.1), by.map_or(y, |b| b.1))
}

/// Rectangles (x, y, w, h) of the other visible windows of the application, and of the work area of the screen.
/// Un rectangle (x, y, largeur, hauteur) en pixels physiques.
type Rect = (i32, i32, i32, i32);

fn neighbours(app: &AppHandle, label: &str, win: &tauri::WebviewWindow) -> (Vec<Rect>, Option<Rect>) {
    let others = app
        .webview_windows()
        .into_iter()
        .filter(|(l, w)| l != label && l != "switcher" && w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
        .filter_map(|(_, w)| Some((w.outer_position().ok()?, w.outer_size().ok()?)))
        .map(|(p, s)| (p.x, p.y, s.width as i32, s.height as i32))
        .collect();
    let area = win.current_monitor().ok().flatten().map(|m| {
        let a = m.work_area();
        (a.position.x, a.position.y, a.size.width as i32, a.size.height as i32)
    });
    (others, area)
}

/// `snap_rect` for a window being dragged, against the other visible windows of the application.
fn snap(app: &AppHandle, label: &str, win: &tauri::WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    let Ok(size) = win.outer_size() else { return (x, y) };
    let scale = win.scale_factor().unwrap_or(1.0);
    let (others, area) = neighbours(app, label, win);
    snap_rect(x, y, size.width as i32, size.height as i32, &others, area, (SNAP_DIST * scale).round() as i32, (SNAP_GAP * scale).round() as i32)
}

/// The size a window at (x, y) takes when its right and bottom edges are dragged to (w, h) and cling to the edges
/// of the other windows (leaving `gap`, or lining up with them) and of the screen, within `dist` pixels.
#[allow(clippy::too_many_arguments)]
pub fn snap_size(x: i32, y: i32, w: i32, h: i32, others: &[(i32, i32, i32, i32)], area: Option<(i32, i32, i32, i32)>, dist: i32, gap: i32) -> (i32, i32) {
    let mut br: Option<(i32, i32)> = None; // closest right edge: (distance, coordinate)
    let mut bb: Option<(i32, i32)> = None; // closest bottom edge
    let near = |best: &mut Option<(i32, i32)>, at: i32, cand: i32| {
        let d = (cand - at).abs();
        if d <= dist && best.is_none_or(|(bd, _)| d < bd) {
            *best = Some((d, cand));
        }
    };
    let (right, bottom) = (x + w, y + h);
    for &(ox, oy, ow, oh) in others {
        if y < oy + oh + dist && bottom > oy - dist {
            near(&mut br, right, ox - gap); // against the left side of a window on our right
            near(&mut br, right, ox + ow); // right edges in line
            near(&mut br, right, ox);
        }
        if x < ox + ow + dist && right > ox - dist {
            near(&mut bb, bottom, oy - gap);
            near(&mut bb, bottom, oy + oh);
            near(&mut bb, bottom, oy);
        }
    }
    if let Some((ax, ay, aw, ah)) = area {
        near(&mut br, right, ax + aw);
        near(&mut bb, bottom, ay + ah);
    }
    (br.map_or(w, |b| b.1 - x), bb.map_or(h, |b| b.1 - y))
}

pub fn drag(app: &AppHandle, label: &str, phase: &str) {
    let st = app.state::<App>();
    let Some(win) = app.get_webview_window(label) else { return };
    match phase {
        "start" => {
            if let (Ok(c), Ok(p)) = (win.cursor_position(), win.outer_position()) {
                st.drags.lock().unwrap().insert(label.to_string(), ((c.x, c.y), (p.x, p.y)));
            }
        }
        "move" => {
            let start = st.drags.lock().unwrap().get(label).copied();
            if let (Some(((cx, cy), (wx, wy))), Ok(c)) = (start, win.cursor_position()) {
                let (x, y) = snap(app, label, &win, wx + (c.x - cx).round() as i32, wy + (c.y - cy).round() as i32);
                let _ = win.set_position(PhysicalPosition::new(x, y));
                crate::tabs::hover(app, label, &win);
            }
        }
        "rstart" => {
            if let (Ok(c), Ok(sz)) = (win.cursor_position(), win.inner_size()) {
                st.resizes.lock().unwrap().insert(label.to_string(), ((c.x, c.y), (sz.width, sz.height)));
            }
        }
        "rmove" => {
            let start = st.resizes.lock().unwrap().get(label).copied();
            let min = st.mins.lock().unwrap().get(label).copied().unwrap_or((200.0, 100.0));
            let scale = win.scale_factor().unwrap_or(1.0);
            if let (Some(((cx, cy), (w, h))), Ok(c)) = (start, win.cursor_position()) {
                let (mut nw, mut nh) = ((w as f64 + (c.x - cx)).round() as i32, (h as f64 + (c.y - cy)).round() as i32);
                if let Ok(p) = win.outer_position() {
                    let (others, area) = neighbours(app, label, &win);
                    (nw, nh) = snap_size(p.x, p.y, nw, nh, &others, area, (SNAP_DIST * scale).round() as i32, (SNAP_GAP * scale).round() as i32);
                }
                if label == "launcher" {
                    nw = w as i32; // the width of the main window is fixed
                }
                let nw = (nw as f64).max(min.0 * scale).round() as u32;
                let nh = (nh as f64).max(min.1 * scale).round() as u32;
                let _ = win.set_size(PhysicalSize::new(nw, nh));
            }
        }
        _ => {
            st.drags.lock().unwrap().remove(label);
            st.resizes.lock().unwrap().remove(label);
            crate::tabs::drop_on_target(app, label);
        }
    }
}
