"use strict";
// Messages pages. One page (index.html), opened as ?w=<kind>[&s=<server>&b=<buffer>&t=<title>] by the
// application: launcher, servers (address book), settings, about, join, console, chat.
// Everything that comes from the network is put on the page with textContent / text nodes, never
// as HTML: a message can not inject markup.

const $ = s => document.querySelector(s);
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
// Scoped to this window: a listener of the whole application would also get what is sent to the other windows.
const thisWindow = T.webviewWindow.getCurrentWebviewWindow();
const listen = (ev, cb) => thisWindow.listen(ev, cb);

const params = new URLSearchParams(location.search);
const PANEL = params.get("w") || "all";
const SERVER = params.get("s") || "";
const BUFFER = params.get("b") || "";
document.documentElement.dataset.panel = PANEL;
document.title = params.get("t") || "Messages";
for (const el of document.querySelectorAll("[data-title]")) el.textContent = document.title;

// Theme: an accent ramp chosen in the settings (see themes.css). The window is opened with its
// theme in the address, so it never shows the wrong one first; a change reaches open windows as an event.
function applyTheme(name) {
  if (name && name !== "cdx") document.documentElement.dataset.theme = name;
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
// The frame (title bar, buttons, grip, active / shaded / theme events) is cdx-frame.js, shared by the whole CDX suite; it talks to
// the application through cdx-bridge.js (the "tauri" transport: win_drag, win_shade, win_zoom, win_close, win_ready).

// The hatched button of the title bar: the commands of the window.
function windowMenu(btn) {
  const r = btn.getBoundingClientRect();
  const items = [["Enrouler la fenêtre", () => invoke("win_shade")]];
  if (document.querySelector(".log")) items.push(["Effacer le texte", () => document.querySelectorAll(".log").forEach(l => { l.textContent = ""; })]);
  items.push(null, ["Fermer", () => invoke("win_close")]);
  CdxFrame.menu(items, r.left, r.bottom + 2, btn.closest(".tb").querySelector(".tb-title").textContent);
}
function initChrome() {
  document.addEventListener("cdx:menu", e => windowMenu(e.target.closest(".tb-btn")));
  CdxFrame.init();   // also tells the application that the window is ready (win_ready)
  listen("settings", e => applySettings(e.payload));
}
initChrome();

// ---------------------------------------------------------------- tabs
// Windows merged by dragging one title bar onto another form a group: each window of the group draws the same strip
// of tabs under its title bar, as told by the application ("tabs"), and clicking a tab brings that window forward.
const TAB_WINDOWS = { servers: "serversWin", settings: "settingsWin", console: "consoleWin", list: "listWin", chat: "chatWin" };
function initTabs() {
  const win = document.getElementById(TAB_WINDOWS[PANEL] || "");
  if (!win) return;
  const bar = document.createElement("div");
  bar.className = "tabs";
  bar.hidden = true;
  win.querySelector(".tb").after(bar);
  listen("tabs", e => {
    if (e.payload.me !== thisWindow.label) return;
    bar.textContent = "";
    bar.hidden = e.payload.tabs.length < 2;
    for (const t of e.payload.tabs) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "tab" + (t.active ? " on" : "");
      b.textContent = t.title + (t.unread ? ` (${t.unread})` : "");
      b.title = t.title;
      b.addEventListener("click", () => { if (!t.active) invoke("tab_activate", { label: t.label }); });
      b.addEventListener("auxclick", ev => { if (ev.button === 1) invoke("tab_close", { label: t.label }); });
      b.addEventListener("contextmenu", ev => {
        ev.preventDefault();
        CdxFrame.menu([
          ["Détacher l'onglet", () => invoke("tab_detach", { label: t.label })],
          ["Fermer l'onglet", () => invoke("tab_close", { label: t.label })],
        ], ev.clientX, ev.clientY, t.title);
      });
      bar.appendChild(b);
    }
  });
  // the window that a dragged one would join lights up
  listen("droptarget", e => document.body.classList.toggle("droptarget", !!e.payload));
  invoke("win_ready"); // asks again, now that the strip listens
}
initTabs();
// Ctrl+K: the quick switcher, from any window (pressed again inside it, it closes).
document.addEventListener("keydown", e => {
  if (!((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k")) return;
  e.preventDefault();
  if (PANEL === "switcher") invoke("win_close");
  else invoke("open_window", { kind: "switcher" }).catch(err => toast(String(err)));
});
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
      if (st.b) sp.style.textShadow = "1px 0 0 currentColor";
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

const NICK_COLORS = ["#ff5a4a", "#faa320", "#ffe090", "#86a6ff", "#ff5cf0", "#ffa6a6", "#c8a6ff", "#ffd24a", "#ff8a3c", "#e0c0ff"];
function nickColor(n) {
  let h = 0;
  for (const c of fold(n)) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return NICK_COLORS[h % NICK_COLORS.length];
}

const pad2 = n => String(n).padStart(2, "0");
function stamp(ms) { const d = new Date(ms); return pad2(d.getHours()) + ":" + pad2(d.getMinutes()); }

// The messages seen with an identifier, so that an answer can quote what it answers.
const MSGS = new Map();
function rememberMsg(l) {
  if (!l.msgid) return;
  MSGS.set(l.msgid, { nick: l.nick, text: l.text });
  if (MSGS.size > 4000) MSGS.delete(MSGS.keys().next().value);
}
const stripFormat = s => s.replace(/\x03(?:\d{1,2}(?:,\d{1,2})?)?|[\x02\x1d\x1f\x1e\x11\x16\x0f]/g, "");
const shorten = (s, n) => (s.length > n ? s.slice(0, n - 1) + "…" : s);

// One line of a window, as a <div>. `replyable`: show the button that answers it.
function renderLine(line, replyable) {
  const div = document.createElement("div");
  div.className = "ln " + line.kind + (line.own ? " own" : "") + (line.highlight && !line.own ? " hl" : "") + (line.past ? " past" : "");
  if (line.reply_to) {
    const q = MSGS.get(line.reply_to);
    const rq = document.createElement("span");
    rq.className = "rq";
    rq.textContent = q ? `↪ ${q.nick} : ${shorten(stripFormat(q.text), 90)}` : "↪ en réponse à un message plus ancien";
    div.appendChild(rq);
  }
  if (replyable && line.msgid && !line.own && ["msg", "action"].includes(line.kind)) {
    div.dataset.msgid = line.msgid; div.dataset.nick = line.nick; div.dataset.text = line.text;
    const rp = document.createElement("button");
    rp.type = "button"; rp.className = "rp"; rp.textContent = "↩ Répondre"; rp.title = "Répondre à ce message";
    div.appendChild(rp);
  }
  if (line.kind === "msg") div.style.setProperty("--nc", nickColor(line.nick));
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
      nk.style.setProperty("--nc", nickColor(line.nick));
      nk.dataset.av = Array.from(line.nick.replace(/^[^\p{L}\p{N}]+/u, ""))[0]?.toUpperCase() || "?";
      // the brackets are spans of their own so that a presentation without them (see feed-* in messages.css) can hide them
      const br = c => { const b = document.createElement("span"); b.className = "br"; b.textContent = c; return b; };
      nk.append(br("<"), line.nick, br(">"));
      div.appendChild(nk);
      renderIrc(tx, line.text);
      break;
    }
    case "action": {
      tx.appendChild(document.createTextNode("*** "));
      const who = document.createElement("span");
      who.className = "nk who";
      who.style.setProperty("--nc", nickColor(line.nick));
      who.textContent = line.nick;
      tx.append(who, " ");
      renderIrc(tx, line.text);
      break;
    }
    case "notice": tx.appendChild(document.createTextNode(line.nick ? "-" + line.nick + "- " : "")); renderIrc(tx, line.text); break;
    case "error": tx.appendChild(document.createTextNode("!! ")); renderIrc(tx, line.text); break;
    case "motd": renderIrc(tx, line.text); break;
    case "raw": renderIrc(tx, line.text); break;
    default: tx.appendChild(document.createTextNode(["join", "part", "quit", "nick"].includes(line.kind) ? "+++ " : "*** ")); renderIrc(tx, line.text);
  }
  div.appendChild(tx);
  if (line.media && line.media.kind === "image" && line.msgid) div.appendChild(imageBox(line));
  return div;
}

// An image in a line: the small preview that came with it (base64 of a JPEG, checked before use), then the whole image
// in the viewer when it is clicked. Nothing from outside is ever put in the page as markup.
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;
function imageBox(line) {
  const box = document.createElement("button");
  box.type = "button"; box.className = "thumb"; box.title = "Afficher l'image";
  const t = line.media.thumb;
  if (typeof t === "string" && t.length > 0 && t.length <= 90000 && BASE64.test(t)) {
    const img = document.createElement("img");
    img.alt = "image"; img.src = "data:image/jpeg;base64," + t;
    box.appendChild(img);
  } else {
    box.textContent = "Afficher l'image";
  }
  box.addEventListener("click", ev => { ev.stopPropagation(); openImage(line.msgid); });
  return box;
}

// ---------------------------------------------------------------- calls
// The window of a conversation is the microphone, the camera and the screen of its call: the microphone is cut into frames
// of 60 ms (960 samples of 16 kHz) and the camera into H.264 access units, sent to the application; what comes back is
// played with Web Audio and drawn from WebCodecs. Nothing of it is markup: bytes only.
const bytesToB64 = u8 => { let s = ""; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); };
const b64ToBytes = b => Uint8Array.from(atob(b), c => c.charCodeAt(0));
const H264 = { codec: "avc1.42E01F", width: 640, height: 480, framerate: 15, bitrate: 700000, avc: { format: "annexb" }, latencyMode: "realtime" };

function initCalls(getBuffer, getCaps) {
  const box = $("#callbox"), info = $("#callInfo"), remote = $("#callRemote"), local = $("#callLocal");
  let call = null, muted = false, media = null, ring = null, clock = 0, since = 0, fade = 0;
  const mine = c => fold(c.buffer) === fold(getBuffer());
  const send = (kind, bytes) => invoke("call_media", { server: SERVER, id: call.id, kind, data: bytesToB64(bytes) }).catch(() => {});

  function render() {
    clearTimeout(fade);
    if (!call) { box.hidden = true; return; }
    const incoming = call.state === "incoming", active = call.state === "active";
    box.hidden = false;
    $("#callAnswer").hidden = !incoming;
    $("#callAnswerVideo").hidden = !(incoming && call.video);
    $("#callMute").hidden = !(call.state === "calling" || active);
    $("#callHang").hidden = call.state === "ended";
    $("#callHang").textContent = incoming ? "Refuser" : "Raccrocher";
    $("#callMute").textContent = muted ? "Remettre le micro" : "Couper le micro";
    const kind = call.video ? "vidéo" : "audio";
    info.textContent = incoming ? `Appel ${kind} de ${call.text}`
      : call.state === "calling" ? `Appel ${kind} avec ${call.text} : connexion…`
      : active ? `Appel ${kind} avec ${call.text} · ${elapsed()}`
      : call.text;
  }
  const elapsed = () => { const s = Math.floor((Date.now() - since) / 1000); return String(Math.floor(s / 60)).padStart(2, "0") + ":" + String(s % 60).padStart(2, "0"); };

  // a ring while someone calls (a short two-tone beep, every 2 seconds)
  function ringStart() {
    if (ring) return;
    try {
      const ctx = new AudioContext();
      const beep = () => { for (const [f, t] of [[440, 0], [480, 0.25]]) { const o = ctx.createOscillator(), g = ctx.createGain(); o.frequency.value = f; g.gain.value = 0.05; o.connect(g); g.connect(ctx.destination); o.start(ctx.currentTime + t); o.stop(ctx.currentTime + t + 0.22); } };
      beep();
      ring = { ctx, timer: setInterval(beep, 2000) };
    } catch { ring = null; }
  }
  function ringStop() { if (ring) { clearInterval(ring.timer); ring.ctx.close().catch(() => {}); ring = null; } }

  // the microphone and, if wanted, the camera: returns whether video is on
  async function startMedia(wantVideo) {
    stopMedia();
    let video = wantVideo;
    if (video && !(typeof VideoEncoder !== "undefined" && (await VideoEncoder.isConfigSupported(H264).catch(() => ({}))).supported)) {
      toast("La vidéo n'est pas disponible sur cet ordinateur : appel audio seulement");
      video = false;
    }
    const stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 }, video: video ? { width: 640, height: 480, frameRate: 15 } : false });
    const ctx = new AudioContext({ sampleRate: 16000 });
    const m = { stream, ctx, next: 0, timers: [], gotKey: false };
    const src = ctx.createMediaStreamSource(stream);
    const proc = ctx.createScriptProcessor(1024, 1, 1);
    let frame = new Int16Array(960), n = 0;
    proc.onaudioprocess = e => {
      if (muted || !call) return;
      const x = e.inputBuffer.getChannelData(0);
      for (let i = 0; i < x.length; i++) {
        frame[n++] = Math.max(-32768, Math.min(32767, Math.round(x[i] * 32767)));
        if (n === 960) { send("audio", new Uint8Array(frame.buffer)); frame = new Int16Array(960); n = 0; }
      }
    };
    src.connect(proc); proc.connect(ctx.destination); // connected so that it runs; it writes nothing to the speakers
    m.proc = proc;
    if (video) {
      local.srcObject = new MediaStream(stream.getVideoTracks());
      await local.play().catch(() => {});
      local.hidden = false;
      const cv = document.createElement("canvas"); cv.width = H264.width; cv.height = H264.height;
      const g = cv.getContext("2d");
      const enc = new VideoEncoder({
        output: chunk => { if (!call) return; const d = new Uint8Array(chunk.byteLength); chunk.copyTo(d); send("video", d); },
        error: () => {},
      });
      enc.configure(H264);
      let k = 0;
      m.timers.push(setInterval(() => {
        if (!call || enc.encodeQueueSize > 4) return;
        g.drawImage(local, 0, 0, cv.width, cv.height);
        const f = new VideoFrame(cv, { timestamp: Math.round(performance.now() * 1000) });
        enc.encode(f, { keyFrame: k++ % 30 === 0 });
        f.close();
      }, 1000 / H264.framerate));
      m.encoder = enc;
    }
    media = m;
    return video;
  }
  function stopMedia() {
    if (!media) return;
    media.timers.forEach(clearInterval);
    try { media.proc.disconnect(); media.proc.onaudioprocess = null; } catch { /* already */ }
    media.stream.getTracks().forEach(t => t.stop());
    try { media.encoder && media.encoder.close(); } catch { /* already */ }
    try { media.decoder && media.decoder.close(); } catch { /* already */ }
    media.ctx.close().catch(() => {});
    media = null;
    local.hidden = true; local.srcObject = null;
    remote.hidden = true;
  }

  function playAudio(bytes) {
    if (!media || bytes.length < 2) return;
    const ctx = media.ctx, n = bytes.length >> 1, f = new Float32Array(n);
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    for (let i = 0; i < n; i++) f[i] = dv.getInt16(i * 2, true) / 32768;
    const buf = ctx.createBuffer(1, n, 16000);
    buf.copyToChannel(f, 0);
    const s = ctx.createBufferSource();
    s.buffer = buf; s.connect(ctx.destination);
    const now = ctx.currentTime;
    let t = Math.max(media.next, now + 0.12);
    if (t - now > 0.6) t = now + 0.12; // too far behind: catch up
    s.start(t);
    media.next = t + n / 16000;
  }
  function playVideo(bytes) {
    if (!media || typeof VideoDecoder === "undefined" || bytes.length < 6) return;
    const key = bytes[0] === 1;
    if (!media.decoder) {
      const g = remote.getContext("2d");
      media.decoder = new VideoDecoder({
        output: fr => { if (remote.width !== fr.displayWidth) { remote.width = fr.displayWidth; remote.height = fr.displayHeight; } g.drawImage(fr, 0, 0); fr.close(); remote.hidden = false; },
        error: () => { media && (media.gotKey = false); },
      });
      media.decoder.configure({ codec: H264.codec, optimizeForLatency: true });
    }
    if (!media.gotKey && !key) return; // a decoder starts on a key frame
    media.gotKey = true;
    try { media.decoder.decode(new EncodedVideoChunk({ type: key ? "key" : "delta", timestamp: Math.round(performance.now() * 1000), data: bytes.subarray(1) })); } catch { media.gotKey = false; }
  }
  listen("call-media", e => {
    const p = e.payload;
    if (!call || p.server !== SERVER || p.call !== call.id || typeof p.data !== "string") return;
    try { const b = b64ToBytes(p.data); if (p.kind === "audio") playAudio(b); else if (p.kind === "video") playVideo(b); } catch { /* a bad frame */ }
  });

  // what the application tells about the call
  function onCall(c) {
    if (!mine(c)) return;
    // news of another call than the one in progress does not concern it
    if (call && call.state !== "ended" && call.id && c.id && c.id !== call.id && c.state !== "incoming") return;
    clearInterval(clock);
    if (c.state === "ended") {
      const wasMine = call !== null;
      stopMedia(); ringStop();
      call = { ...c }; muted = false;
      render();
      if (wasMine || c.text) fade = setTimeout(() => { call = null; render(); }, 4000);
      return;
    }
    call = { ...c };
    if (c.state === "incoming") ringStart(); else ringStop();
    if (c.state === "active") { if (!since || !clock) since = Date.now(); clock = setInterval(render, 1000); }
    render();
  }

  async function begin(video) {
    const buffer = getBuffer();
    try { video = await startMedia(video); } catch (e) { toast("Micro ou caméra inaccessible : " + (e && e.message ? e.message : e)); stopMedia(); return null; }
    since = 0;
    return { buffer, video };
  }
  $("#btnCall").addEventListener("click", async () => { const r = await begin(false); if (r) { call = call || { id: "", state: "calling", video: false, text: "", buffer: r.buffer }; if (await call_ok("call_start", { server: SERVER, buffer: r.buffer, video: false })) { render(); } else stopMedia(); } });
  $("#btnVideoCall").addEventListener("click", async () => { const r = await begin(true); if (r) { call = call || { id: "", state: "calling", video: r.video, text: "", buffer: r.buffer }; if (await call_ok("call_start", { server: SERVER, buffer: r.buffer, video: r.video })) { render(); } else stopMedia(); } });
  async function call_ok(cmd, args) { const r = await invoke(cmd, args).then(() => true, err => { toast(String(err)); return false; }); if (!r) call = null; return r; }
  $("#callAnswer").addEventListener("click", async () => { const id = call.id; if (await begin(false) && await call_ok("call_answer", { server: SERVER, id, accept: true, video: false })) ringStop(); else if (call === null) render(); });
  $("#callAnswerVideo").addEventListener("click", async () => { const id = call.id; const r = await begin(true); if (r && await call_ok("call_answer", { server: SERVER, id, accept: true, video: r.video })) ringStop(); else if (call === null) render(); });
  $("#callHang").addEventListener("click", () => {
    if (!call) return;
    if (call.state === "incoming") invoke("call_answer", { server: SERVER, id: call.id, accept: false, video: false }).catch(e => toast(String(e)));
    else invoke("call_hangup", { server: SERVER, id: call.id }).catch(e => toast(String(e)));
    ringStop();
  });
  $("#callMute").addEventListener("click", () => {
    muted = !muted;
    if (call) invoke("call_mute", { server: SERVER, id: call.id, muted }).catch(() => {});
    render();
  });
  return { onCall, restore: c => { if (c) onCall(c); }, show: () => { $("#btnCall").hidden = !(getCaps().calls && !/^[#&+!]/.test(getBuffer())); $("#btnVideoCall").hidden = $("#btnCall").hidden; } };
}

async function openImage(msgid) {
  const viewer = $("#viewer");
  if (!viewer) return;
  const img = $("#viewerImg"), msg = $("#viewerMsg");
  img.removeAttribute("src");
  msg.textContent = "Chargement de l'image…";
  viewer.hidden = false;
  const url = await call("get_media", { server: SERVER, msgid });
  if (viewer.hidden) return; // closed while loading
  if (typeof url === "string" && /^data:image\/(jpeg|png|webp);base64,[A-Za-z0-9+/=]+$/.test(url)) { msg.textContent = ""; img.src = url; }
  else viewer.hidden = true;
}

const GROUP_MS = 5 * 60 * 1000;
const dayKey = ms => { const d = new Date(ms); return d.getFullYear() * 10000 + (d.getMonth() + 1) * 100 + d.getDate(); };
// "Aujourd'hui", "Hier", otherwise "Jeudi 29 octobre" (with the year when it is not this one).
function dayTitle(ms) {
  const el = document.createElement("div");
  el.className = "day";
  const now = new Date(), d = new Date(ms), key = dayKey(ms);
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  let t;
  if (key === dayKey(now.getTime())) t = "Aujourd'hui";
  else if (key === dayKey(yesterday.getTime())) t = "Hier";
  else {
    t = d.toLocaleDateString("fr-FR", { weekday: "long", day: "numeric", month: "long", ...(d.getFullYear() !== now.getFullYear() ? { year: "numeric" } : {}) });
    t = t.charAt(0).toUpperCase() + t.slice(1);
  }
  el.textContent = t;
  return el;
}

// A scrolling log that follows the end unless the reader scrolled up.
class Log {
  constructor(el, limit, newBtn) {
    this.el = el; this.limit = limit; this.newBtn = newBtn;
    this.stick = true;
    this.replyable = false;
    this.prev = null; // the last line added: who, when, to tell what continues it
    el.addEventListener("scroll", () => {
      this.stick = el.scrollTop + el.clientHeight >= el.scrollHeight - 6;
      if (this.stick && this.newBtn) this.newBtn.hidden = true;
    });
    if (newBtn) newBtn.addEventListener("click", () => { this.toEnd(); el.focus(); });
  }
  toEnd() { this.el.scrollTop = this.el.scrollHeight; this.stick = true; if (this.newBtn) this.newBtn.hidden = true; }
  add(line) { this.addAll([line]); }
  // What follows a line of the same author within GROUP_MS is its continuation: the presentations of a chat show the
  // name once, then the messages. A change of day puts a title above. (The classic transcript ignores both.)
  addAll(lines) {
    const frag = document.createDocumentFragment();
    for (const l of lines) {
      rememberMsg(l);
      const el = renderLine(l, this.replyable);
      const day = dayKey(l.time), p = this.prev;
      if (!p || p.day !== day) frag.appendChild(dayTitle(l.time));
      if (l.kind === "msg") {
        const cont = !!p && p.msg && p.nick === l.nick && p.day === day && l.time - p.time < GROUP_MS;
        el.classList.add(cont ? "cont" : "first", "last", "msg");
        if (cont) p.el.classList.remove("last");
      }
      this.prev = { el, day, msg: l.kind === "msg", nick: l.nick, time: l.time };
      frag.appendChild(el);
    }
    this.el.appendChild(frag);
    while (this.el.childElementCount > this.limit) this.el.firstElementChild.remove();
    // the first line left may have been the continuation of one that is gone: it shows its name now
    const top = this.el.firstElementChild;
    if (top && top.classList.contains("cont")) top.classList.replace("cont", "first");
    if (this.stick) this.el.scrollTop = this.el.scrollHeight;
    else if (this.newBtn) this.newBtn.hidden = false;
  }
  // Something that is not a line (a title) was put in the log: the next message begins a group.
  breakGroup() { if (this.prev) this.prev.msg = false; }
  clear() { this.el.textContent = ""; this.prev = null; this.stick = true; if (this.newBtn) this.newBtn.hidden = true; }
}

// Does the conversation of this window belong to a chat protocol (told by the account's capabilities)? Then the
// presentation chosen in the settings applies to it; IRC keeps the transcript.
let RICH_FEED = false;
const FEED_STYLES = ["classique", "aere", "blocs", "bulles", "cartes"];
const TEXT_SIZES = ["petite", "moyenne", "grande"];
const OWN_SIDES = ["auto", "gauche", "droite"];
let LAST_SETTINGS = null;
function applySettings(s) {
  LAST_SETTINGS = s;
  const feed = FEED_STYLES.includes(s.feed_style) ? s.feed_style : "blocs";
  const size = TEXT_SIZES.includes(s.text_size) ? s.text_size : "petite";
  for (const f of FEED_STYLES) document.body.classList.toggle("feed-" + f, RICH_FEED && f === feed);
  document.body.classList.toggle("rich-on", RICH_FEED && feed !== "classique");
  const side = OWN_SIDES.includes(s.own_side) ? s.own_side : "auto";
  document.body.classList.toggle("own-right", RICH_FEED && (side === "droite" || (side === "auto" && feed === "bulles")));
  for (const z of TEXT_SIZES) document.body.classList.toggle("sz-" + z, z === size);
  document.body.classList.toggle("nojoins", !s.show_joins);
  document.body.classList.toggle("nots", !s.show_timestamps);
  document.body.classList.toggle("smallmem", !!s.small_member_icons);
  document.body.classList.toggle("plainchat", !s.colored_chat);
}

// Events of the application. Events that arrive while a snapshot is being fetched wait for it,
// and those the snapshot already contains (seq <= snapshot.seq) are dropped.
function subscribe(handler) {
  const state = { ready: false, queue: [], seq: 0 };
  listen("chat", e => {
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
function makeInput(form, input, log, getBuffer, reply) {
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
    const r = reply ? reply.get() : null;
    if (r) reply.clear();
    try { await invoke("send_input", { server: SERVER, buffer: getBuffer(), text, replyTo: r ? r.msgid : null, replyNick: r ? r.nick : null }); }
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
    const keep = sel.value || localStorage.getItem("messages.server") || "";
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
    // the server's protocol decides which buttons make sense
    const here = ov.servers.find(s => s.id === current());
    $("#btnList").hidden = !!here && here.caps && !here.caps.channel_list;

    const connected = ov.servers.filter(s => stateOf(s.id).status === "connected").length;
    const busy = ov.servers.some(s => ["connecting", "registering", "reconnecting"].includes(stateOf(s.id).status));
    const pill = $("#statusPill");
    pill.classList.toggle("on", connected > 0);
    pill.classList.toggle("busy", busy);
    $("#statusText").textContent = ov.servers.length === 0 ? "Carnet vide"
      : busy ? "Connexion en cours…" : connected ? `${connected} serveur${connected > 1 ? "s" : ""} connecté${connected > 1 ? "s" : ""}` : "Aucune connexion";
    $("#cntServers").textContent = String(connected);
    $("#cntChannels").textContent = String(ov.servers.reduce((n, s) => n + stateOf(s.id).channels.length, 0));

    const list = $("#openList");
    list.textContent = "";
    const sv = ov.servers.length ? stateOf(current()) : { channels: [], queries: [] };
    const items = [...sv.channels.map(c => [c, "salon"]), ...(sv.queries || []).map(q => [q, "message"])];
    // what the user hid goes under a heading at the bottom of the list, that opens and closes
    const hid = new Set(((ov.hidden && ov.hidden[current()]) || []).map(fold));
    const shown = items.filter(([c]) => !hid.has(fold(c)));
    const hiddenItems = items.filter(([c]) => hid.has(fold(c)));
    if (!shown.length && !hiddenItems.length) {
      const e = document.createElement("div");
      e.className = "empty";
      e.textContent = ov.servers.length ? "Aucun salon ouvert" : "Ajoutez un serveur dans le carnet d'adresses";
      list.appendChild(e);
    }
    const act = new Map((sv.activity || []).map(a => [fold(a.name), a]));
    const server = current();
    const addRow = ([c, icon], isHidden) => {
      const a = act.get(fold(c));
      const row = document.createElement("div");
      row.className = "row-item" + (a ? " unread" : "") + (isHidden ? " hiddenrow" : "");
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
      const open = () => call("open_window", { kind: "chat", server, buffer: c });
      row.addEventListener("click", open);
      row.addEventListener("keydown", e => { if (e.key === "Enter") open(); });
      row.addEventListener("contextmenu", e => {
        e.preventDefault();
        CdxFrame.menu([
          ["Ouvrir", open],
          isHidden ? ["Réafficher dans la liste", () => setHidden(c, false)] : ["Masquer de la liste", () => setHidden(c, true)],
        ], e.clientX, e.clientY, c);
      });
      list.appendChild(row);
    };
    for (const it of shown) addRow(it, false);
    if (hiddenItems.length) {
      let open = false;
      try { open = localStorage.getItem("messages.hiddenOpen") === "1"; } catch { /* no storage: closed */ }
      const head = document.createElement("div");
      head.className = "row-item hidhead" + (hiddenItems.some(([c]) => act.has(fold(c))) ? " unread" : "");
      head.tabIndex = 0;
      const label = document.createElement("span");
      label.className = "grow";
      label.textContent = `${open ? "▼" : "▶"} Masquées (${hiddenItems.length})`;
      head.appendChild(label);
      const toggle = () => { try { localStorage.setItem("messages.hiddenOpen", open ? "0" : "1"); } catch { /* ignore */ } render(); };
      head.addEventListener("click", toggle);
      head.addEventListener("keydown", e => { if (e.key === "Enter") toggle(); });
      list.appendChild(head);
      if (open) for (const it of hiddenItems) addRow(it, true);
    }
  }
  async function setHidden(buffer, hide) {
    if (await call("set_hidden", { server: current(), buffer, hidden: hide }) === null) return;
    const o = await call("get_overview");
    if (o) { ov = o; render(); }
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
  sel.addEventListener("change", () => { localStorage.setItem("messages.server", sel.value); render(); });
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
  listen("chat", e => { if (["status", "joined", "parted", "me", "query", "listed"].includes(e.payload.event.kind)) refresh(); });
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
  listen("chat", e => { const k = e.payload.event.kind; if (k === "line" || k === "read") refreshStates(); });
  $("#btnList").addEventListener("click", () => call("open_window", { kind: "list", server: current() }));

  // A newer release on GitHub: announced here, installed on request.
  const bar = $("#updateBar"), btn = $("#updateBtn");
  function showUpdate(info) {
    bar.hidden = !info;
    if (info) { $("#updateText").textContent = `Version ${info.version} disponible (vous avez ${info.current})`; btn.disabled = false; btn.textContent = "Installer"; btn.hidden = false; }
  }
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    $("#updateText").textContent = "Téléchargement…";
    if (await call("install_update") === null) { /* the error comes as an event and a toast */ }
  });
  listen("update", e => {
    const p = e.payload;
    if (p.kind === "available") showUpdate(p.info);
    else if (p.kind === "progress") { bar.hidden = false; btn.hidden = true; $("#updateText").textContent = p.total ? `Téléchargement ${Math.floor(p.done * 100 / p.total)} %` : "Téléchargement…"; }
    else if (p.kind === "installing") { btn.hidden = true; $("#updateText").textContent = "Installation : Messages va redémarrer"; }
    else if (p.kind === "error") { toast(p.message); call("update_info").then(showUpdate); }
  });
  call("update_info").then(showUpdate);
  window.addEventListener("focus", refresh);
  refresh();
}

// ================================================================ address book
function initServers() {
  CdxSplit.init({ pane: $(".book .side"), side: "left", key: "carnet", min: 140, max: 360, minOther: 260 });   // la barre entre les serveurs et leur détail
  let ov = null, selected = "", fresh = false;
  const f = id => $("#" + id);
  const stateOf = id => (ov && ov.states[id]) || { status: "disconnected" };

  // The form shows what the protocol of the account needs (IRC: an address, a nick; WhatsApp: a name).
  function syncProto() {
    const p = f("fProto").value || "irc";
    for (const el of document.querySelectorAll("#serverForm [data-proto]")) el.hidden = el.dataset.proto !== p;
  }
  function protocolsOffered() {
    const list = (ov && ov.protocols) || [{ id: "irc", name: "IRC" }];
    const sel = f("fProto");
    if ([...sel.options].map(o => o.value).join() !== list.map(p => p.id).join()) {
      sel.textContent = "";
      for (const p of list) { const o = document.createElement("option"); o.value = p.id; o.textContent = p.name; sel.appendChild(o); }
    }
    f("fProto").hidden = f("lProto").hidden = list.length < 2;
  }
  function fill(entry) {
    protocolsOffered();
    f("fProto").value = entry.protocol || "irc";
    f("fWaName").value = entry.protocol === "whatsapp" ? entry.name || "" : "";
    f("fName").value = entry.protocol === "whatsapp" ? "" : entry.name || ""; f("fHost").value = entry.host || ""; f("fPort").value = entry.port || 6697;
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
    syncAuth(); syncTls(); syncProto();
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
    if (f("fProto").value === "whatsapp") {
      return { id: fresh ? "" : (base.id || ""), protocol: "whatsapp", name: f("fWaName").value.trim(), host: "", port: 6697, tls: true, auth: "none", autoconnect: f("fAuto").checked, autojoin: "" };
    }
    return {
      id: fresh ? "" : (base.id || ""), protocol: "irc", name: f("fName").value.trim(), host: f("fHost").value.trim(), port: +f("fPort").value || 0,
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
      const sub = document.createElement("span"); sub.className = "sub"; sub.textContent = s.protocol === "whatsapp" ? "WhatsApp" : `${s.host}:${s.port}`;
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
    fill({ port: 6697, tls: true, auth: "none", protocol: "irc" });
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
    if (keep || !f("fName").value && !f("fHost").value && !f("fWaName").value) {
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
  f("fProto").addEventListener("change", () => { syncProto(); (f("fProto").value === "whatsapp" ? f("fWaName") : f("fName")).focus(); });
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
  listen("chat", e => { if (e.payload.event.kind === "status") call("get_overview").then(o => { if (o) { ov = o; renderList(); } }); });
  refresh().then(() => { if (!ov.servers.length) startNew(); });
}

// ================================================================ settings
function initSettings() {
  const f = id => $("#" + id);
  const showPanel = () => { for (const g of document.querySelectorAll("#settingsForm .grp")) g.hidden = g.dataset.panel !== f("sPanel").value; };
  f("sPanel").addEventListener("change", showPanel);
  showPanel();
  async function load() {
    const ov = await call("get_overview");
    if (!ov) return;
    const s = ov.settings;
    f("sNick").value = s.nick; f("sAlt").value = s.alt_nick; f("sUser").value = s.username; f("sReal").value = s.realname;
    f("sQuit").value = s.quit_message; f("sPart").value = s.part_message;
    f("sFeed").value = s.feed_style; f("sSize").value = s.text_size; f("sSide").value = s.own_side;
    f("sStamps").checked = s.show_timestamps; f("sSmall").checked = s.small_member_icons; f("sColors").checked = s.colored_chat; f("sJoins").checked = s.show_joins; f("sScroll").value = s.scrollback;
    f("sNotif").checked = s.notifications; f("sQueries").checked = s.open_queries; f("sConsole").checked = s.open_console;
    f("sTheme").value = s.theme;
    f("sUpdates").checked = s.check_updates;
    f("sLog").checked = s.log_enabled; f("sLogDays").value = s.log_days;
    f("sIgnores").value = s.ignores.join("\n"); f("sWords").value = s.alert_words.join("\n");
  }
  async function save() {
    const s = {
      nick: f("sNick").value.trim(), alt_nick: f("sAlt").value.trim(), username: f("sUser").value.trim(), realname: f("sReal").value,
      quit_message: f("sQuit").value, part_message: f("sPart").value,
      feed_style: f("sFeed").value, text_size: f("sSize").value, own_side: f("sSide").value,
      show_timestamps: f("sStamps").checked, small_member_icons: f("sSmall").checked, colored_chat: f("sColors").checked, show_joins: f("sJoins").checked, scrollback: +f("sScroll").value || 0,
      notifications: f("sNotif").checked, open_queries: f("sQueries").checked, open_console: f("sConsole").checked,
      theme: f("sTheme").value,
      check_updates: f("sUpdates").checked,
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
  $("#checkUpdate").addEventListener("click", async () => {
    const line = $("#updateLine");
    line.textContent = "Recherche…";
    $("#checkUpdate").disabled = true;
    try {
      const u = await invoke("check_update");
      line.textContent = u ? `Version ${u.version} disponible : installez-la depuis le menu principal` : "Vous avez la dernière version";
    } catch (e) { line.textContent = ""; toast(String(e)); }
    $("#checkUpdate").disabled = false;
  });
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
    const last = localStorage.getItem("messages.server");
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
  // The code to link the account with the phone: drawn square by square, nothing but '0' and '1' comes from outside.
  function showPairing(code) {
    const box = $("#pairing");
    if (!code || !/^[01]+$/.test(code.modules) || code.modules.length !== code.size * code.size) { box.hidden = true; return; }
    const scale = 5, quiet = 4, side = (code.size + 2 * quiet) * scale;
    const c = $("#pairCanvas");
    c.width = c.height = side;
    const g = c.getContext("2d");
    g.fillStyle = "#fff"; g.fillRect(0, 0, side, side);
    g.fillStyle = "#000";
    for (let i = 0; i < code.modules.length; i++) {
      if (code.modules[i] === "1") g.fillRect((quiet + i % code.size) * scale, (quiet + Math.floor(i / code.size)) * scale, scale, scale);
    }
    box.hidden = false;
  }
  const sub = subscribe(p => {
    const ev = p.event;
    if (ev.kind === "pairing") showPairing(ev.code);
    else if (ev.kind === "paired") showPairing(null);
    else if (ev.kind === "status" && ev.state === "disconnected") showPairing(null);
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
    showPairing(snap.server.pairing);
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
  CdxSplit.init({ pane: $("#membersBox"), side: "right", key: "membres", min: 110, max: 420, minOther: 220 });   // la barre entre la conversation et les membres
  const isChannel = /^[#&+!]/.test(BUFFER);
  const log = new Log($("#log"), 1000, $("#newMsg"));
  let myNick = "", buffer = BUFFER;
  // What the protocol of this server can do (told by the snapshot): the menu and the topic bar follow it.
  let caps = { ranks: true, whois: true, moderation: true, channel_list: true, topic: true, images: false, calls: false, rich_feed: false };
  const calls = initCalls(() => buffer, () => caps);
  const users = new Map();
  const input = $("#msgInput");
  // The text box is one line high (the height of the buttons) and grows with the message, up to five lines; then it scrolls.
  const fitInput = () => { input.style.height = "0"; const lh = parseFloat(getComputedStyle(input).lineHeight) || 13; input.style.height = Math.min(input.scrollHeight + 2, 5 * lh + 8) + "px"; };
  input.addEventListener("input", fitInput);
  $("#inputForm").addEventListener("submit", () => setTimeout(fitInput, 0));
  new MutationObserver(fitInput).observe(document.body, { attributes: true, attributeFilter: ["class"] });   // the text size (Réglages > Affichage) changes the height of a line
  fitInput();
  // Answering a message: the bar above the input line shows what is answered (Esc cancels).
  let replying = null;
  const setReply = r => {
    replying = r;
    $("#replyBar").hidden = !r;
    if (r) $("#replyText").textContent = `Réponse à ${r.nick} : ${shorten(stripFormat(r.text), 80)}`;
    input.focus();
  };
  log.replyable = true;
  log.el.addEventListener("click", e => {
    const b = e.target.closest(".rp");
    const row = b && b.closest(".ln");
    if (row) setReply({ msgid: row.dataset.msgid, nick: row.dataset.nick, text: row.dataset.text });
  });
  $("#replyCancel").addEventListener("click", () => setReply(null));
  input.addEventListener("keydown", e => { if (e.key === "Escape" && replying) { e.preventDefault(); setReply(null); } });
  makeInput($("#inputForm"), input, log, () => buffer, { get: () => replying, clear: () => setReply(null) });
  // The input is a tall text box (it wraps): Enter sends, and a line break has no meaning on IRC.
  input.addEventListener("keydown", e => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); $("#inputForm").requestSubmit(); } });

  // the viewer of images
  $("#viewer").addEventListener("click", () => { $("#viewer").hidden = true; });
  document.addEventListener("keydown", e => { if (e.key === "Escape" && !$("#viewer").hidden) { $("#viewer").hidden = true; e.stopPropagation(); } }, true);

  // sending an image: the button, or an image pasted in the input
  async function sendImageFile(file) {
    if (!file || !["image/jpeg", "image/png", "image/webp"].includes(file.type)) { toast("Seuls les JPEG, PNG et WebP peuvent être envoyés"); return; }
    if (file.size > 10 * 1024 * 1024) { toast("Image trop grosse (10 Mo au plus)"); return; }
    const b64 = async blob => {
      const buf = new Uint8Array(await blob.arrayBuffer());
      let s = ""; for (let i = 0; i < buf.length; i += 0x8000) s += String.fromCharCode(...buf.subarray(i, i + 0x8000));
      return btoa(s);
    };
    // the small preview that travels with the image (the recipient sees it before the whole image arrives)
    let thumb = "";
    try {
      const bmp = await createImageBitmap(file);
      const k = Math.min(1, 100 / Math.max(bmp.width, bmp.height));
      const c = document.createElement("canvas");
      c.width = Math.max(1, Math.round(bmp.width * k)); c.height = Math.max(1, Math.round(bmp.height * k));
      c.getContext("2d").drawImage(bmp, 0, 0, c.width, c.height);
      thumb = await b64(await new Promise(r => c.toBlob(r, "image/jpeg", 0.6)));
    } catch { /* no preview: the image is still sent */ }
    const caption = input.value.trim();
    $("#btnImage").disabled = true;
    const ok = await call("send_image", { server: SERVER, buffer, data: await b64(file), thumb, caption });
    $("#btnImage").disabled = false;
    if (ok !== null && caption) input.value = "";
  }
  $("#btnImage").addEventListener("click", () => $("#fileImage").click());
  $("#fileImage").addEventListener("change", () => { const f = $("#fileImage").files[0]; $("#fileImage").value = ""; sendImageFile(f); });
  input.addEventListener("paste", e => {
    const f = [...(e.clipboardData ? e.clipboardData.files : [])].find(x => x.type.startsWith("image/"));
    if (f && caps.images) { e.preventDefault(); sendImageFile(f); }
  });

  $("#membersBox").hidden = !isChannel;
  $("#topicBar").hidden = !isChannel;

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
        row.dataset.ini = Array.from(u.nick.replace(/^[^\p{L}\p{N}]+/u, "") || u.nick)[0] || "?";
        row.style.setProperty("--nc", nickColor(u.nick));
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
    const items = [["Message privé", () => say("/query " + nick)]];
    if (caps.whois) items.push(["Informations (whois)", () => say("/whois " + nick)]);
    if (caps.ranks) items.push(null,
      ["Donner la parole (+v)", () => say("/voice " + nick)],
      ["Retirer la parole (−v)", () => say("/devoice " + nick)],
      ["Nommer opérateur (+o)", () => say("/op " + nick)],
      ["Retirer opérateur (−o)", () => say("/deop " + nick)]);
    if (caps.moderation) items.push(null,
      ["Expulser", () => say("/kick " + nick)],
      ["Bannir", () => say("/ban " + nick)]);
    CdxFrame.menu(items, e.clientX, e.clientY, nick);
  });

  // ---- topic
  function setTopic(t) {
    $("#topicText").textContent = t || "(pas de sujet)";
    $("#topicText").style.opacity = t ? 1 : 0.55;
    $("#topicText").title = t;
  }
  const editTopic = () => {
    const bar = $("#topicBar"), span = $("#topicText");
    if (bar.querySelector("input")) return;
    const inp = document.createElement("input");
    inp.className = "text-input"; inp.maxLength = 400;
    inp.value = span.title || "";
    span.hidden = true; span.after(inp); inp.focus();
    const done = send => {
      if (send) invoke("send_input", { server: SERVER, buffer, text: inp.value.trim() ? "/topic " + inp.value.trim() : "/quote TOPIC " + buffer + " :" }).catch(err => toast(String(err)));
      inp.remove(); span.hidden = false;
    };
    inp.addEventListener("keydown", e => { if (e.key === "Enter") done(true); else if (e.key === "Escape") done(false); });
    inp.addEventListener("blur", () => { if (inp.isConnected) done(false); });
  };
  $("#topicBtn").addEventListener("click", editTopic);
  $("#topicText").addEventListener("dblclick", editTopic);

  // ---- unread lines, shown in the title bar while the window is not the active one
  let unread = 0;
  const baseTitle = document.title;
  function showUnread() {
    const t = unread ? `${baseTitle} (${unread})` : baseTitle;
    for (const el of document.querySelectorAll("[data-title]")) el.textContent = t;
    invoke("tab_unread", { n: unread }).catch(() => {});
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
      case "call": calls.onCall(ev.call); break;
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
    caps = snap.caps || caps;
    RICH_FEED = !!caps.rich_feed;
    applySettings(snap.settings);
    if (!caps.topic) $("#topicBar").hidden = true;
    $("#btnImage").hidden = !caps.images;
    calls.show();
    calls.restore(snap.server.call);
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
      log.breakGroup();
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
  listen("chat", e => { const p = e.payload; if (p.server === SERVER && p.event.kind === "whois" && fold(p.event.info.nick) === fold(nickOf)) show(p.event.info); });
  call("get_whois", { server: SERVER, nick: nickOf }).then(i => { if (i) show(i); });
  $("#whoisClose").addEventListener("click", () => invoke("win_close"));
  $("#whoisQuery").addEventListener("click", async () => { await call("send_input", { server: SERVER, buffer: "", text: "/query " + nickOf }); invoke("win_close"); });
  document.addEventListener("keydown", e => { if (e.key === "Escape") invoke("win_close"); });
}

// ================================================================ channel list
function initList() {
  const f = id => $("#" + id);
  const PAGE = 200;
  let shown = 0, sel = "", timer = 0, token = 0, sortBy = "users";
  async function load(append) {
    const me = ++token;
    const off = append ? shown : 0;
    const page = await call("get_channel_list", { server: SERVER, filter: f("lFilter").value, minUsers: +f("lMin").value || 0, byName: sortBy === "name", offset: off, limit: PAGE });
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
  CdxColumns.init({ head: f("lHead"), cols: [181, 85, "1fr"], key: "salons" });   // colonnes redimensionnables
  const later = () => { clearTimeout(timer); timer = setTimeout(() => load(false), 200); };
  f("lFilter").addEventListener("input", later);
  f("lMin").addEventListener("input", later);
  f("lHead").addEventListener("click", e => {
    const h = e.target.closest(".lh[data-sort]");
    if (!h) return;
    sortBy = h.dataset.sort;
    for (const o of f("lHead").querySelectorAll(".lh[data-sort]")) o.classList.toggle("on", o === h);
    load(false);
  });
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
  listen("chat", e => {
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

// ================================================================ quick switcher (Ctrl+K)
function initSwitcher() {
  const input = $("#swInput"), box = $("#swList");
  let items = [], shown = [], sel = 0;
  const plain = s => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  const ACTIONS = [
    { icon: "carnet", label: "Carnet d'adresses", run: () => invoke("open_window", { kind: "servers" }) },
    { icon: "reglages", label: "Réglages", run: () => invoke("open_window", { kind: "settings" }) },
    { icon: "info", label: "À propos", run: () => invoke("open_window", { kind: "about" }) },
    { icon: "salon", label: "Rejoindre un salon…", run: () => invoke("open_window", { kind: "join" }) },
    { icon: "deconnecter", label: "Masquer dans la zone de notification", run: () => invoke("hide_all") },
    { icon: "quitter", label: "Quitter Messages", run: () => invoke("quit_app") },
  ].map(a => ({ ...a, kind: "action", key: plain(a.label) }));

  // Every word typed must be found, in the name or in the server's: a word at the start of a name
  // counts more than one in the middle. -1 = does not match.
  function score(q, text) {
    let total = 0;
    for (const w of q.split(/\s+/).filter(Boolean)) {
      const i = text.indexOf(w);
      if (i < 0) return -1;
      total += i === 0 ? 30 : /[\s#@\-_.]/.test(text[i - 1]) ? 20 : 10;
    }
    return total;
  }

  function compute() {
    const q = plain(input.value.trim());
    const out = [];
    for (const it of items) {
      const key = plain(it.buffer + " " + it.server_name + (it.kind === "console" ? " messages console" : ""));
      const s = q ? score(q, key) : 0;
      if (s < 0) continue;
      // What needs you comes first, then what you looked at last.
      const boost = (it.mentions ? 60 : 0) + (it.unread ? 30 : 0) + (it.recent != null ? Math.max(0, 25 - it.recent) : 0);
      out.push({ ...it, rank: s + boost });
    }
    if (q) for (const a of ACTIONS) { const s = score(q, a.key); if (s >= 0) out.push({ ...a, rank: s - 5 }); }
    else for (const a of ACTIONS) out.push({ ...a, rank: -10 });
    // # to join, @ to write: offered for each connected server.
    const typed = input.value.trim();
    if (/^[#&][^\s,]+$/.test(typed)) for (const s of servers()) out.push({ kind: "join", icon: "salon", label: `Rejoindre ${typed}`, sub: s.server_name, server: s.server, buffer: typed, rank: 200 });
    if (/^@[^\s,]+$/.test(typed)) for (const s of servers()) out.push({ kind: "query", icon: "message", label: `Écrire à ${typed.slice(1)}`, sub: s.server_name, server: s.server, buffer: typed.slice(1), rank: 200 });
    out.sort((a, b) => b.rank - a.rank || fold(a.label || a.buffer).localeCompare(fold(b.label || b.buffer)));
    return out.slice(0, 60);
  }
  const servers = () => { const seen = new Map(); for (const it of items) if (it.connected && !seen.has(it.server)) seen.set(it.server, it); return [...seen.values()]; };

  function paint() {
    shown = compute();
    if (sel >= shown.length) sel = Math.max(0, shown.length - 1);
    box.textContent = "";
    if (!shown.length) {
      const e = document.createElement("div"); e.className = "row-item empty-hint";
      e.textContent = "Rien ne correspond. # pour rejoindre un salon, @ pour écrire à quelqu'un.";
      box.appendChild(e);
    }
    shown.forEach((it, i) => {
      const row = document.createElement("div");
      row.className = "row-item" + (i === sel ? " sel" : "");
      row.setAttribute("role", "option");
      const ic = document.createElement("span"); ic.className = "pix ic";
      ic.innerHTML = pix(it.icon || (it.kind === "console" ? "console" : /^[#&+!]/.test(it.buffer) ? "salon" : "message"), 14);
      const name = document.createElement("span"); name.className = "grow";
      name.textContent = it.label || (it.kind === "console" ? "Messages du serveur" : it.buffer);
      const sub = it.sub != null ? it.sub : it.server_name;
      if (sub) { const s = document.createElement("span"); s.className = "sub"; s.textContent = sub; name.appendChild(s); }
      row.append(ic, name);
      const n = it.unread || 0;
      if (n) { const c = document.createElement("span"); c.className = "count" + (it.mentions ? " hot" : ""); c.textContent = n > 99 ? "99+" : String(n); row.appendChild(c); }
      row.addEventListener("click", () => go(it));
      row.addEventListener("mousemove", () => { if (sel !== i) { sel = i; mark(); } });
      box.appendChild(row);
    });
    const cur = box.children[sel];
    if (cur && cur.scrollIntoView) cur.scrollIntoView({ block: "nearest" });
  }
  const mark = () => [...box.children].forEach((r, i) => r.classList.toggle("sel", i === sel));

  async function go(it) {
    try {
      if (it.kind === "chat") await invoke("open_window", { kind: "chat", server: it.server, buffer: it.buffer });
      else if (it.kind === "console") await invoke("open_window", { kind: "console", server: it.server });
      else if (it.kind === "join") await invoke("send_input", { server: it.server, buffer: "", text: "/join " + it.buffer, replyTo: null, replyNick: null });
      else if (it.kind === "query") await invoke("send_input", { server: it.server, buffer: "", text: "/query " + it.buffer, replyTo: null, replyNick: null });
      else if (it.run) await it.run();
    } catch (e) { toast(String(e)); return; }
    invoke("win_close");
  }

  input.addEventListener("input", () => { sel = 0; paint(); });
  input.addEventListener("keydown", e => {
    if (e.key === "ArrowDown") { e.preventDefault(); sel = Math.min(shown.length - 1, sel + 1); mark(); box.children[sel]?.scrollIntoView({ block: "nearest" }); }
    else if (e.key === "ArrowUp") { e.preventDefault(); sel = Math.max(0, sel - 1); mark(); box.children[sel]?.scrollIntoView({ block: "nearest" }); }
    else if (e.key === "Enter") { e.preventDefault(); if (shown[sel]) go(shown[sel]); }
    else if (e.key === "Escape") { e.preventDefault(); invoke("win_close"); }
  });
  window.addEventListener("focus", () => input.focus());
  // A quick switcher is for one gesture: clicking elsewhere dismisses it (once it has had the focus).
  let hadFocus = document.hasFocus();
  let away = 0;
  window.addEventListener("focus", () => { hadFocus = true; clearTimeout(away); });
  window.addEventListener("blur", () => { if (hadFocus) away = setTimeout(() => { if (!document.hasFocus()) invoke("win_close"); }, 250); });
  call("switcher_items").then(list => { items = list || []; paint(); input.focus(); });
}

// ---------------------------------------------------------------- context menu

// ---------------------------------------------------------------- start
({ launcher: initLauncher, servers: initServers, settings: initSettings, about: initAbout, join: initJoin, console: initConsole, chat: initChat, whois: initWhois, list: initList, switcher: initSwitcher }[PANEL] || (() => {}))();
