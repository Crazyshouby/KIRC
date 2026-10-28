//! IRC wire format: one line = optional IRCv3 tags, optional prefix, a command
//! and up to fifteen parameters, the last of which may contain spaces.

use std::fmt;

/// Longest line a server may send us (tags included): beyond this the line is dropped.
pub const MAX_LINE: usize = 8192 + 512;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Message {
    pub tags: Vec<(String, String)>,
    pub prefix: Option<String>,
    pub command: String,
    pub params: Vec<String>,
}

/// `nick!user@host`, or a bare server name (then `user` and `host` are empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source<'a> {
    pub nick: &'a str,
    pub user: &'a str,
    pub host: &'a str,
}

impl Message {
    pub fn new(command: &str, params: &[&str]) -> Self {
        Message {
            tags: Vec::new(),
            prefix: None,
            command: command.to_ascii_uppercase(),
            params: params.iter().map(|p| p.to_string()).collect(),
        }
    }

    /// Parses one line without its line terminator. `None` for an empty or malformed line.
    pub fn parse(line: &str) -> Option<Message> {
        let mut rest = line.trim_matches(|c| c == '\r' || c == '\n');
        let mut msg = Message::default();

        if let Some(r) = rest.strip_prefix('@') {
            let (tags, after) = r.split_once(' ')?;
            for tag in tags.split(';').filter(|t| !t.is_empty()) {
                let (k, v) = tag.split_once('=').unwrap_or((tag, ""));
                msg.tags.push((k.to_string(), unescape_tag(v)));
            }
            rest = after.trim_start_matches(' ');
        }
        if let Some(r) = rest.strip_prefix(':') {
            let (prefix, after) = r.split_once(' ')?;
            msg.prefix = Some(prefix.to_string());
            rest = after.trim_start_matches(' ');
        }
        if rest.is_empty() {
            return None;
        }
        let (cmd, mut args) = match rest.split_once(' ') {
            Some((c, a)) => (c, a),
            None => (rest, ""),
        };
        msg.command = cmd.to_ascii_uppercase();
        loop {
            args = args.trim_start_matches(' ');
            if args.is_empty() {
                break;
            }
            if let Some(trailing) = args.strip_prefix(':') {
                msg.params.push(trailing.to_string());
                break;
            }
            match args.split_once(' ') {
                Some((p, a)) => {
                    msg.params.push(p.to_string());
                    args = a;
                }
                None => {
                    msg.params.push(args.to_string());
                    break;
                }
            }
        }
        Some(msg)
    }

    pub fn param(&self, i: usize) -> &str {
        self.params.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn last(&self) -> &str {
        self.params.last().map(String::as_str).unwrap_or("")
    }

    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn source(&self) -> Option<Source<'_>> {
        let p = self.prefix.as_deref()?;
        let (nick, rest) = match p.split_once('!') {
            Some((n, r)) => (n, r),
            None => match p.split_once('@') {
                Some((n, h)) => return Some(Source { nick: n, user: "", host: h }),
                None => return Some(Source { nick: p, user: "", host: "" }),
            },
        };
        let (user, host) = rest.split_once('@').unwrap_or((rest, ""));
        Some(Source { nick, user, host })
    }
}

/// Serialises a message to send. Control characters that would end the line
/// (CR, LF, NUL) are removed from every part: text typed or pasted by the user
/// can never smuggle a second command.
impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&clean(&self.command))?;
        let n = self.params.len();
        for (i, p) in self.params.iter().enumerate() {
            let p = clean(p);
            let last = i + 1 == n;
            if last && (p.is_empty() || p.contains(' ') || p.starts_with(':')) {
                write!(f, " :{p}")?;
            } else {
                // A middle parameter can neither hold a space nor start with ':'.
                write!(f, " {}", p.replace(' ', "_").trim_start_matches(':'))?;
            }
        }
        Ok(())
    }
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\r' | '\n' | '\0')).collect()
}

fn unescape_tag(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

/// Cuts `text` into pieces of at most `max` bytes, on character boundaries
/// (and preferably after a space), so a long message is sent as several lines.
pub fn split_text(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(sp) = rest[..cut].rfind(' ') {
            if sp > max / 2 {
                cut = sp + 1;
            }
        }
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    if !rest.is_empty() || out.is_empty() {
        out.push(rest.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefix_command_and_trailing() {
        let m = Message::parse(":nick!user@host PRIVMSG #chan :hello world").unwrap();
        assert_eq!(m.command, "PRIVMSG");
        assert_eq!(m.params, vec!["#chan", "hello world"]);
        let s = m.source().unwrap();
        assert_eq!((s.nick, s.user, s.host), ("nick", "user", "host"));
    }

    #[test]
    fn parses_tags_and_unescapes_them() {
        let m = Message::parse("@time=2026-10-28T12:00:00.000Z;x=a\\sb\\:c :n!u@h NOTICE me :hi").unwrap();
        assert_eq!(m.tag("time"), Some("2026-10-28T12:00:00.000Z"));
        assert_eq!(m.tag("x"), Some("a b;c"));
        assert_eq!(m.command, "NOTICE");
    }

    #[test]
    fn command_is_upper_cased_and_numerics_work() {
        let m = Message::parse(":irc.example.org 001 me :Welcome").unwrap();
        assert_eq!(m.command, "001");
        let m = Message::parse("ping :abc").unwrap();
        assert_eq!(m.command, "PING");
        assert_eq!(m.last(), "abc");
        assert!(m.prefix.is_none());
    }

    #[test]
    fn server_prefix_has_no_user() {
        let m = Message::parse(":irc.example.org NOTICE * :hello").unwrap();
        let s = m.source().unwrap();
        assert_eq!((s.nick, s.user, s.host), ("irc.example.org", "", ""));
    }

    #[test]
    fn rejects_empty_and_prefix_only_lines() {
        assert!(Message::parse("").is_none());
        assert!(Message::parse("\r\n").is_none());
        assert!(Message::parse(":only.a.prefix").is_none());
        assert!(Message::parse("@tag-only").is_none());
    }

    #[test]
    fn tolerates_repeated_spaces_and_empty_trailing() {
        let m = Message::parse("MODE   #c   +o   bob").unwrap();
        assert_eq!(m.params, vec!["#c", "+o", "bob"]);
        let m = Message::parse("TOPIC #c :").unwrap();
        assert_eq!(m.params, vec!["#c", ""]);
    }

    #[test]
    fn serialises_with_trailing_only_when_needed() {
        assert_eq!(Message::new("JOIN", &["#rust"]).to_string(), "JOIN #rust");
        assert_eq!(Message::new("PRIVMSG", &["#c", "hello world"]).to_string(), "PRIVMSG #c :hello world");
        assert_eq!(Message::new("PRIVMSG", &["#c", "hello"]).to_string(), "PRIVMSG #c hello");
        assert_eq!(Message::new("PRIVMSG", &["#c", ":)"]).to_string(), "PRIVMSG #c ::)");
        assert_eq!(Message::new("TOPIC", &["#c", ""]).to_string(), "TOPIC #c :");
    }

    #[test]
    fn injection_is_impossible() {
        let line = Message::new("PRIVMSG", &["#c", "hi\r\nQUIT :pwned"]).to_string();
        assert!(!line.contains('\n') && !line.contains('\r'));
        assert_eq!(line, "PRIVMSG #c :hiQUIT :pwned");
        // A middle parameter cannot carry a space either.
        let line = Message::new("JOIN", &["#a b", "x"]).to_string();
        assert_eq!(line, "JOIN #a_b x");
    }

    #[test]
    fn splits_long_text_on_char_boundaries() {
        let text = "é".repeat(500); // 1000 bytes
        let parts = split_text(&text, 400);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.len() <= 400));
        assert_eq!(parts.concat(), text);
        assert_eq!(split_text("court", 400), vec!["court"]);
        assert_eq!(split_text("", 400), vec![""]);
    }

    #[test]
    fn split_prefers_spaces() {
        let text = format!("{} {}", "a".repeat(300), "b".repeat(300));
        let parts = split_text(&text, 400);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with(' '));
    }
}
