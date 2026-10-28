//! Small text helpers for what comes from the network.

/// Removes mIRC formatting (bold, colours, reverse...) so a notification shows plain text.
pub fn strip_formatting(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\u{2}' | '\u{1d}' | '\u{1f}' | '\u{1e}' | '\u{11}' | '\u{16}' | '\u{f}' => {}
            '\u{3}' => {
                // \x03 fg[,bg], each one or two digits
                for _ in 0..2 {
                    if it.peek().map_or(false, |d| d.is_ascii_digit()) {
                        it.next();
                    }
                }
                if it.peek() == Some(&',') {
                    let mut look = it.clone();
                    look.next();
                    if look.peek().map_or(false, |d| d.is_ascii_digit()) {
                        it.next();
                        for _ in 0..2 {
                            if it.peek().map_or(false, |d| d.is_ascii_digit()) {
                                it.next();
                            }
                        }
                    }
                }
            }
            '\u{4}' => {
                // \x04 RRGGBB[,RRGGBB]
                for _ in 0..6 {
                    if it.peek().map_or(false, |d| d.is_ascii_hexdigit()) {
                        it.next();
                    }
                }
                if it.peek() == Some(&',') {
                    let mut look = it.clone();
                    look.next();
                    if look.peek().map_or(false, |d| d.is_ascii_hexdigit()) {
                        it.next();
                        for _ in 0..6 {
                            if it.peek().map_or(false, |d| d.is_ascii_hexdigit()) {
                                it.next();
                            }
                        }
                    }
                }
            }
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// At most `max` characters, with an ellipsis when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Percent-encodes a URL query value.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A window label made of the characters Tauri accepts, from any text.
pub fn label_part(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Only web links may be opened from a message.
pub fn is_web_url(u: &str) -> bool {
    let low = u.to_ascii_lowercase();
    (low.starts_with("http://") || low.starts_with("https://")) && !u.chars().any(|c| c.is_control() || c == ' ') && u.len() < 2048
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_irc_formatting() {
        assert_eq!(strip_formatting("\u{2}gras\u{2} et \u{1f}souligné\u{f}"), "gras et souligné");
        assert_eq!(strip_formatting("\u{3}04,12rouge sur bleu\u{3} fin"), "rouge sur bleu fin");
        assert_eq!(strip_formatting("\u{3}4texte"), "texte");
        assert_eq!(strip_formatting("12,5 vrai nombre"), "12,5 vrai nombre");
        assert_eq!(strip_formatting("\u{3},5virgule"), ",5virgule");
        assert_eq!(strip_formatting("\u{4}FF0000,00ff00x"), "x");
        assert_eq!(strip_formatting("a\u{7}b\u{0}c"), "abc", "bells and NULs vanish");
    }

    #[test]
    fn truncates_on_characters() {
        assert_eq!(truncate("éééé", 4), "éééé");
        assert_eq!(truncate("éééééé", 4), "ééé…");
    }

    #[test]
    fn encodes_for_urls_and_labels() {
        assert_eq!(url_encode("#rust fr"), "%23rust%20fr");
        assert_eq!(url_encode("é"), "%C3%A9");
        assert_eq!(label_part("#a"), "2361");
        assert!(label_part("é #/").chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn only_web_urls_pass() {
        assert!(is_web_url("https://example.org/a?b=c"));
        assert!(is_web_url("HTTP://example.org"));
        assert!(!is_web_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("ms-msdt:/id PCWDiagnostic"));
        assert!(!is_web_url("https://a b"));
        assert!(!is_web_url("https://a\nb"));
    }
}
