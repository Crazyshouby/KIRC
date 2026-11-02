//! Which buffer is which WhatsApp chat, and what the people are called.
//!
//! A chat is known to WhatsApp by an identifier (`33612345678@s.whatsapp.net`, `1234-5678@g.us`) and to the pages by a
//! name: `#Famille` for a group (so that the page shows its member list), the contact's name for a private chat. The
//! names are kept in a small file next to the session so that a chat found again after a restart is the same buffer
//! (its window, its log, its counters).

use crate::chat::fold;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Chats and names kept (a session's files must not grow without end).
const MAX_CHATS: usize = 2000;
const MAX_NAMES: usize = 20000;
const MAX_NAME_LEN: usize = 60;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Chat {
    buffer: String,
    jid: String,
    group: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    chats: Vec<Chat>,
    /// Display name by the user part of an identifier (the number, or the anonymous identifier).
    names: HashMap<String, String>,
    /// The phone number behind an anonymous identifier, both as user parts.
    #[serde(default)]
    phones: HashMap<String, String>,
}

#[derive(Default)]
pub struct Registry {
    path: Option<PathBuf>,
    by_buffer: HashMap<String, Chat>,
    by_jid: HashMap<String, String>,
    names: HashMap<String, String>,
    /// The same person under two identifiers (the phone number and the anonymous one), both ways.
    aliases: HashMap<String, String>,
    /// The phone number behind an anonymous identifier (user parts).
    phones: HashMap<String, String>,
    dirty: bool,
}

/// A name fit to be a buffer: no control characters, spaces squeezed, not starting like a channel, not too long.
pub fn clean_name(raw: &str) -> String {
    let squeezed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let no_control: String = squeezed.chars().filter(|c| !c.is_control() && *c != ',').collect();
    let start = no_control.trim_start_matches(['#', '&', '+', '!', ' ']);
    start.chars().take(MAX_NAME_LEN).collect::<String>().trim().to_string()
}

/// The name of a group's buffer: its subject, behind a '#' so that the page treats it as a channel.
pub fn group_buffer(subject: &str) -> String {
    let n = clean_name(subject);
    format!("#{}", if n.is_empty() { "groupe" } else { &n })
}

/// The name of a private chat's buffer: the contact's name, else the number.
pub fn user_buffer(display: &str, user: &str) -> String {
    let n = clean_name(display);
    if !n.is_empty() {
        return n;
    }
    let digits: String = user.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if digits.is_empty() { "inconnu".to_string() } else { digits }
}

/// The part of an identifier before the '@' (and before a device suffix).
pub fn user_of(jid: &str) -> &str {
    let user = jid.split('@').next().unwrap_or(jid);
    user.split([':', '.']).next().unwrap_or(user)
}

impl Registry {
    pub fn load(path: PathBuf) -> Registry {
        let saved: Saved = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut r = Registry { path: Some(path), names: saved.names, phones: saved.phones, ..Default::default() };
        for (lid, pn) in r.phones.clone() {
            r.aliases.insert(lid.clone(), pn.clone());
            r.aliases.insert(pn, lid);
        }
        for c in saved.chats.into_iter().take(MAX_CHATS) {
            r.by_jid.insert(c.jid.clone(), c.buffer.clone());
            r.by_buffer.insert(fold(&c.buffer), c);
        }
        r
    }

    pub fn save(&mut self) {
        let Some(path) = self.path.clone().filter(|_| self.dirty) else { return };
        let mut chats: Vec<Chat> = self.by_buffer.values().cloned().collect();
        chats.sort_by(|a, b| a.buffer.cmp(&b.buffer));
        let Ok(text) = serde_json::to_string(&Saved { chats, names: self.names.clone(), phones: self.phones.clone() }) else { return };
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)).is_ok() {
            self.dirty = false;
        }
    }

    pub fn set_name(&mut self, jid: &str, name: &str) {
        let name = clean_name(name);
        let user = user_of(jid).to_string();
        let other = self.aliases.get(&user).cloned();
        for u in std::iter::once(user).chain(other) {
            if u.is_empty() || name.is_empty() || self.names.get(&u).is_some_and(|n| *n == name) {
                continue;
            }
            if self.names.len() >= MAX_NAMES && !self.names.contains_key(&u) {
                continue;
            }
            self.names.insert(u, name.clone());
            self.dirty = true;
        }
    }

    /// Two identifiers of the same person: they share their name from now on. When one of them is an anonymous
    /// identifier (`@lid`), the other is its phone number.
    pub fn link(&mut self, a_jid: &str, b_jid: &str) {
        let (a, b) = (user_of(a_jid).to_string(), user_of(b_jid).to_string());
        match (a_jid.ends_with("@lid"), b_jid.ends_with("@lid")) {
            (true, false) if !a.is_empty() && !b.is_empty() && self.phones.len() < MAX_NAMES => {
                self.phones.insert(a.clone(), b.clone());
                self.dirty = true;
            }
            (false, true) if !a.is_empty() && !b.is_empty() && self.phones.len() < MAX_NAMES => {
                self.phones.insert(b.clone(), a.clone());
                self.dirty = true;
            }
            _ => {}
        }
        if a.is_empty() || b.is_empty() || a == b || self.aliases.len() >= MAX_NAMES {
            return;
        }
        let known = self.names.get(&a).or_else(|| self.names.get(&b)).cloned();
        self.aliases.insert(a.clone(), b.clone());
        self.aliases.insert(b.clone(), a.clone());
        if let Some(n) = known {
            self.set_name(&a, &n);
        }
    }

    pub fn name_of(&self, jid: &str) -> Option<&str> {
        let user = user_of(jid);
        self.names.get(user).or_else(|| self.aliases.get(user).and_then(|o| self.names.get(o))).map(String::as_str)
    }

    /// What to call a person: the name we know, else the phone number, else the identifier.
    pub fn display(&self, jid: &str) -> String {
        let user = user_of(jid);
        self.name_of(jid).map_or_else(|| user_buffer("", self.phones.get(user).map_or(user, String::as_str)), str::to_string)
    }

    /// The buffers of the private chats we know, to list them again at each connection.
    pub fn private_buffers(&self) -> Vec<String> {
        let mut v: Vec<String> = self.by_buffer.values().filter(|c| !c.group).map(|c| c.buffer.clone()).collect();
        v.sort_by_key(|b| fold(b));
        v
    }

    /// Private chats still called by a number, though a name may be known by now.
    pub fn unnamed_private(&self) -> Vec<String> {
        let numberish = |b: &str| !b.is_empty() && b.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '(' || c == ')');
        self.by_buffer.values().filter(|c| !c.group && numberish(&c.buffer)).map(|c| c.jid.clone()).collect()
    }

    /// Gives a private chat the buffer `wanted` (made unique); (old, new) when it changed.
    pub fn rename(&mut self, jid: &str, wanted: &str) -> Option<(String, String)> {
        let old = self.by_jid.get(jid)?.clone();
        if fold(&old) == fold(wanted) {
            return None;
        }
        let mut name = wanted.to_string();
        let mut n = 2;
        while self.by_buffer.contains_key(&fold(&name)) {
            name = format!("{wanted} ({n})");
            n += 1;
        }
        let mut chat = self.by_buffer.remove(&fold(&old))?;
        chat.buffer = name.clone();
        self.by_buffer.insert(fold(&name), chat);
        self.by_jid.insert(jid.to_string(), name.clone());
        self.dirty = true;
        Some((old, name))
    }

    /// The buffer of a chat: the one it had, else a new one named `wanted` (made unique among the buffers).
    pub fn buffer_for(&mut self, jid: &str, group: bool, wanted: &str) -> String {
        if let Some(b) = self.by_jid.get(jid) {
            return b.clone();
        }
        let mut name = wanted.to_string();
        let mut n = 2;
        while self.by_buffer.contains_key(&fold(&name)) {
            name = format!("{wanted} ({n})");
            n += 1;
        }
        if self.by_buffer.len() < MAX_CHATS {
            self.by_jid.insert(jid.to_string(), name.clone());
            self.by_buffer.insert(fold(&name), Chat { buffer: name.clone(), jid: jid.to_string(), group });
            self.dirty = true;
        }
        name
    }

    /// The buffer of a chat we already know.
    pub fn buffer_of(&self, jid: &str) -> Option<&str> {
        self.by_jid.get(jid).map(String::as_str)
    }

    /// The chat behind a buffer: its identifier, and whether it is a group.
    pub fn chat_of(&self, buffer: &str) -> Option<(&str, bool)> {
        self.by_buffer.get(&fold(buffer)).map(|c| (c.jid.as_str(), c.group))
    }

    /// The private chat of someone typed as a name or a number, if one exists.
    pub fn find_user(&self, wanted: &str) -> Option<&str> {
        let wanted = fold(wanted.trim());
        self.by_buffer.values().find(|c| !c.group && (fold(&c.buffer) == wanted || user_of(&c.jid) == wanted)).map(|c| c.jid.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_is_a_channel_and_a_contact_is_not() {
        assert_eq!(group_buffer("  Famille   proche "), "#Famille proche");
        assert_eq!(group_buffer("##Les #amis"), "#Les #amis");
        assert_eq!(group_buffer("   "), "#groupe");
        assert!(!user_buffer("+Alice", "336").starts_with(['#', '&', '+', '!']));
        assert_eq!(user_buffer("", "33612345678"), "33612345678");
        assert_eq!(user_buffer("Alice, la belle", "1"), "Alice la belle");
    }

    #[test]
    fn names_have_no_control_characters_and_a_limit() {
        assert_eq!(clean_name("a\u{0}b\nc"), "ab c");
        assert_eq!(clean_name(&"x".repeat(200)).chars().count(), MAX_NAME_LEN);
    }

    #[test]
    fn the_user_part_ignores_the_device_and_the_server() {
        assert_eq!(user_of("33612345678@s.whatsapp.net"), "33612345678");
        assert_eq!(user_of("33612345678:12@s.whatsapp.net"), "33612345678");
        assert_eq!(user_of("215530082410615.0:3@lid"), "215530082410615");
    }

    #[test]
    fn a_chat_keeps_its_buffer_and_two_chats_never_share_one() {
        let mut r = Registry::default();
        let a = r.buffer_for("1@g.us", true, "#Famille");
        assert_eq!(a, "#Famille");
        assert_eq!(r.buffer_for("1@g.us", true, "#Autre nom"), "#Famille", "known chat: the name it had");
        assert_eq!(r.buffer_for("2@g.us", true, "#famille"), "#famille (2)", "same name, other chat (case does not matter)");
        assert_eq!(r.chat_of("#FAMILLE"), Some(("1@g.us", true)));
        assert_eq!(r.chat_of("#famille (2)"), Some(("2@g.us", true)));
    }

    #[test]
    fn people_are_called_by_their_name_then_their_number() {
        let mut r = Registry::default();
        assert_eq!(r.display("336@s.whatsapp.net"), "336");
        r.set_name("336:7@s.whatsapp.net", "Alice");
        assert_eq!(r.display("336@s.whatsapp.net"), "Alice");
        assert_eq!(r.name_of("336@lid"), Some("Alice"), "the same user part, whatever the server");
        r.buffer_for("336@s.whatsapp.net", false, "Alice");
        assert_eq!(r.find_user("alice"), Some("336@s.whatsapp.net"));
        assert_eq!(r.find_user("336"), Some("336@s.whatsapp.net"));
        assert_eq!(r.find_user("nobody"), None);
    }

    #[test]
    fn the_two_identifiers_of_a_person_share_their_name() {
        let mut r = Registry::default();
        r.set_name("336@s.whatsapp.net", "Alice");
        r.link("336@s.whatsapp.net", "9999@lid");
        assert_eq!(r.display("9999@lid"), "Alice", "linked after the name was known");
        r.set_name("4444@lid", "Bob");
        r.link("4444@lid", "337@s.whatsapp.net");
        assert_eq!(r.display("337@s.whatsapp.net"), "Bob");
        r.set_name("337@s.whatsapp.net", "Robert");
        assert_eq!(r.display("4444@lid"), "Robert", "a new name reaches both");
        r.link("1@lid", "1@s.whatsapp.net");
        assert_eq!(r.display("1@lid"), "1", "a person linked to itself changes nothing");
    }

    #[test]
    fn an_anonymous_identifier_shows_its_phone_number_until_a_name_is_known() {
        let mut r = Registry::default();
        r.link("215530082410615@lid", "33612345678@s.whatsapp.net");
        assert_eq!(r.display("215530082410615@lid"), "33612345678");
        r.set_name("33612345678@s.whatsapp.net", "Alice");
        assert_eq!(r.display("215530082410615@lid"), "Alice");
    }

    #[test]
    fn a_chat_called_by_a_number_gets_its_name_when_it_is_known() {
        let mut r = Registry::default();
        let b = r.buffer_for("215@lid", false, "215");
        r.buffer_for("2@s.whatsapp.net", false, "Bob");
        assert_eq!(r.unnamed_private(), vec!["215@lid".to_string()]);
        assert_eq!(r.rename("215@lid", "Alice"), Some((b, "Alice".to_string())));
        assert_eq!(r.chat_of("alice"), Some(("215@lid", false)));
        assert_eq!(r.chat_of("215"), None);
        assert_eq!(r.rename("215@lid", "ALICE"), None, "same name: nothing to do");
        assert!(r.unnamed_private().is_empty());
        assert_eq!(r.rename("215@lid", "Bob").map(|(_, n)| n).as_deref(), Some("Bob (2)"), "never the buffer of another chat");
    }

    #[test]
    fn the_private_chats_are_listed_again_after_a_restart() {
        let mut r = Registry::default();
        r.buffer_for("1@g.us", true, "#Famille");
        r.buffer_for("2@s.whatsapp.net", false, "Bob");
        r.buffer_for("3@lid", false, "alice");
        assert_eq!(r.private_buffers(), vec!["alice".to_string(), "Bob".to_string()]);
    }

    #[test]
    fn the_registry_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("messages-wa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chats.json");
        let mut r = Registry::load(path.clone());
        r.set_name("336@s.whatsapp.net", "Alice");
        r.buffer_for("336@s.whatsapp.net", false, "Alice");
        r.buffer_for("1@g.us", true, "#Famille");
        r.save();
        let again = Registry::load(path);
        assert_eq!(again.display("336@s.whatsapp.net"), "Alice");
        assert_eq!(again.chat_of("alice"), Some(("336@s.whatsapp.net", false)));
        assert_eq!(again.chat_of("#famille"), Some(("1@g.us", true)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
