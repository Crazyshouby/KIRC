//! The list of the programs whose windows get the frame: a text file, one program per line.
//!
//! %APPDATA%\CDX-Frames\programmes.txt, created on the first run. A line is a program name (`charmap.exe`; `.exe` may be left
//! out); what follows a `#` is a comment. The file is read again whenever it changes, so the list can be edited while the
//! program runs.

use std::path::PathBuf;
use std::time::SystemTime;

pub const DEFAULT: &str = "# Programmes dont les fenêtres reçoivent le cadre CDX, un par ligne.\n\
# Les applications qui dessinent elles-mêmes leur barre de titre (Explorateur, navigateurs, Terminal Windows,\n\
# applications du Store) perdraient leurs onglets : ne les mettez pas ici.\n\
charmap.exe\n\
# mmc.exe\n\
# devmgmt.msc ne se décore pas lui-même : c'est mmc.exe qui l'ouvre\n";

/// The program names of a file's text: lower case, `.exe` added when missing, comments and blank lines dropped.
pub fn parse(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // some editors begin a UTF-8 file with a byte order mark
    let text = text.trim_start_matches('\u{feff}');
    for line in text.lines() {
        let name = line.split('#').next().unwrap_or("").trim().trim_matches('"').to_lowercase();
        if name.is_empty() || name.contains(['\\', '/', ':', '*', '?', '<', '>', '|']) {
            continue;
        }
        let name = if name.contains('.') { name } else { format!("{name}.exe") };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("CDX-Frames").join("programmes.txt"))
}

/// The list in the file (created with the default content if there is none) and when it was last changed.
pub fn load() -> (Vec<String>, Option<SystemTime>) {
    let Some(p) = path() else { return (parse(DEFAULT), None) };
    if !p.exists() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, DEFAULT);
    }
    let text = std::fs::read_to_string(&p).unwrap_or_else(|_| DEFAULT.to_string());
    (parse(&text), std::fs::metadata(&p).and_then(|m| m.modified()).ok())
}

pub fn modified() -> Option<SystemTime> {
    path().and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn a_list_has_one_program_per_line_without_comments() {
        let text = "# titre\n  CharMap.exe  # la table\n\nmmc\n\"regedit.exe\"\ncharmap.exe\n";
        assert_eq!(parse(text), ["charmap.exe", "mmc.exe", "regedit.exe"]);
    }

    #[test]
    fn a_byte_order_mark_does_not_spoil_the_first_name() {
        assert_eq!(parse("\u{feff}charmap\r\nmmc.exe\r\n"), ["charmap.exe", "mmc.exe"]);
    }

    #[test]
    fn a_path_or_a_pattern_is_not_a_program_name() {
        assert!(parse("C:\\Windows\\notepad.exe\n*.exe\nfoo/bar\n").is_empty());
    }

    #[test]
    fn the_default_list_is_small_and_safe() {
        let l = parse(super::DEFAULT);
        assert_eq!(l, ["charmap.exe"]);
    }
}
