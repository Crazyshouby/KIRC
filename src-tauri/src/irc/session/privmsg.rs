//! Les messages reçus (PRIVMSG, NOTICE), la fenêtre où ils vont, et les réponses CTCP. Sorti de session.rs (audit du 3/11/2026, 4.6).

use super::*;

impl Session {
    pub(super) fn on_privmsg(&mut self, msg: &Message, time: i64, notice: bool, out: &mut Vec<Out>) {
        if self.is_ignored(msg) {
            return;
        }
        let msgid = msg.tag("msgid").filter(|v| valid_msgid(v)).unwrap_or("").to_string();
        let reply_to = msg.tag("+draft/reply").or_else(|| msg.tag("+reply")).filter(|v| valid_msgid(v)).unwrap_or("").to_string();
        let src_nick = msg.source().map_or("", |s| s.nick).to_string();
        let from_server = msg.source().is_none_or(|s| s.user.is_empty() && s.host.is_empty() && s.nick.contains('.'));
        let target = msg.param(0).to_string();
        let text = msg.param(1).to_string();

        // CTCP: \x01COMMAND args\x01
        if text.starts_with('\u{1}') {
            let body = text.trim_matches('\u{1}');
            let (cmd, rest) = body.split_once(' ').unwrap_or((body, ""));
            if notice {
                // A CTCP reply (VERSION, PING...): show it in the console.
                let t = format!("Réponse CTCP {} de {src_nick} : {rest}", cmd.to_ascii_uppercase());
                out.push(self.line("", LineKind::Info, "", t, time, false, false));
                return;
            }
            if cmd.eq_ignore_ascii_case("ACTION") {
                let Some(buffer) = self.message_buffer(&src_nick, &target, out) else { return };
                let hl = self.mentions_me(rest) || !self.is_channel(&target);
                out.push(Self::with_ids(self.line(&buffer, LineKind::Action, &src_nick, rest.to_string(), time, hl, false), &msgid, &reply_to));
                return;
            }
            self.ctcp_reply(&src_nick, cmd, rest, time, out);
            return;
        }

        if notice {
            if from_server || src_nick.is_empty() {
                out.push(self.line("", LineKind::Notice, &src_nick, text, time, false, false));
                return;
            }
            // NickServ told us we are identified: no need to wait to join.
            if fold(&src_nick) == "nickserv" && self.join_at.is_some() {
                let t = fold(&text);
                if ["identified", "recognized", "recognised", "reconnu", "identifi", "accepted", "logged in"]
                    .iter()
                    .any(|k| t.contains(k))
                {
                    self.join_at = Some(0);
                }
            }
            let buffer = if self.is_channel(&target) {
                // A notice to a channel we are not on goes to the console, never to a new buffer.
                self.channels.get(&fold(&target)).map_or_else(String::new, |c| c.name.clone())
            } else if self.queries.contains_key(&fold(&src_nick)) {
                self.queries[&fold(&src_nick)].clone()
            } else {
                String::new()
            };
            let hl = self.is_channel(&target) && self.mentions_me(&text);
            out.push(self.line(&buffer, LineKind::Notice, &src_nick, text, time, hl, false));
            return;
        }

        let Some(buffer) = self.message_buffer(&src_nick, &target, out) else { return };
        let hl = !self.is_channel(&target) || self.mentions_me(&text);
        out.push(Self::with_ids(self.line(&buffer, LineKind::Msg, &src_nick, text, time, hl, false), &msgid, &reply_to));
    }

    /// The buffer an incoming message belongs to; opens a query for a private message.
    /// `None` for what must be ignored: a message to a channel we are not on, or without a sender.
    pub(super) fn message_buffer(&mut self, from: &str, target: &str, out: &mut Vec<Out>) -> Option<String> {
        if self.is_channel(target) {
            return self.channels.get(&fold(target)).map(|c| c.name.clone());
        }
        if from.is_empty() {
            return None;
        }
        let fresh = !self.queries.contains_key(&fold(from));
        let q = self.query_name(from);
        if fresh {
            out.push(Out::Ui(Event::Query { buffer: q.clone() }));
        }
        Some(q)
    }

    pub(super) fn ctcp_reply(&mut self, from: &str, cmd: &str, args: &str, now: i64, out: &mut Vec<Out>) {
        let up = cmd.to_ascii_uppercase();
        let reply = match up.as_str() {
            "VERSION" => format!("VERSION {}", self.cfg.version),
            "PING" => format!("PING {args}"),
            "TIME" => "TIME (non communiquée)".to_string(),
            "SOURCE" => "SOURCE https://github.com/Crazyshouby/cdx-messages".to_string(),
            _ => return,
        };
        // At most one reply per second: a flood of CTCP requests is a classic annoyance.
        if now - self.last_ctcp_reply < 1000 && self.last_ctcp_reply != 0 {
            return;
        }
        self.last_ctcp_reply = now;
        out.push(Out::Send(Message::new("NOTICE", &[from, &format!("\u{1}{reply}\u{1}")]).to_string()));
        out.push(self.info("", format!("Requête CTCP {up} de {from}"), now));
    }
}
