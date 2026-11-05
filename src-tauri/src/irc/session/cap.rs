//! La négociation des capacités IRCv3 (CAP LS / REQ / ACK) et l'authentification SASL PLAIN. Sorti de session.rs (audit du 3/11/2026, 4.6).

use super::*;

impl Session {
    pub(super) fn on_cap(&mut self, msg: &Message, out: &mut Vec<Out>) {
        let sub = msg.param(1).to_ascii_uppercase();
        match sub.as_str() {
            "LS" => {
                let more = msg.param(2) == "*";
                for cap in msg.last().split_whitespace() {
                    let (name, value) = cap.split_once('=').unwrap_or((cap, ""));
                    self.cap_available.insert(name.to_string(), value.to_string());
                }
                if more {
                    return;
                }
                let mut want: Vec<&str> = ["server-time", "multi-prefix", "away-notify", "message-tags"]
                    .into_iter()
                    .filter(|c| self.cap_available.contains_key(*c))
                    .collect();
                if self.cfg.sasl.is_some() {
                    // "sasl=PLAIN,EXTERNAL": the list, when given, must include PLAIN.
                    let plain = self.cap_available.get("sasl").map(|v| v.is_empty() || v.split(',').any(|m| m.eq_ignore_ascii_case("PLAIN")));
                    match plain {
                        Some(true) => want.push("sasl"),
                        _ => out.push(self.error("", "Ce serveur ne propose pas SASL PLAIN : authentification impossible", 0)),
                    }
                }
                // One request per capability: a refusal of one does not cancel the others.
                self.cap_pending = want.len();
                if want.is_empty() {
                    self.cap_end(out);
                }
                for c in want {
                    out.push(Out::SendNow(format!("CAP REQ :{c}")));
                }
            }
            "ACK" | "NAK" => {
                let acked = sub == "ACK";
                for cap in msg.last().split_whitespace() {
                    let name = cap.trim_start_matches(['-', '~', '=']);
                    if acked {
                        self.cap_enabled.insert(name.to_string());
                    } else if name == "sasl" && self.cfg.sasl.is_some() {
                        out.push(self.error("", "Le serveur a refusé SASL : authentification impossible", 0));
                    }
                    self.cap_pending = self.cap_pending.saturating_sub(1);
                }
                if self.cap_pending == 0 {
                    if self.cap_enabled.contains("sasl") && self.cfg.sasl.is_some() && !self.sasl_ok {
                        self.sasl_pending = true;
                        out.push(Out::SendNow("AUTHENTICATE PLAIN".into()));
                    } else {
                        self.cap_end(out);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn cap_end(&mut self, out: &mut Vec<Out>) {
        if !self.cap_ended {
            self.cap_ended = true;
            out.push(Out::SendNow("CAP END".into()));
        }
    }

    pub(super) fn on_authenticate(&mut self, msg: &Message, out: &mut Vec<Out>) {
        if !self.sasl_pending || msg.param(0) != "+" {
            return;
        }
        let Some((account, password)) = &self.cfg.sasl else { return };
        let raw = format!("{account}\0{account}\0{password}");
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        let bytes = b64.as_bytes();
        for chunk in bytes.chunks(400) {
            out.push(Out::SendNow(format!("AUTHENTICATE {}", String::from_utf8_lossy(chunk))));
        }
        if bytes.len() % 400 == 0 {
            out.push(Out::SendNow("AUTHENTICATE +".into()));
        }
        // The password only lives as long as the exchange.
        self.sasl_pending = false;
    }
}
