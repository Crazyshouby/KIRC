"use strict";
// KIRC pages. One page (index.html), opened as ?w=<kind>[&s=<server>&b=<buffer>&t=<title>] by the
// application: launcher, servers (address book), settings, about, join, console, chat.
// Everything that comes from the network is put on the page with textContent / text nodes, never
// as HTML: a message can not inject markup.

const $ = s => document.querySelector(s);
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, cb) => T.event.listen(ev, cb);

const params = new URLSearchParams(location.search);
const PANEL = params.get("w") || "all";
const SERVER = params.get("s") || "";
const BUFFER = params.get("b") || "";
document.documentElement.dataset.panel = PANEL;
document.title = params.get("t") || "KIRC";
for (const el of document.querySelectorAll("[data-title]")) el.textContent = document.title;

// Theme: an accent ramp chosen in the settings (see themes.css). The window is opened with its
// theme in the address, so it never shows the wrong one first; a change reaches open windows as an event.
function applyTheme(name) {
  if (name && name !== "kdx") document.documentElement.dataset.theme = name;
  else delete document.documentElement.dataset.theme;
}
applyTheme(params.get("th"))
fillPix();

// RFC 1459 case folding, as the server does it.
const fold = s => s.replace(/[A-Z\[\]\\~]/g, c => ({ "[": "{", "]": "}", "\\": "|", "~": "^" }[c] || c.toLowerCase()));

// ---------------------------------------------------------------- toasts
const TOAST_MAX = 3;
function toast(msg, kind) {
  const box = $("#toasts");
  if (!box) return;
  const t = document.createElement("div");
  t.className = "toast" + (kind === "ok" ? " ok" : "");
  const text = document.createElement("span");
  text.className = "toast-text";
  text.textContent = msg;
  t.appendChild(text);
  let timer = 0;
  const dismiss = () => { clearTimeout(timer); t.classList.remove("show"); setTimeout(() => t.remove(), 250); };
  t.addEventListener("click", dismiss);
  while (box.children.length >= TOAST_MAX) box.firstChild.remove();
  box.appendChild(t);
  requestAnimationFrame(() => t.classList.add("show"));
  timer = setTimeout(dismiss, 5000);
}

// Calls the application; an error is shown and returned as null.
async function call(cmd, args) {
  try { return await invoke(cmd, args); }
  catch (e) { toast(String(e)); return null; }
}

// ---------------------------------------------------------------- window chrome
const TB_GLYPHS = {
  x: '<svg viewBox="0 0 9 9"><path d="M1 1L8 8M8 1L1 8" stroke="#fff" stroke-width="2" fill="none"/></svg>',
  minus: '<svg viewBox="0 0 9 9"><path d="M1 4.5H8" stroke="#fff" stroke-width="2" fill="none"/></svg>',
};
function initChrome() {
  for (const tb of document.querySelectorAll(".tb")) {
    // Moving: the page reports press / movement / release, the application reads the cursor and
    // moves the window. Two presses within 400 ms roll the window up or down.
    let lastDown = 0, dragging = false, queued = false;
    tb.addEventListener("pointerdown", ev => {
      if (ev.button !== 0 || ev.target.closest(".tb-btn")) return;
      const now = performance.now();
      if (now - lastDown < 400) { lastDown = 0; invoke("win_shade"); return; }
      lastDown = now;
      dragging = true;
      tb.setPointerCapture(ev.pointerId);
      invoke("win_drag", { phase: "start" });
    });
    tb.addEventListener("pointermove", () => {
      if (!dragging || queued) return;
      queued = true;
      requestAnimationFrame(() => { queued = false; if (dragging) invoke("win_drag", { phase: "move" }); });
    });
    const end = () => { if (dragging) { dragging = false; invoke("win_drag", { phase: "end" }); } };
    tb.addEventListener("pointerup", end);
    tb.addEventListener("pointercancel", end);
    for (const b of tb.querySelectorAll(".tb-btn")) {
      b.innerHTML = TB_GLYPHS[b.dataset.glyph] || TB_GLYPHS.minus;
      b.addEventListener("click", () => invoke(b.dataset.act === "close" ? "win_close" : "win_shade"));
    }
  }
  for (const g of document.querySelectorAll(".grip-rz")) {
    let sizing = false, queued = false;
    g.addEventListener("pointerdown", ev => {
      if (ev.button !== 0) return;
      sizing = true;
      g.setPointerCapture(ev.pointerId);
      invoke("win_drag", { phase: "rstart" });
    });
    g.addEventListener("pointermove", () => {
      if (!sizing || queued) return;
      queued = true;
      requestAnimationFrame(() => { queued = false; if (sizing) invoke("win_drag", { phase: "rmove" }); });
    });
    const end = () => { if (sizing) { sizing = false; invoke("win_drag", { phase: "end" }); } };
    g.addEventListener("pointerup", end);
    g.addEventListener("pointercancel", end);
  }
  listen("active", e => document.body.classList.toggle("active", !!e.payload));
  listen("theme", e => applyTheme(e.payload));
  listen("shade", e => document.body.classList.toggle("shaded", !!e.payload));
  invoke("win_ready");
}
initChrome();
// The right-click menu of the browser has nothing to offer here (but keep it in text fields).
document.addEventListener("contextmenu", e => { if (!e.target.closest("input, textarea")) e.preventDefault(); });

// ---------------------------------------------------------------- IRC text
const MIRC = ["#ffffff", "#8c8c8c", "#4a6aff", "#18c828", "#ff4a4a", "#c04a2a", "#c040c0", "#fc9a20",
  "#ffff40", "#40ff40", "#20c0c0", "#40ffff", "#6a8aff", "#ff60ff", "#a0a0a0", "#d2d2d2"];
const URL_RE = /\bhttps?:\/\/[^\s<>"'\x00-\x1f]+/gi;

function makeLink(text, url) {
  const a = document.createElement("a");
  a.className = "link";
  a.href = "#";
  a.title = url;
  a.textContent = text;
  a.addEventListener("click", ev => { ev.preventDefault(); ev.stopPropagation(); call("open_url", { url }); });
  return a;
}

// Splits plain text into text nodes and links; trailing punctuation stays out of the link.
function appendLinked(parent, text) {
  let last = 0;
  for (const m of text.matchAll(URL_RE)) {
    let url = m[0];
    while (/[.,;:!?'"]$/.test(url) || (url.endsWith(")") && !url.includes("("))) url = url.slice(0, -1);
    if (url.length <= 8) continue;
    if (m.index > last) parent.appendChild(document.createTextNode(text.slice(last, m.index)));
    parent.appendChild(makeLink(url, url));
    last = m.index + url.length;
  }
  if (last < text.length) parent.appendChild(document.createTextNode(text.slice(last)));
}

// Renders mIRC formatting (bold, italic, underline, strike, reverse, colours) and links.
function renderIrc(parent, text) {
  let st = { b: false, i: false, u: false, s: false, m: false, r: false, fg: null, bg: null };
  let buf = "";
  const flush = () => {
    if (!buf) return;
    const plain = !(st.b || st.i || st.u || st.s || st.m || st.r || st.fg !== null || st.bg !== null);
    if (plain) appendLinked(parent, buf);
    else {
      const sp = document.createElement("span");
      if (st.b) sp.style.fontWeight = "800";
      if (st.i) sp.style.fontStyle = "italic";
      const deco = [st.u && "underline", st.s && "line-through"].filter(Boolean).join(" ");
      if (deco) sp.style.textDecoration = deco;
      let fg = st.fg !== null ? MIRC[st.fg] : null, bg = st.bg !== null ? MIRC[st.bg] : null;
      if (st.r) { [fg, bg] = [bg || "#000", fg || "#00ff00"]; }
      if (fg) sp.style.color = fg;
      if (bg) { sp.style.background = bg; }
      appendLinked(sp, buf);
      parent.appendChild(sp);
    }
    buf = "";
  };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\x02") { flush(); st.b = !st.b; }
    else if (c === "\x1d") { flush(); st.i = !st.i; }
    else if (c === "\x1f") { flush(); st.u = !st.u; }
    else if (c === "\x1e") { flush(); st.s = !st.s; }
    else if (c === "\x11") { flush(); st.m = !st.m; }
    else if (c === "\x16") { flush(); st.r = !st.r; }
    else if (c === "\x0f") { flush(); st = { b: false, i: false, u: false, s: false, m: false, r: false, fg: null, bg: null }; }
    else if (c === "\x03") {
      flush();
      const m = /^(\d{1,2})(?:,(\d{1,2}))?/.exec(text.slice(i + 1));
      if (m) {
        i += m[0].length;
        const f = +m[1] % 100, g = m[2] === undefined ? null : +m[2] % 100;
        st.fg = f < 16 ? f : null;
        if (g !== null) st.bg = g < 16 ? g : null;
      } else { st.fg = null; st.bg = null; }
    }
    else if (c === "\x04") { flush(); const m = /^[0-9a-fA-F]{6}(?:,[0-9a-fA-F]{6})?/.exec(text.slice(i + 1)); if (m) i += m[0].length; }
    else if (c < " " && c !== "\t") { /* other control characters are dropped */ }
    else buf += c;
  }
  flush();
}

const NICK_COLORS = ["#ff5a4a", "#faa320", "#ffe090", "#8af08a", "#86a6ff", "#ff5cf0", "#5cf0ff", "#ffa6a6", "#c8a6ff", "#ffd24a", "#7af0c0", "#ff8a3c"];
function nickColor(n) {
  let h = 0;
  for (const c of fold(n)) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return NICK_COLORS[h % NICK_COLORS.length];
}

const pad2 = n => String(n).padStart(2, "0");
function stamp(ms) { const d = new Date(ms); return pad2(d.getHours()) + ":" + pad2(d.getMinutes()); }

// One line of a window, as a <div>.
function renderLine(line) {
  const div = document.createElement("div");
  div.className = "ln " + line.kind + (line.own ? " own" : "") + (line.highlight && !line.own ? " hl" : "") + (line.past ? " past" : "");
  const ts = document.createElement("span");
  ts.className = "ts";
  ts.textContent = stamp(line.time);
  div.appendChild(ts);
  const tx = document.createElement("span");
  tx.className = "tx";
  switch (line.kind) {
    case "msg": {
      const nk = document.createElement("span");
      nk.className = "nk";
      nk.style.color = nickColor(line.nick);
      nk.textContent = "<" + line.nick + ">";
      div.appendChild(nk);
      renderIrc(tx, line.text);
      break;
    }
    case "action": tx.appendChild(document.createTextNode("* " + line.nick + " ")); renderIrc(tx, line.text); break;
    case "notice": tx.appendChild(document.createTextNode(line.nick ? "-" + line.nick + "- " : "")); renderIrc(tx, line.text); break;
    case "error": tx.appendChild(document.createTextNode("!! ")); renderIrc(tx, line.text); break;
    case "motd": renderIrc(tx, line.text); break;
    case "raw": renderIrc(tx, line.text); break;
    default: tx.appendChild(document.createTextNode("*** ")); renderIrc(tx, line.text);
  }
  div.appendChild(tx);
  return div;
}

// A scrolling log that follows the end unless the reader scrolled up.
class Log {
  constructor(el, limit, newBtn) {
    this.el = el; this.limit = limit; this.newBtn = newBtn;
    this.stick = true;
    el.addEventListener("scroll", () => {
      this.stick = el.scrollTop + el.clientHeight >= el.scrollHeight - 6;
      if (this.stick && this.newBtn) this.newBtn.hidden = true;
    });
    if (newBtn) newBtn.addEventListener("click", () => { this.toEnd(); el.focus(); });
  }
  toEnd() { this.el.scrollTop = this.el.scrollHeight; this.stick = true; if (this.newBtn) this.newBtn.hidden = true; }
  add(line) { this.addAll([line]); }
  addAll(lines) {
    const frag = document.createDocumentFragment();
    for (const l of lines) frag.appendChild(renderLine(l));
    this.el.appendChild(frag);
    while (this.el.childElementCount > this.limit) this.el.firstElementChild.remove();
    if (this.stick) this.el.scrollTop = this.el.scrollHeight;
    else if (this.newBtn) this.newBtn.hidden = false;
  }
  clear() { this.el.textContent = ""; this.stick = true; if (this.newBtn) this.newBtn.hidden = true; }
}

function applySettings(s) {
  document.body.classList.toggle("nojoins", !s.show_joins);
  document.body.classList.toggle("nots", !s.show_timestamps);
}

// Events of the application. Events that arrive while a snapshot is being fetched wait for it,
// and those the snapshot already contains (seq <= snapshot.seq) are dropped.
function subscribe(handler) {
  const state = { ready: false, queue: [], seq: 0 };
  listen("irc", e => {
    const p = e.payload;
    if (p.server !== SERVER) return; // every window hears every server: seq counters are per server
    if (state.ready) { if (p.seq > state.seq) handler(p); }
    else if (state.queue.length < 5000) state.queue.push(p);
  });
  return {
    ready(seq) {
      state.seq = seq; state.ready = true;
      for (const p of state.queue) if (p.seq > seq) handler(p);
      state.queue = [];
    },
  };
}

const STATUS_TEXT = { connecting: "Connexion…", registering: "Enregistrement…", connected: "Connecté", disconnected: "Déconnecté", reconnecting: "Reconnexion…" };
const statusOrb = st => st === "connected" ? "green" : st === "disconnected" ? "" : "amber";

// Sends a typed line; /clear is the page's own business.
function makeInput(form, input, log, getBuffer) {
  const history = [];
  let hi = 0, draft = "";
  form.addEventListener("submit", async ev => {
    ev.preventDefault();
    const text = input.value;
    if (!text.trim()) return;
    input.value = "";
    if (history[history.length - 1] !== text) history.push(text);
    if (history.length > 100) history.shift();
    hi = history.length; draft = "";
    const t = text.trim().toLowerCase();
    if (t === "/clear" || t === "/effacer") { log.clear(); call("clear_buffer", { server: SERVER, buffer: getBuffer() }); return; }
    try { await invoke("send_input", { server: SERVER, buffer: getBuffer(), text }); }
    catch (e) { log.add({ buffer: getBuffer(), kind: "error", nick: "", text: String(e), time: Date.now(), highlight: false, own: false }); }
  });
  input.addEventListener("keydown", ev => {
    if (ev.key === "ArrowUp" && history.length) {
      if (hi === history.length) draft = input.value;
      hi = Math.max(0, hi - 1); input.value = history[hi]; ev.preventDefault();
    } else if (ev.key === "ArrowDown" && hi < history.length) {
      hi++; input.value = hi === history.length ? draft : history[hi]; ev.preventDefault();
    }
  });
}

// ================================================================ launcher
function initLauncher() {
  let ov = null;
  const sel = $("#serverSelect");
  const stateOf = id => (ov && ov.states[id]) || { status: "disconnected", channels: [] };
  const current = () => sel.value;

  function render() {
    const keep = sel.value || localStorage.getItem("kirc.server") || "";
    sel.textContent = "";
    for (const s of ov.servers) {
      const o = document.createElement("option");
      o.value = s.id;
      const st = stateOf(s.id).status;
      o.textContent = (st === "connected" ? "● " : st === "disconnected" ? "○ " : "◐ ") + s.name;
      sel.appendChild(o);
    }
    if (ov.servers.some(s => s.id === keep)) sel.value = keep;
    sel.disabled = ov.servers.length === 0;

    const st = ov.servers.length ? stateOf(current()).status : "disconnected";
    const btn = $("#connectBtn");
    btn.disabled = ov.servers.length === 0;
    btn.textContent = st === "connected" ? "Déconnecter" : st === "disconnected" ? "Connecter" : "Annuler";
    btn.classList.toggle("quiet", st !== "disconnected");
    for (const id of ["#btnConsole", "#btnJoin"]) $(id).disabled = st !== "connected" && id === "#btnJoin" || ov.servers.length === 0;

    const connected = ov.servers.filter(s => stateOf(s.id).status === "connected").length;
    const busy = ov.servers.some(s => ["connecting", "registering", "reconnecting"].includes(stateOf(s.id).status));
    const pill = $("#statusPill");
    pill.classList.toggle("on", connected > 0);
    pill.classList.toggle("busy", busy);
    $("#statusText").textContent = ov.servers.length === 0 ? "Carnet vide"
      : busy ? "Connexion en cours…" : connected ? `${connected} serveur${connected > 1 ? "s" : ""} connecté${connected > 1 ? "s" : ""}` : "Aucune connexion";
    $("#cntServers").textContent = `[${connected}]`;
    $("#cntChannels").textContent = `[${ov.servers.reduce((n, s) => n + stateOf(s.id).channels.length, 0)}]`;

    const list = $("#openList");
    list.textContent = "";
    const sv = ov.servers.length ? stateOf(current()) : { channels: [], queries: [] };
    const items = [...sv.channels.map(c => [c, "salon"]), ...(sv.queries || []).map(q => [q, "message"])];
    if (!items.length) {
      const e = document.createElement("div");
      e.className = "empty";
      e.textContent = ov.servers.length ? "Aucun salon ouvert" : "Ajoutez un serveur dans le carnet d'adresses";
      list.appendChild(e);
    }
    const act = new Map((sv.activity || []).map(a => [fold(a.name), a]));
    for (const [c, icon] of items) {
      const a = act.get(fold(c));
      const row = document.createElement("div");
      row.className = "row-item" + (a ? " unread" : "");
      row.tabIndex = 0;
      const ic = document.createElement("span"); ic.className = "pix"; ic.innerHTML = pix(icon, 14);
      const name = document.createElement("span"); name.className = "grow"; name.textContent = c;
      row.append(ic, name);
      if (a) {
        const n = document.createElement("span");
        n.className = "count" + (a.mentions ? " hot" : "");
        n.textContent = a.unread > 99 ? "99+" : String(a.unread);
        n.title = a.mentions ? `${a.unread} non lus dont ${a.mentions} mention(s)` : `${a.unread} non lus`;
        row.appendChild(n);
      }
      const open = () => call("open_window", { kind: "chat", server: current(), buffer: c });
      row.addEventListener("click", open);
      row.addEventListener("keydown", e => { if (e.key === "Enter") open(); });
      list.appendChild(row);
    }
  }
  let pending = false;
  async function refresh() {
    if (pending) return;
    pending = true;
    await new Promise(r => setTimeout(r, 30));
    pending = false;
    const o = await call("get_overview");
    if (o) { ov = o; render(); }
  }
  sel.addEventListener("change", () => { localStorage.setItem("kirc.server", sel.value); render(); });
  $("#connectBtn").addEventListener("click", async () => {
    const st = stateOf(current()).status;
    if (st === "disconnected") await call("connect_server", { id: current() });
    else await call("disconnect_server", { id: current() });
  });
  $("#btnConsole").addEventListener("click", () => call("open_window", { kind: "console", server: current() }));
  $("#btnJoin").addEventListener("click", () => call("open_window", { kind: "join" }));
  $("#btnBook").addEventListener("click", () => call("open_window", { kind: "servers" }));
  $("#btnSettings").addEventListener("click", () => call("open_window", { kind: "settings" }));
  $("#btnAbout").addEventListener("click", () => call("open_window", { kind: "about" }));
  $("#btnQuit").addEventListener("click", () => invoke("quit_app"));
  listen("irc", e => { if (["status", "joined", "parted", "me", "query"].includes(e.payload.event.kind)) refresh(); });
  // Unread counters: only the connection states are asked for, at most twice a second.
  let statesTimer = 0;
  const refreshStates = () => {
    if (statesTimer || !ov) return;
    statesTimer = setTimeout(async () => {
      statesTimer = 0;
      const s = await call("get_states");
      if (s && ov) { ov.states = s; render(); }
    }, 500);
  };
  listen("irc", e => { const k = e.payload.event.kind; if (k === "line" || k === "read") refreshStates(); });
  $("#btnList").addEventListener("click", () => call("open_window", { kind: "list", server: current() }));
  window.addEventListener("focus", refresh);
  refresh();
}

// ================================================================ address book
function initServers() {
  let ov = null, selected = "", fresh = false;
  const f = id => $("#" + id);
  const stateOf = id => (ov && ov.states[id]) || { status: "disconnected" };

  function fill(entry) {
    f("fName").value = entry.name || ""; f("fHost").value = entry.host || ""; f("fPort").value = entry.port || 6697;
    f("fTls").checked = !!entry.tls; f("fInvalid").checked = !!entry.accept_invalid_certs;
    f("fNick").value = entry.nick || ""; f("fAlt").value = entry.alt_nick || ""; f("fUser").value = entry.username || ""; f("fReal").value = entry.realname || "";
    f("fAuth").value = entry.auth || "none"; f("fAccount").value = entry.account || "";
    f("fServerPass").value = ""; f("fAuthPass").value = "";
    f("fServerPass").placeholder = entry.has_pass ? "(enregistré — laisser vide pour le garder)" : "(serveur protégé seulement)";
    f("fAuthPass").placeholder = entry.has_auth ? "(enregistré — laisser vide pour le garder)" : "";
    f("fForgetPass").checked = false; f("fForgetAuth").checked = false;
    f("rowForgetPass").hidden = !entry.has_pass; f("rowForgetAuth").hidden = !entry.has_auth;
    f("fAuto").checked = !!entry.autoconnect; f("fJoin").value = entry.autojoin || "";
    f("fPreset").value = "";
    syncAuth(); syncTls();
  }
  function syncAuth() {
    const a = f("fAuth").value;
    f("fAccount").disabled = a !== "sasl"; f("fAuthPass").disabled = a === "none";
    f("lAccount").style.opacity = f("fAccount").disabled ? 0.45 : 1; f("lAuthPass").style.opacity = f("fAuthPass").disabled ? 0.45 : 1;
  }
  function syncTls() {
    const on = f("fTls").checked;
    f("tlsHint").style.visibility = on ? "hidden" : "visible";
    f("fInvalid").disabled = !on;
  }
  function entryFromForm() {
    const base = (ov.servers.find(s => s.id === selected) || {});
    return {
      id: fresh ? "" : (base.id || ""), name: f("fName").value.trim(), host: f("fHost").value.trim(), port: +f("fPort").value || 0,
      tls: f("fTls").checked, accept_invalid_certs: f("fTls").checked && f("fInvalid").checked,
      nick: f("fNick").value.trim(), alt_nick: f("fAlt").value.trim(), username: f("fUser").value.trim(), realname: f("fReal").value.trim(),
      auth: f("fAuth").value, account: f("fAccount").value.trim(), autoconnect: f("fAuto").checked, autojoin: f("fJoin").value,
    };
  }
  function renderList() {
    const list = f("serverList");
    list.textContent = "";
    if (!ov.servers.length) {
      const e = document.createElement("div"); e.className = "empty"; e.textContent = "Aucun serveur. Choisissez un modèle ou saisissez une adresse.";
      list.appendChild(e);
    }
    for (const s of ov.servers) {
      const row = document.createElement("div");
      row.className = "row-item" + (s.id === selected && !fresh ? " sel" : "");
      row.tabIndex = 0;
      const orb = document.createElement("span"); orb.className = "orb " + statusOrb(stateOf(s.id).status);
      const name = document.createElement("span"); name.className = "grow";
      name.textContent = s.name;
      const sub = document.createElement("span"); sub.className = "sub"; sub.textContent = `${s.host}:${s.port}`;
      name.appendChild(sub);
      row.append(orb, name);
      const pick = () => { select(s.id); };
      row.addEventListener("click", pick);
      row.addEventListener("dblclick", () => call("connect_server", { id: s.id }));
      row.addEventListener("keydown", e => { if (e.key === "Enter") pick(); });
      list.appendChild(row);
    }
    const st = fresh ? "disconnected" : stateOf(selected).status;
    f("connectServer").textContent = st === "disconnected" ? "Connecter" : "Déconnecter";
    f("connectServer").classList.toggle("quiet", st !== "disconnected");
    f("delServer").disabled = fresh || !selected;
  }
  function select(id) {
    selected = id; fresh = false;
    fill(ov.servers.find(s => s.id === id) || {});
    renderList();
  }
  function startNew() {
    fresh = true; selected = "";
    fill({ port: 6697, tls: true, auth: "none" });
    renderList();
    f("fName").focus();
  }
  async function save() {
    const e = entryFromForm();
    // null keeps the stored password, "" removes it.
    const forgetPass = f("fForgetPass").checked, forgetAuth = f("fForgetAuth").checked || e.auth === "none";
    const typedPass = f("fServerPass").value, typedAuth = f("fAuthPass").value;
    const pass = forgetPass ? "" : typedPass === "" ? null : typedPass;
    const authSecret = forgetAuth ? "" : typedAuth === "" ? null : typedAuth;
    const r = await call("save_server", { entry: e, pass, authSecret });
    if (!r) return null;
    await refresh(r.id);
    if (r.warning) toast(r.warning); else toast("Serveur enregistré", "ok");
    return r.id;
  }
  async function refresh(keep) {
    const o = await call("get_overview");
    if (!o) return;
    ov = o;
    if (keep) { selected = keep; fresh = false; }
    if (!fresh && !ov.servers.some(s => s.id === selected)) selected = ov.servers.length ? ov.servers[0].id : "";
    if (keep || !f("fName").value && !f("fHost").value) {
      if (selected) fill(ov.servers.find(s => s.id === selected)); else startNew();
    }
    renderList();
  }

  f("fPreset").addEventListener("change", () => {
    const v = f("fPreset").value;
    if (!v) return;
    const [name, host, port] = v.split("|");
    f("fName").value = name; f("fHost").value = host; f("fPort").value = port; f("fTls").checked = true; syncTls();
  });
  f("fAuth").addEventListener("change", syncAuth);
  f("fTls").addEventListener("change", () => { if (f("fTls").checked && +f("fPort").value === 6667) f("fPort").value = 6697; if (!f("fTls").checked && +f("fPort").value === 6697) f("fPort").value = 6667; syncTls(); });
  f("newServer").addEventListener("click", startNew);
  f("saveServer").addEventListener("click", save);
  f("serverForm").addEventListener("submit", e => { e.preventDefault(); save(); });
  f("connectServer").addEventListener("click", async () => {
    const st = fresh ? "disconnected" : stateOf(selected).status;
    if (st !== "disconnected") { await call("disconnect_server", { id: selected }); return; }
    const id = await save();
    if (id) await call("connect_server", { id });
  });
  let armed = 0;
  f("delServer").addEventListener("click", async () => {
    if (!armed) {
      f("delServer").textContent = "Confirmer ?";
      armed = setTimeout(() => { armed = 0; f("delServer").textContent = "Supprimer"; }, 3000);
      return;
    }
    clearTimeout(armed); armed = 0; f("delServer").textContent = "Supprimer";
    if (await call("delete_server", { id: selected }) !== null) {
      selected = "";
      await refresh();
      if (selected) select(selected); else startNew(); // the form must not keep showing the deleted server
    }
  });
  listen("irc", e => { if (e.payload.event.kind === "status") call("get_overview").then(o => { if (o) { ov = o; renderList(); } }); });
  refresh().then(() => { if (!ov.servers.length) startNew(); });
}

// ================================================================ settings
function initSettings() {
  const f = id => $("#" + id);
  async function load() {
    const ov = await call("get_overview");
    if (!ov) return;
    const s = ov.settings;
    f("sNick").value = s.nick; f("sAlt").value = s.alt_nick; f("sUser").value = s.username; f("sReal").value = s.realname;
    f("sQuit").value = s.quit_message; f("sPart").value = s.part_message;
    f("sStamps").checked = s.show_timestamps; f("sJoins").checked = s.show_joins; f("sScroll").value = s.scrollback;
    f("sNotif").checked = s.notifications; f("sQueries").checked = s.open_queries; f("sConsole").checked = s.open_console;
    f("sTheme").value = s.theme;
    f("sLog").checked = s.log_enabled; f("sLogDays").value = s.log_days;
    f("sIgnores").value = s.ignores.join("\n"); f("sWords").value = s.alert_words.join("\n");
  }
  async function save() {
    const s = {
      nick: f("sNick").value.trim(), alt_nick: f("sAlt").value.trim(), username: f("sUser").value.trim(), realname: f("sReal").value,
      quit_message: f("sQuit").value, part_message: f("sPart").value,
      show_timestamps: f("sStamps").checked, show_joins: f("sJoins").checked, scrollback: +f("sScroll").value || 0,
      notifications: f("sNotif").checked, open_queries: f("sQueries").checked, open_console: f("sConsole").checked,
      theme: f("sTheme").value,
      log_enabled: f("sLog").checked, log_days: +f("sLogDays").value || 0,
      ignores: f("sIgnores").value.split("\n").map(x => x.trim()).filter(Boolean),
      alert_words: f("sWords").value.split("\n").map(x => x.trim()).filter(Boolean),
    };
    if (await call("save_settings", { settings: s }) !== null) toast("Réglages enregistrés", "ok");
  }
  f("saveSettings").addEventListener("click", save);
  f("settingsForm").addEventListener("submit", e => { e.preventDefault(); save(); });
  f("cancelSettings").addEventListener("click", () => invoke("win_close"));
  f("openLogs").addEventListener("click", () => call("open_logs_folder"));
  f("sTheme").addEventListener("change", () => applyTheme(f("sTheme").value)); // preview, this window only
  load();
}

// ================================================================ about
async function initAbout() {
  const info = await call("app_info");
  if (info) { $("#aboutVersion").textContent = "version " + info.version; $("#aboutPath").textContent = info.data_dir; }
}

// ================================================================ join
function initJoin() {
  const sel = $("#jServer");
  async function load() {
    const ov = await call("get_overview");
    if (!ov) return;
    sel.textContent = "";
    for (const s of ov.servers) {
      const st = (ov.states[s.id] || {}).status;
      const o = document.createElement("option");
      o.value = s.id; o.textContent = s.name + (st === "connected" ? "" : " (non connecté)");
      sel.appendChild(o);
    }
    const last = localStorage.getItem("kirc.server");
    const online = ov.servers.find(s => (ov.states[s.id] || {}).status === "connected");
    if (last && ov.servers.some(s => s.id === last)) sel.value = last; else if (online) sel.value = online.id;
    $("#jChannel").focus();
  }
  async function ok() {
    let chan = $("#jChannel").value.trim();
    if (!chan) { $("#jChannel").focus(); return; }
    if (!/^[#&+!]/.test(chan)) chan = "#" + chan;
    const key = $("#jKey").value.trim();
    const r = await call("send_input", { server: sel.value, buffer: "", text: "/join " + chan + (key ? " " + key : "") });
    if (r !== null) invoke("win_close");
  }
  $("#joinOk").addEventListener("click", ok);
  $("#joinCancel").addEventListener("click", () => invoke("win_close"));
  $("#joinForm").addEventListener("submit", e => { e.preventDefault(); ok(); });
  document.addEventListener("keydown", e => { if (e.key === "Escape") invoke("win_close"); });
  load();
}

// ================================================================ console
function initConsole() {
  const log = new Log($("#consoleLog"), 1000, null);
  makeInput($("#consoleForm"), $("#consoleInput"), log, () => "");
  const pill = $("#consoleStatus");
  let nick = "", network = "";
  function status(st, text) {
    pill.classList.toggle("on", st === "connected");
    pill.classList.toggle("busy", ["connecting", "registering", "reconnecting"].includes(st));
    $("#consoleStatusText").textContent = (STATUS_TEXT[st] || st) + (network ? " — " + network : "") + (nick ? " — " + nick : "") + (st !== "connected" && text ? " (" + text + ")" : "");
  }
  let lastStatus = ["disconnected", ""];
  const sub = subscribe(p => {
    const ev = p.event;
    if (ev.kind === "line" && ev.line.buffer === "") log.add(ev.line);
    else if (ev.kind === "status") { lastStatus = [ev.state, ev.text]; status(...lastStatus); }
    else if (ev.kind === "me") { nick = ev.nick; network = ev.network; status(...lastStatus); }
  });
  (async () => {
    const snap = await call("buffer_snapshot", { server: SERVER, buffer: "" });
    if (!snap) return;
    applySettings(snap.settings);
    log.limit = snap.settings.scrollback;
    nick = snap.server.nick; network = snap.server.network; lastStatus = [snap.server.status, snap.server.status_text];
    status(...lastStatus);
    log.addAll(snap.buffer.lines);
    log.toEnd();
    sub.ready(snap.seq);
    $("#consoleInput").focus();
  })();
}

// ================================================================ chat
const RANKS = "~&@%+";
const SYMBOL_NAMES = { "~": "propriétaire", "&": "administrateur", "@": "opérateur", "%": "demi-opérateur", "+": "voix" };

function initChat() {
  const isChannel = /^[#&+!]/.test(BUFFER);
  const log = new Log($("#log"), 1000, $("#newMsg"));
  let myNick = "", buffer = BUFFER;
  const users = new Map();
  const input = $("#msgInput");
  makeInput($("#inputForm"), input, log, () => buffer);

  $("#membersBox").hidden = !isChannel;
  $("#topicBar").hidden = !isChannel;
  if (!isChannel) $("#topicBar").hidden = true;

  // ---- members
  const rank = u => { const i = u.modes ? RANKS.indexOf(u.modes[0]) : -1; return i < 0 ? 9 : i; };
  let paintQueued = false;
  function paintMembers() {
    if (paintQueued) return;
    paintQueued = true;
    requestAnimationFrame(() => {
      paintQueued = false;
      const list = [...users.values()].sort((a, b) => rank(a) - rank(b) || (fold(a.nick) < fold(b.nick) ? -1 : 1));
      const box = $("#memberList");
      const frag = document.createDocumentFragment();
      for (const u of list) {
        const row = document.createElement("div");
        row.className = "mem r" + rank(u) + (u.away ? " away" : "");
        row.title = u.modes ? SYMBOL_NAMES[u.modes[0]] + (u.away ? " — absent" : "") : (u.away ? "absent" : "");
        const sym = document.createElement("span"); sym.className = "sym"; sym.textContent = u.modes ? u.modes[0] : "";
        const nm = document.createElement("span"); nm.className = "nm"; nm.textContent = u.nick;
        row.append(sym, nm);
        row.dataset.nick = u.nick;
        frag.appendChild(row);
      }
      box.textContent = "";
      box.appendChild(frag);
      $("#memCount").textContent = `[${list.length}]`;
    });
  }
  $("#memberList").addEventListener("dblclick", e => {
    const row = e.target.closest(".mem");
    if (row) invoke("send_input", { server: SERVER, buffer, text: "/query " + row.dataset.nick }).catch(err => toast(String(err)));
  });
  $("#memberList").addEventListener("contextmenu", e => {
    const row = e.target.closest(".mem");
    if (!row) return;
    e.preventDefault();
    const nick = row.dataset.nick;
    const say = text => invoke("send_input", { server: SERVER, buffer, text }).catch(err => toast(String(err)));
    openMenu(e.clientX, e.clientY, nick, [
      ["Message privé", () => say("/query " + nick)],
      ["Informations (whois)", () => say("/whois " + nick)],
      null,
      ["Donner la parole (+v)", () => say("/voice " + nick)],
      ["Retirer la parole (−v)", () => say("/devoice " + nick)],
      ["Nommer opérateur (+o)", () => say("/op " + nick)],
      ["Retirer opérateur (−o)", () => say("/deop " + nick)],
      null,
      ["Expulser", () => say("/kick " + nick)],
      ["Bannir", () => say("/ban " + nick)],
    ]);
  });

  // ---- topic
  function setTopic(t) {
    $("#topicText").textContent = t || "(pas de sujet)";
    $("#topicText").style.opacity = t ? 1 : 0.55;
    $("#topicText").title = t;
  }
  $("#topicBar").addEventListener("dblclick", () => {
    const bar = $("#topicBar"), span = $("#topicText");
    if (bar.querySelector("input")) return;
    const inp = document.createElement("input");
    inp.className = "text-input"; inp.maxLength = 400;
    inp.value = span.title || "";
    span.hidden = true; bar.appendChild(inp); inp.focus();
    const done = send => {
      if (send) invoke("send_input", { server: SERVER, buffer, text: inp.value.trim() ? "/topic " + inp.value.trim() : "/quote TOPIC " + buffer + " :" }).catch(err => toast(String(err)));
      inp.remove(); span.hidden = false;
    };
    inp.addEventListener("keydown", e => { if (e.key === "Enter") done(true); else if (e.key === "Escape") done(false); });
    inp.addEventListener("blur", () => { if (inp.isConnected) done(false); });
  });

  // ---- unread lines, shown in the title bar while the window is not the active one
  let unread = 0;
  const baseTitle = document.title;
  function showUnread() {
    const t = unread ? `${baseTitle} (${unread})` : baseTitle;
    for (const el of document.querySelectorAll("[data-title]")) el.textContent = t;
  }
  const readAll = () => { unread = 0; showUnread(); };
  window.addEventListener("focus", readAll);
  listen("active", e => { if (e.payload) readAll(); });

  // ---- pasting several lines asks first (a classic way to flood a channel by accident)
  let pasted = [];
  const hidePaste = () => { pasted = []; $("#pasteBar").hidden = true; };
  input.addEventListener("paste", ev => {
    const lines = (ev.clipboardData.getData("text") || "").split(/\r?\n/).filter(l => l.trim() !== "");
    if (lines.length < 2) return;
    ev.preventDefault();
    pasted = lines;
    $("#pasteBar").hidden = false;
    const tooMany = lines.length > 50;
    $("#pasteText").textContent = tooMany ? `${lines.length} lignes : trop pour un seul collage (50 au plus)` : `Envoyer ${lines.length} lignes d'un coup ?`;
    $("#pasteSend").hidden = tooMany;
  });
  $("#pasteCancel").addEventListener("click", () => { hidePaste(); input.focus(); });
  $("#pasteSend").addEventListener("click", async () => {
    const lines = pasted;
    hidePaste();
    for (const l of lines) {
      // A pasted line that starts with a slash is text, never a command.
      const text = l.startsWith("/") ? "/" + l : l;
      try { await invoke("send_input", { server: SERVER, buffer, text }); } catch (e) { toast(String(e)); break; }
    }
    input.focus();
  });

  // ---- connection banner
  function banner(st, text) {
    const b = $("#chatBanner");
    if (st === "connected") { b.hidden = true; return; }
    b.hidden = false;
    $("#chatBannerText").textContent = (STATUS_TEXT[st] || st) + (text ? " — " + text : "");
  }

  function setNick(n) { myNick = n; $("#myNick").textContent = n || "…"; }

  const sameBuffer = b => fold(b) === fold(buffer);
  const sub = subscribe(p => {
    const ev = p.event;
    switch (ev.kind) {
      case "line":
        if (sameBuffer(ev.line.buffer)) {
          log.add(ev.line);
          if (!ev.line.own && !document.hasFocus() && ["msg", "action", "notice"].includes(ev.line.kind)) { unread++; showUnread(); }
        }
        break;
      case "users": if (sameBuffer(ev.buffer)) { users.clear(); for (const u of ev.users) users.set(fold(u.nick), u); paintMembers(); } break;
      case "user_set": if (sameBuffer(ev.buffer)) { users.set(fold(ev.user.nick), ev.user); paintMembers(); } break;
      case "user_del": if (sameBuffer(ev.buffer)) { users.delete(fold(ev.nick)); paintMembers(); } break;
      case "topic": if (sameBuffer(ev.buffer)) setTopic(ev.topic); break;
      case "joined": if (sameBuffer(ev.buffer)) { users.clear(); paintMembers(); } break;
      case "me": setNick(ev.nick); break;
      case "status": banner(ev.state, ev.text); break;
    }
  });

  // ---- Tab completion of nicks
  let comp = null;
  input.addEventListener("keydown", ev => {
    if (ev.key !== "Tab") { if (!["Shift", "Control", "Alt"].includes(ev.key)) comp = null; return; }
    ev.preventDefault();
    const v = input.value, pos = input.selectionStart;
    if (!comp) {
      const m = /(\S*)$/.exec(v.slice(0, pos));
      const prefix = m ? m[1] : "";
      const names = [...users.values()].map(u => u.nick).filter(n => fold(n).startsWith(fold(prefix)) && fold(n) !== fold(myNick))
        .sort((a, b) => (fold(a) < fold(b) ? -1 : 1));
      if (!names.length) return;
      comp = { names, i: -1, start: pos - prefix.length, tail: v.slice(pos), atStart: pos - prefix.length === 0 };
    }
    comp.i = (comp.i + (ev.shiftKey ? -1 : 1) + comp.names.length) % comp.names.length;
    const word = comp.names[comp.i] + (comp.atStart ? ": " : " ");
    const head = v.slice(0, comp.start);
    input.value = head + word + comp.tail;
    input.setSelectionRange((head + word).length, (head + word).length);
  });

  (async () => {
    const snap = await call("buffer_snapshot", { server: SERVER, buffer: BUFFER });
    if (!snap) return;
    applySettings(snap.settings);
    log.limit = snap.settings.scrollback;
    buffer = snap.buffer.name || BUFFER;
    setNick(snap.server.nick);
    setTopic(snap.buffer.topic);
    for (const u of snap.buffer.users) users.set(fold(u.nick), u);
    paintMembers();
    banner(snap.server.status, snap.server.status_text);
    // The end of the conversation as logged earlier, above what this session holds.
    const first = snap.buffer.lines.length ? snap.buffer.lines[0].time : Date.now();
    const past = await call("read_backlog", { server: SERVER, buffer: buffer, before: Math.floor(first / 1000) * 1000 });
    if (past && past.length) {
      log.addAll(past.map(l => ({ ...l, past: true })));
      const sep = document.createElement("div");
      sep.className = "ln sep";
      sep.textContent = "— historique enregistré —";
      log.el.appendChild(sep);
    }
    log.addAll(snap.buffer.lines);
    log.toEnd();
    sub.ready(snap.seq);
    input.focus();
  })();
  window.addEventListener("focus", () => input.focus());
  window.addEventListener("keydown", ev => {
    // Typing anywhere in the window goes to the input line (Ctrl+C and friends excepted).
    if (ev.ctrlKey || ev.altKey || ev.metaKey || ev.key.length !== 1) return;
    if (document.activeElement && document.activeElement.matches("input, textarea")) return;
    input.focus();
  });
}

// ================================================================ whois
function initWhois() {
  const form = $("#whoisForm");
  const nickOf = BUFFER;
  const dur = s => {
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
    return h ? `${h} h ${pad2(m)} min` : m ? `${m} min ${pad2(s % 60)} s` : `${s} s`;
  };
  function show(info) {
    form.textContent = "";
    const rows = [
      ["Pseudo", info.nick], ["Identifiant", `${info.user}@${info.host}`], ["Nom réel", info.realname],
      ["Serveur", info.server ? `${info.server}${info.server_info ? " — " + info.server_info : ""}` : ""],
      ["Inactif", info.idle != null ? dur(info.idle) : ""],
      ["Connecté depuis", info.signon ? new Date(info.signon * 1000).toLocaleString("fr-FR") : ""],
      ["Compte", info.account], ["Salons", info.channels],
      ["Absent", info.away, true],
      ["Sécurité", info.secure ? "connexion chiffrée" : ""], ["Statut", info.operator ? "opérateur IRC" : ""],
    ];
    for (const [k, v, warn] of rows) {
      if (!v) continue;
      const l = document.createElement("span"); l.className = "lbl"; l.textContent = k;
      const d = document.createElement("span"); d.className = "v" + (warn ? " warn" : ""); d.textContent = v;
      form.append(l, d);
    }
  }
  listen("irc", e => { const p = e.payload; if (p.server === SERVER && p.event.kind === "whois" && fold(p.event.info.nick) === fold(nickOf)) show(p.event.info); });
  call("get_whois", { server: SERVER, nick: nickOf }).then(i => { if (i) show(i); });
  $("#whoisClose").addEventListener("click", () => invoke("win_close"));
  $("#whoisQuery").addEventListener("click", async () => { await call("send_input", { server: SERVER, buffer: "", text: "/query " + nickOf }); invoke("win_close"); });
  document.addEventListener("keydown", e => { if (e.key === "Escape") invoke("win_close"); });
}

// ================================================================ channel list
function initList() {
  const f = id => $("#" + id);
  const PAGE = 200;
  let shown = 0, sel = "", timer = 0, token = 0;
  async function load(append) {
    const me = ++token;
    const off = append ? shown : 0;
    const page = await call("get_channel_list", { server: SERVER, filter: f("lFilter").value, minUsers: +f("lMin").value || 0, byName: f("lSort").value === "name", offset: off, limit: PAGE });
    if (!page || me !== token) return;
    const box = f("chanList");
    if (!append) { box.textContent = ""; shown = 0; }
    if (!page.entries.length && !append) {
      const e = document.createElement("div"); e.className = "empty";
      e.textContent = page.loading ? "Réception de la liste…" : page.total ? "Aucun salon ne correspond" : "Liste vide : cliquez sur Actualiser";
      box.appendChild(e);
    }
    box.querySelector(".more")?.remove();
    const frag = document.createDocumentFragment();
    for (const en of page.entries) {
      const row = document.createElement("div");
      row.className = "chan-row" + (en.name === sel ? " sel" : "");
      row.dataset.name = en.name;
      const n = document.createElement("span"); n.className = "n"; n.textContent = en.name;
      const u = document.createElement("span"); u.className = "u"; u.textContent = String(en.users);
      const t = document.createElement("span"); t.className = "t"; t.title = en.topic;
      renderIrc(t, en.topic);
      row.append(n, u, t);
      frag.appendChild(row);
    }
    box.appendChild(frag);
    shown = off + page.entries.length;
    if (shown < page.matched) {
      const more = document.createElement("button");
      more.className = "ghost-btn more"; more.style.margin = "6px"; more.textContent = `Afficher la suite (${page.matched - shown} restants)`;
      more.addEventListener("click", () => load(true));
      box.appendChild(more);
    }
    const st = f("lStatus");
    st.classList.toggle("busy", page.loading); st.classList.toggle("on", !page.loading && page.total > 0);
    f("lStatusText").textContent = page.loading ? "Réception de la liste…" : `${page.matched} salon${page.matched > 1 ? "s" : ""} sur ${page.total}`;
  }
  const later = () => { clearTimeout(timer); timer = setTimeout(() => load(false), 200); };
  f("lFilter").addEventListener("input", later);
  f("lMin").addEventListener("input", later);
  f("lSort").addEventListener("change", () => load(false));
  f("listBar").addEventListener("submit", e => { e.preventDefault(); load(false); });
  f("lRefresh").addEventListener("click", () => call("send_input", { server: SERVER, buffer: "", text: "/list" }));
  f("chanList").addEventListener("click", e => {
    const row = e.target.closest(".chan-row");
    if (!row) return;
    sel = row.dataset.name;
    for (const r of f("chanList").children) r.classList.toggle("sel", r === row);
  });
  f("chanList").addEventListener("dblclick", async e => {
    const row = e.target.closest(".chan-row");
    if (row) { if (await call("send_input", { server: SERVER, buffer: "", text: "/join " + row.dataset.name }) !== null) toast("Demande envoyée : " + row.dataset.name, "ok"); }
  });
  listen("irc", e => {
    const p = e.payload;
    if (p.server !== SERVER) return;
    if (p.event.kind === "list_start") { f("lStatus").classList.add("busy"); f("lStatusText").textContent = "Réception de la liste…"; f("chanList").textContent = ""; }
    else if (p.event.kind === "list_ready") load(false);
  });
  (async () => {
    await load(false);
    const page = await call("get_channel_list", { server: SERVER, filter: "", minUsers: 0, byName: false, offset: 0, limit: 1 });
    // Opened from the menu with nothing received yet: ask for the list.
    if (page && !page.total && !page.loading) call("send_input", { server: SERVER, buffer: "", text: "/list" });
  })();
}

// ---------------------------------------------------------------- context menu
function openMenu(x, y, title, items) {
  const m = $("#ctxMenu");
  m.textContent = "";
  const t = document.createElement("div"); t.className = "ctx-title"; t.textContent = title; m.appendChild(t);
  for (const it of items) {
    if (!it) { const s = document.createElement("div"); s.className = "ctx-sep"; m.appendChild(s); continue; }
    const row = document.createElement("div"); row.className = "ctx-item"; row.textContent = it[0];
    row.addEventListener("click", () => { closeMenu(); it[1](); });
    m.appendChild(row);
  }
  m.classList.add("show");
  const r = m.getBoundingClientRect();
  m.style.left = Math.max(2, Math.min(x, innerWidth - r.width - 2)) + "px";
  m.style.top = Math.max(2, Math.min(y, innerHeight - r.height - 2)) + "px";
}
function closeMenu() { $("#ctxMenu").classList.remove("show"); }
document.addEventListener("pointerdown", e => { if (!e.target.closest("#ctxMenu")) closeMenu(); });
window.addEventListener("blur", closeMenu);

// ---------------------------------------------------------------- start
({ launcher: initLauncher, servers: initServers, settings: initSettings, about: initAbout, join: initJoin, console: initConsole, chat: initChat, whois: initWhois, list: initList }[PANEL] || (() => {}))();
