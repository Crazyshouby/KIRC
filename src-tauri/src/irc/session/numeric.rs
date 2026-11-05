//! Les réponses numériques du serveur (bienvenue, ISUPPORT, WHOIS, LIST, erreurs), la jonction automatique et l'horloge. Sorti de session.rs (audit du 3/11/2026, 4.6).

use super::*;

impl Session {
    pub(super) fn on_numeric(&mut self, num: u16, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let rest = || msg.params.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
        match num {
            1 => {
                self.registered = true;
                if !msg.param(0).is_empty() {
                    self.nick = msg.param(0).to_string();
                }
                out.push(Out::Ui(Event::Status { state: ConnState::Connected, text: msg.last().to_string() }));
                out.push(Out::Ui(Event::Me { nick: self.nick.clone(), network: self.network.clone() }));
                out.push(self.line("", LineKind::Info, "", msg.last().to_string(), time, false, false));
                self.after_welcome(time, out);
            }
            2 | 3 | 4 | 250..=255 | 265 | 266 => {
                out.push(self.line("", LineKind::Info, "", rest(), time, false, false));
            }
            5 => self.on_isupport(msg, out, time),
            375 | 372 | 377 | 378 => {
                out.push(self.line("", LineKind::Motd, "", msg.last().to_string(), time, false, false));
            }
            376 | 422 => {
                out.push(self.line("", LineKind::Motd, "", msg.last().to_string(), time, false, false));
            }
            331 => {
                let name = self.channel_name(msg.param(1));
                out.push(Out::Ui(Event::Topic { buffer: name.clone(), topic: String::new() }));
            }
            332 => {
                let name = self.channel_name(msg.param(1));
                let topic = msg.param(2).to_string();
                out.push(Out::Ui(Event::Topic { buffer: name.clone(), topic: topic.clone() }));
                out.push(self.line(&name, LineKind::Topic, "", format!("Sujet : {topic}"), time, false, false));
            }
            333 => {
                let name = self.channel_name(msg.param(1));
                out.push(self.line(&name, LineKind::Info, "", format!("Sujet défini par {}", msg.param(2)), time, false, false));
            }
            353 => {
                let key = fold(msg.param(2));
                let entries: Vec<UserEntry> = msg.last().split_whitespace().map(|n| self.user_entry(n)).collect();
                if let Some(ch) = self.channels.get_mut(&key) {
                    ch.pending.get_or_insert_with(Vec::new).extend(entries);
                }
            }
            366 => {
                let key = fold(msg.param(1));
                if let Some(ch) = self.channels.get_mut(&key) {
                    if let Some(list) = ch.pending.take() {
                        ch.users = list.into_iter().map(|u| (fold(&u.nick), u)).collect();
                        let users = Self::sorted_users(ch);
                        out.push(Out::Ui(Event::Users { buffer: ch.name.clone(), users }));
                    }
                }
            }
            324 => {
                let name = self.channel_name(msg.param(1));
                out.push(self.line(&name, LineKind::Info, "", format!("Modes de {name} : {}", msg.params.iter().skip(2).cloned().collect::<Vec<_>>().join(" ")), time, false, false));
            }
            329 => {}
            // WHOIS / WHOWAS
            // WHOIS is collected and shown as one card (see Event::Whois); WHOWAS stays a line.
            311 => {
                self.whois = Some(WhoisInfo {
                    nick: msg.param(1).to_string(),
                    user: msg.param(2).to_string(),
                    host: msg.param(3).to_string(),
                    realname: msg.last().to_string(),
                    ..Default::default()
                });
            }
            314 => {
                let t = format!("{} était {}@{} : {}", msg.param(1), msg.param(2), msg.param(3), msg.last());
                out.push(self.line(&self.reply_buffer.clone(), LineKind::Info, "", t, time, false, false));
            }
            312 => {
                if let Some(w) = self.whois.as_mut() {
                    w.server = msg.param(2).to_string();
                    w.server_info = msg.last().to_string();
                }
            }
            313 => {
                if let Some(w) = self.whois.as_mut() {
                    w.operator = true;
                }
            }
            317 => {
                if let Some(w) = self.whois.as_mut() {
                    w.idle = msg.param(2).parse().ok();
                    w.signon = msg.param(3).parse().ok();
                }
            }
            319 => {
                if let Some(w) = self.whois.as_mut() {
                    w.channels = msg.last().to_string();
                }
            }
            330 => {
                if let Some(w) = self.whois.as_mut() {
                    w.account = msg.param(2).to_string();
                }
            }
            671 => {
                if let Some(w) = self.whois.as_mut() {
                    w.secure = true;
                }
            }
            318 => {
                if let Some(w) = self.whois.take() {
                    out.push(Out::Ui(Event::Whois { info: Box::new(w) }));
                }
            }
            369 => {}
            301 => {
                if let Some(w) = self.whois.as_mut() {
                    w.away = msg.last().to_string();
                } else {
                    let t = format!("{} est absent : {}", msg.param(1), msg.last());
                    let b = self.reply_target_for(msg.param(1));
                    out.push(self.line(&b, LineKind::Info, "", t, time, false, false));
                }
            }
            // LIST: collected whole (a big network sends tens of thousands), shown in its own window.
            321 => {
                self.list = Some(Vec::new());
                out.push(Out::Ui(Event::ListStart));
            }
            322 => {
                if let Some(l) = self.list.as_mut() {
                    if l.len() < MAX_LIST {
                        l.push(ChannelEntry { name: msg.param(1).to_string(), users: msg.param(2).parse().unwrap_or(0), topic: msg.last().to_string() });
                    }
                }
            }
            323 => {
                if let Some(entries) = self.list.take() {
                    out.push(Out::Ui(Event::ChannelList { entries }));
                }
            }
            305 => {
                self.away = false;
                out.push(Out::Ui(Event::Away { away: false }));
                out.push(self.info(&self.reply_buffer.clone(), "Vous n'êtes plus marqué absent", time));
            }
            306 => {
                self.away = true;
                out.push(Out::Ui(Event::Away { away: true }));
                out.push(self.info(&self.reply_buffer.clone(), "Vous êtes marqué absent", time));
            }
            // SASL
            900 => out.push(self.info("", format!("Connecté en tant que {}", msg.param(2)), time)),
            903 => {
                self.sasl_ok = true;
                self.sasl_pending = false;
                out.push(self.info("", "Authentification SASL réussie", time));
                self.cap_end(out);
            }
            902 | 904 | 905 | 906 | 907 => {
                self.sasl_pending = false;
                out.push(self.error("", format!("Authentification SASL échouée : {}", msg.last()), time));
                self.cap_end(out);
            }
            // Nick in use / erroneous, while registering.
            432 | 433 | 436 if !self.registered => {
                self.nick_tries += 1;
                let next = match self.cfg.alt_nicks.get(self.nick_tries - 1) {
                    Some(n) if !n.is_empty() => n.clone(),
                    _ => format!("{}{}", self.cfg.nick, "_".repeat(self.nick_tries - self.cfg.alt_nicks.len().min(self.nick_tries - 1))),
                };
                out.push(self.info("", format!("Pseudo {} indisponible, essai de {next}", msg.param(1)), time));
                self.nick = next.clone();
                out.push(Out::SendNow(Message::new("NICK", &[&next]).to_string()));
            }
            464 => out.push(self.error("", "Mot de passe du serveur refusé", time)),
            // Everything else in the error ranges: show it where the user is looking.
            400..=599 => {
                let b = self.reply_buffer.clone();
                out.push(self.error(&b, rest(), time));
            }
            _ => {}
        }
    }

    pub(super) fn reply_target_for(&self, nick: &str) -> String {
        self.queries.get(&fold(nick)).cloned().unwrap_or_else(|| self.reply_buffer.clone())
    }

    pub(super) fn on_isupport(&mut self, msg: &Message, out: &mut Vec<Out>, now: i64) {
        let n = msg.params.len();
        for tok in msg.params.iter().take(n.saturating_sub(1)).skip(1) {
            let (key, val) = tok.split_once('=').unwrap_or((tok.as_str(), ""));
            match key {
                "PREFIX" => {
                    // (ov)@+
                    if let Some(rest) = val.strip_prefix('(') {
                        if let Some((modes, syms)) = rest.split_once(')') {
                            let v: Vec<(char, char)> = modes.chars().zip(syms.chars()).collect();
                            if !v.is_empty() {
                                self.prefixes = v;
                            }
                        }
                    }
                }
                "CHANTYPES" if !val.is_empty() => self.chantypes = val.to_string(),
                "CHANMODES" => {
                    for (i, part) in val.split(',').take(4).enumerate() {
                        self.chanmodes[i] = part.to_string();
                    }
                }
                "NETWORK" => {
                    self.network = val.to_string();
                    out.push(Out::Ui(Event::Me { nick: self.nick.clone(), network: self.network.clone() }));
                    out.push(self.info("", format!("Réseau : {val}"), now));
                }
                _ => {}
            }
        }
    }

    pub(super) fn after_welcome(&mut self, now: i64, out: &mut Vec<Out>) {
        if !self.sasl_ok {
            if let Some(p) = self.cfg.nickserv_password.clone().filter(|p| !p.is_empty()) {
                out.push(Out::Send(Message::new("PRIVMSG", &["NickServ", &format!("IDENTIFY {p}")]).to_string()));
                out.push(self.info("", "Identification auprès de NickServ…", now));
                // Join once NickServ confirms, or after a short wait.
                self.join_at = Some(now + 3000);
                return;
            }
        }
        self.do_join(now, out);
    }

    pub(super) fn do_join(&mut self, _now: i64, out: &mut Vec<Out>) {
        self.join_at = None;
        let mut list: Vec<String> = self.cfg.autojoin.clone();
        for r in &self.rejoin {
            if !list.iter().any(|a| fold(a.split(' ').next().unwrap_or("")) == fold(r.split(' ').next().unwrap_or(""))) {
                list.push(r.clone());
            }
        }
        self.rejoin.clear();
        let mut chans: Vec<String> = Vec::new();
        for entry in list {
            let mut it = entry.split_whitespace();
            let Some(c) = it.next() else { continue };
            chans.push(c.to_string());
            if let Some(k) = it.next() {
                self.keys.insert(fold(c), k.to_string());
            }
        }
        // Channels with a key first, as the protocol wants.
        chans.sort_by_key(|c| !self.keys.contains_key(&fold(c)));
        for c in chans {
            let line = match self.keys.get(&fold(&c)) {
                Some(k) => Message::new("JOIN", &[&c, k]).to_string(),
                None => Message::new("JOIN", &[&c]).to_string(),
            };
            out.push(Out::Send(line));
        }
    }

    /// Called about once a second by the network layer.
    pub fn tick(&mut self, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(at) = self.join_at {
            if now >= at {
                self.do_join(now, &mut out);
            }
        }
        out
    }
}
