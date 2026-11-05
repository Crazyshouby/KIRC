//! Tabs: windows merged into one place.
//!
//! Every tab stays a window of its own, with its page, its history and its counters; a group of tabs simply shows
//! one of them at a time, the others being hidden, and the tab that is shown takes the place and the size of the one
//! it replaces. Each window of a group draws the same strip of tabs under its title bar (see `initTabs` in messages.js),
//! fed by the "tabs" events published here. Dragging a window by its title bar onto the title bar of another one
//! merges them (the target lights up while the cursor is over it).
//!
//! Windows are only touched with the lock released: asking a window something waits for the main thread, which may
//! itself be waiting for this lock (a window being destroyed).

use crate::app::App;
use serde::Serialize;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Height of the drop zone at the top of a window (title bar and strip of tabs), in logical pixels.
const DROP_ZONE: f64 = 46.0;

#[derive(Default)]
pub struct Tabs {
    groups: Vec<Group>,
    unread: HashMap<String, u32>,
    /// The window that a window being dragged would join, while the cursor is over its top.
    target: Option<String>,
}

struct Group {
    tabs: Vec<String>,
    active: String,
}

#[derive(Serialize, Clone)]
struct Tab {
    label: String,
    title: String,
    active: bool,
    unread: u32,
}

#[derive(Serialize, Clone)]
struct State {
    me: String,
    tabs: Vec<Tab>,
}

impl Tabs {
    fn group_of(&self, label: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.tabs.iter().any(|t| t == label))
    }
}

/// Windows that can become tabs: the main window and the dialogs stay on their own.
pub fn mergeable(label: &str) -> bool {
    label == "servers" || label == "settings" || ["console-", "list-", "chat-"].iter().any(|p| label.starts_with(p))
}

/// A tab that is not the one shown: its window is hidden.
pub fn is_hidden_tab(app: &AppHandle, label: &str) -> bool {
    let st = app.state::<App>();
    let t = st.tabs.lock().unwrap();
    t.group_of(label).is_some_and(|i| t.groups[i].active != label)
}

pub fn is_grouped(app: &AppHandle, label: &str) -> bool {
    app.state::<App>().tabs.lock().unwrap().group_of(label).is_some()
}

fn get(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

fn rect(w: &WebviewWindow) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    Some((w.outer_position().ok()?, w.inner_size().ok()?))
}

fn place(w: &WebviewWindow, r: (PhysicalPosition<i32>, PhysicalSize<u32>), with_size: bool) {
    if with_size {
        let _ = w.set_size(r.1);
    }
    let _ = w.set_position(r.0);
}

fn reveal(w: &WebviewWindow) {
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// The short name of a tab: a channel is shown by its name alone.
fn short_title(label: &str, title: &str) -> String {
    if label.starts_with("chat-") {
        title.split(" \u{2014} ").next().unwrap_or(title).to_string()
    } else {
        title.to_string()
    }
}

/// Tells every window of a group (or the one that was left alone) what the strip of tabs shows.
fn publish(app: &AppHandle, tabs: &[String], active: &str) {
    let unread = app.state::<App>().tabs.lock().unwrap().unread.clone();
    let list: Vec<Tab> = tabs
        .iter()
        .map(|l| Tab {
            label: l.clone(),
            title: get(app, l).and_then(|w| w.title().ok()).map_or_else(|| l.clone(), |t| short_title(l, &t)),
            active: l == active,
            unread: unread.get(l).copied().unwrap_or(0),
        })
        .collect();
    for l in tabs {
        let _ = app.emit_to(l.as_str(), "tabs", State { me: l.clone(), tabs: if tabs.len() > 1 { list.clone() } else { Vec::new() } });
    }
}

/// What a window that just opened (or reloaded) should show.
pub fn publish_for(app: &AppHandle, label: &str) {
    let snap = {
        let st = app.state::<App>();
        let t = st.tabs.lock().unwrap();
        t.group_of(label).map(|i| (t.groups[i].tabs.clone(), t.groups[i].active.clone()))
    };
    match snap {
        Some((tabs, active)) => publish(app, &tabs, &active),
        None => {
            let _ = app.emit_to(label, "tabs", State { me: label.to_string(), tabs: Vec::new() });
        }
    }
}

/// Shows `label` in place of the tab of its group that is shown now.
pub fn activate(app: &AppHandle, label: &str) {
    let found = {
        let st = app.state::<App>();
        let mut t = st.tabs.lock().unwrap();
        t.group_of(label).map(|i| {
            let old = std::mem::replace(&mut t.groups[i].active, label.to_string());
            (old, t.groups[i].tabs.clone())
        })
    };
    let Some((old, tabs)) = found else { return };
    if old != label {
        if let (Some(ow), Some(nw)) = (get(app, &old), get(app, label)) {
            if let Some(r) = rect(&ow) {
                place(&nw, r, !app.state::<App>().is_shaded(&old));
            }
            reveal(&nw);
            let _ = ow.hide();
        }
    }
    publish(app, &tabs, label);
}

/// Merges the group of `dragged` into the group of `target`; `dragged` is the tab shown afterwards.
pub fn merge(app: &AppHandle, dragged: &str, target: &str) {
    if dragged == target || !mergeable(dragged) || !mergeable(target) {
        return;
    }
    let merged = {
        let st = app.state::<App>();
        let mut t = st.tabs.lock().unwrap();
        if t.group_of(dragged).is_some() && t.group_of(dragged) == t.group_of(target) {
            return;
        }
        let moved: Vec<String> = match t.group_of(dragged) {
            Some(i) => t.groups.remove(i).tabs,
            None => vec![dragged.to_string()],
        };
        let i = match t.group_of(target) {
            Some(i) => i,
            None => {
                t.groups.push(Group { tabs: vec![target.to_string()], active: target.to_string() });
                t.groups.len() - 1
            }
        };
        let old = std::mem::replace(&mut t.groups[i].active, dragged.to_string());
        t.groups[i].tabs.extend(moved);
        (old, t.groups[i].tabs.clone())
    };
    let (old, tabs) = merged;
    if let (Some(ow), Some(dw)) = (get(app, &old), get(app, dragged)) {
        if let Some(r) = rect(&ow) {
            place(&dw, r, true);
        }
        reveal(&dw);
    }
    for l in tabs.iter().filter(|l| l.as_str() != dragged) {
        if let Some(w) = get(app, l) {
            let _ = w.hide();
        }
    }
    publish(app, &tabs, dragged);
}

/// The tab to show when `label` goes away: its right neighbour, else its left one.
fn neighbour(tabs: &[String], label: &str) -> Option<String> {
    let i = tabs.iter().position(|t| t == label)?;
    tabs.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| tabs.get(j))).cloned()
}

/// Takes a tab out of its group; its window appears a little apart from the group.
pub fn detach(app: &AppHandle, label: &str) {
    let left = {
        let st = app.state::<App>();
        let mut t = st.tabs.lock().unwrap();
        let Some(i) = t.group_of(label) else { return };
        let was_active = t.groups[i].active == label;
        let next = neighbour(&t.groups[i].tabs, label);
        t.groups[i].tabs.retain(|l| l != label);
        let rest = t.groups[i].tabs.clone();
        if was_active {
            if let Some(n) = next {
                t.groups[i].active = n;
            }
        }
        let active = t.groups[i].active.clone();
        if rest.len() < 2 {
            t.groups.remove(i);
        }
        (rest, active, was_active)
    };
    let (rest, active, was_active) = left;
    let Some(w) = get(app, label) else { return };
    let r = rect(&w);
    if was_active {
        if let (Some(nw), Some(r)) = (get(app, &active), r) {
            place(&nw, r, true);
            reveal(&nw);
        }
    }
    if let Some(r) = r {
        let scale = w.scale_factor().unwrap_or(1.0);
        let off = (30.0 * scale).round() as i32;
        place(&w, (PhysicalPosition::new(r.0.x + off, r.0.y + off), r.1), false);
    }
    reveal(&w);
    publish_for(app, label);
    for l in &rest {
        publish_for(app, l);
    }
}

/// Before a tab's window closes: another tab of the group takes its place.
pub fn before_close(app: &AppHandle, label: &str) {
    let next = {
        let st = app.state::<App>();
        let t = st.tabs.lock().unwrap();
        t.group_of(label).filter(|&i| t.groups[i].active == label).and_then(|i| neighbour(&t.groups[i].tabs, label))
    };
    if let Some(n) = next {
        activate(app, &n);
    }
}

/// A window is gone: out of its group.
pub fn forget(app: &AppHandle, label: &str) {
    let left = {
        let st = app.state::<App>();
        let mut t = st.tabs.lock().unwrap();
        t.unread.remove(label);
        if t.target.as_deref() == Some(label) {
            t.target = None;
        }
        let Some(i) = t.group_of(label) else { return };
        let was_active = t.groups[i].active == label;
        let next = neighbour(&t.groups[i].tabs, label);
        t.groups[i].tabs.retain(|l| l != label);
        let rest = t.groups[i].tabs.clone();
        if was_active {
            if let Some(n) = next {
                t.groups[i].active = n;
            }
        }
        let active = t.groups[i].active.clone();
        if rest.len() < 2 {
            t.groups.remove(i);
        }
        (rest, active, was_active)
    };
    let (rest, active, was_active) = left;
    if was_active {
        if let Some(w) = get(app, &active) {
            reveal(&w);
        }
    }
    for l in &rest {
        publish_for(app, l);
    }
}

/// The number of unread lines of a tab, for its label.
pub fn set_unread(app: &AppHandle, label: &str, n: u32) {
    let snap = {
        let st = app.state::<App>();
        let mut t = st.tabs.lock().unwrap();
        t.unread.insert(label.to_string(), n);
        t.group_of(label).map(|i| (t.groups[i].tabs.clone(), t.groups[i].active.clone()))
    };
    if let Some((tabs, active)) = snap {
        publish(app, &tabs, &active);
    }
}

/// While `label` is dragged: finds the window whose top the cursor is over, and lights it up.
pub fn hover(app: &AppHandle, label: &str, win: &WebviewWindow) {
    if !mergeable(label) {
        return;
    }
    let Ok(c) = win.cursor_position() else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let zone = (DROP_ZONE * scale).round();
    let found = app.webview_windows().into_iter().find_map(|(l, w)| {
        if l == label || !mergeable(&l) || !w.is_visible().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
            return None;
        }
        let (p, s) = (w.outer_position().ok()?, w.outer_size().ok()?);
        let inside = c.x >= p.x as f64 && c.x < (p.x + s.width as i32) as f64 && c.y >= p.y as f64 && c.y < p.y as f64 + zone.min(s.height as f64);
        inside.then_some(l)
    });
    let (old, new) = {
        let st = app.state::<App>();
        let mut t = st.tabs.lock().unwrap();
        // a window of the group it is dragged from is not a target
        let found = found.filter(|f| t.group_of(label).is_none_or(|i| !t.groups[i].tabs.contains(f)));
        if t.target == found {
            return;
        }
        (std::mem::replace(&mut t.target, found.clone()), found)
    };
    if let Some(o) = old {
        let _ = app.emit_to(o.as_str(), "droptarget", false);
    }
    if let Some(n) = new {
        let _ = app.emit_to(n.as_str(), "droptarget", true);
    }
}

/// The title bar of `label` is let go: merge it into the window that was lit up, if any.
pub fn drop_on_target(app: &AppHandle, label: &str) {
    let target = app.state::<App>().tabs.lock().unwrap().target.take();
    if let Some(t) = target {
        let _ = app.emit_to(t.as_str(), "droptarget", false);
        merge(app, label, &t);
    }
}

#[cfg(test)]
mod tests {
    use super::{mergeable, neighbour, short_title};

    #[test]
    fn only_working_windows_merge() {
        for l in ["servers", "settings", "console-ab", "list-ab", "chat-ab-cd"] {
            assert!(mergeable(l), "{l}");
        }
        for l in ["launcher", "about", "join", "switcher", "whois-ab-cd"] {
            assert!(!mergeable(l), "{l}");
        }
    }

    #[test]
    fn the_neighbour_is_the_right_one_then_the_left_one() {
        let t: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(neighbour(&t, "a").as_deref(), Some("b"));
        assert_eq!(neighbour(&t, "b").as_deref(), Some("c"));
        assert_eq!(neighbour(&t, "c").as_deref(), Some("b"));
        assert_eq!(neighbour(&t[..1], "a"), None);
    }

    #[test]
    fn a_channel_tab_shows_the_channel_alone() {
        assert_eq!(short_title("chat-1-2", "#test \u{2014} Serveur de test"), "#test");
        assert_eq!(short_title("console-1", "Messages \u{2014} Serveur de test"), "Messages \u{2014} Serveur de test");
    }
}
