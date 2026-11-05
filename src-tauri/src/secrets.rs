//! Passwords live in the Windows credential manager, never in config.json.
//! Each server has two: "pass" (the server password) and "auth" (the SASL or
//! NickServ password).

const SERVICE: &str = "CDX Messages";
/// The name before 2026-11-02: a password still stored under it is copied to SERVICE the first time it is read, then removed.
const LEGACY_SERVICE: &str = "KIRC";

fn entry_in(service: &str, server: &str, kind: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, &format!("{server}/{kind}")).map_err(|e| format!("coffre Windows indisponible : {e}"))
}

fn entry(server: &str, kind: &str) -> Result<keyring::Entry, String> {
    entry_in(SERVICE, server, kind)
}

fn read(service: &str, server: &str, kind: &str) -> Option<String> {
    match entry_in(service, server, kind).ok()?.get_password() {
        Ok(p) if !p.is_empty() => Some(p),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            log::warn!("lecture du mot de passe ({server}/{kind}) : {e}");
            None
        }
    }
}

pub fn get(server: &str, kind: &str) -> Option<String> {
    if let Some(p) = read(SERVICE, server, kind) {
        return Some(p);
    }
    let p = read(LEGACY_SERVICE, server, kind)?;
    if set(server, kind, &p).is_ok() {
        if let Ok(e) = entry_in(LEGACY_SERVICE, server, kind) {
            let _ = e.delete_credential();
        }
    }
    Some(p)
}

/// Stores a password; an empty one removes it.
pub fn set(server: &str, kind: &str, value: &str) -> Result<(), String> {
    let e = entry(server, kind)?;
    if value.is_empty() {
        return match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("suppression du mot de passe impossible : {err}")),
        };
    }
    e.set_password(value).map_err(|err| format!("enregistrement du mot de passe impossible : {err}"))
}

pub fn forget_server(server: &str) {
    for kind in ["pass", "auth"] {
        let _ = set(server, kind, "");
    }
}
