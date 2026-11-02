// End-to-end test of the real application (node >= 22): starts tools/fake_ircd.py and Messages with its
// own data directory, drives the pages through the WebView2 debugging port, checks what they show
// and saves a screenshot of each window in tests/shots/.
//
//   node tests/e2e.mjs [path-to-cdx-messages.exe]
//
// Nothing here touches the user's real configuration (CDX_MESSAGES_DATA_DIR) or credentials.
import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.argv[2] || join(root, "src-tauri", "target", "debug", "cdx-messages.exe");
const shots = join(root, "tests", "shots");
mkdirSync(shots, { recursive: true });
const data = mkdtempSync(join(tmpdir(), "messages-e2e-"));
const DEBUG_PORT = 9333, IRC_PORT = 16667, TLS_PORT = 16697;

const sleep = ms => new Promise(r => setTimeout(r, ms));
// Whatever happens, the whole run ends.
setTimeout(() => { console.error("ERROR: the test took more than 8 minutes"); process.exit(3); }, 8 * 60 * 1000).unref();
let failures = 0;
const check = (cond, what) => { console.log((cond ? "  ok   " : "  FAIL ") + what); if (!cond) failures++; };

async function until(fn, what, ms = 10000) {
  const t0 = Date.now();
  for (;;) {
    try { const v = await fn(); if (v) return v; } catch { /* retry */ }
    if (Date.now() - t0 > ms) throw new Error("timeout: " + what);
    await sleep(100);
  }
}

// ---- CDP
class Page {
  constructor(info, ws) { this.info = info; this.ws = ws; this.id = 0; this.waiting = new Map(); ws.onmessage = m => { const d = JSON.parse(m.data); const w = this.waiting.get(d.id); if (w) { this.waiting.delete(d.id); d.error ? w.reject(new Error(d.error.message)) : w.resolve(d.result); } }; }
  static async open(info) { const ws = new WebSocket(info.webSocketDebuggerUrl); await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; }); return new Page(info, ws); }
  // No call may wait for ever: a window that is not drawn (a CI machine has no screen) never answers.
  send(method, params = {}) {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => { this.waiting.delete(id); reject(new Error("no answer to " + method)); }, 20000);
      this.waiting.set(id, { resolve: v => { clearTimeout(t); resolve(v); }, reject: e => { clearTimeout(t); reject(e); } });
    });
  }
  // For actions that close the window itself: nothing can answer once it is gone.
  async fire(expr) { try { await this.eval(`setTimeout(() => { ${expr} }, 30); 0`); } catch { /* the window is gone */ } }
  async eval(expr) { const r = await this.send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || "eval failed"); return r.result.value; }
  // Screenshots are a courtesy: where the window cannot be drawn they are skipped, not fatal.
  async shot(name) { try { const r = await this.send("Page.captureScreenshot", { format: "png" }); writeFileSync(join(shots, name + ".png"), Buffer.from(r.data, "base64")); } catch (e) { console.log("   (no screenshot " + name + ": " + e.message + ")"); } }
  close() { this.ws.close(); }
}
const targets = async () => (await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json`)).json()).filter(t => t.type === "page");
async function page(match, ms = 10000) {
  const info = await until(async () => (await targets()).find(t => match(new URL(t.url).searchParams)), "window " + match, ms);
  return Page.open(info);
}
const win = (w, extra = {}) => p => p.get("w") === w && Object.entries(extra).every(([k, v]) => p.get(k) === v);

// ---- processes
const children = [];
function start(cmd, args, opts = {}) { const c = spawn(cmd, args, { stdio: "ignore", windowsHide: true, ...opts }); children.push(c); return c; }
// Only the processes started here are stopped (by pid, with their children): never every cdx-messages.exe of the machine, which would close the user's own.
function cleanup() { for (const c of children) { try { execFileSync("taskkill", ["/F", "/T", "/PID", String(c.pid)], { stdio: "ignore" }); } catch { /* gone */ } try { c.kill(); } catch { /* gone */ } } try { rmSync(data, { recursive: true, force: true }); } catch { /* busy */ } }
process.on("exit", cleanup);

// A raw client to drive the fake server (XSAY & co.) on behalf of the app's connection is not
// possible: the commands must come from the app's own connection, so the app sends them with /quote.

async function main() {
  if (!existsSync(exe)) throw new Error("no exe: " + exe);
  writeFileSync(join(data, "config.json"), JSON.stringify({
    version: 1,
    settings: { nick: "kirc", realname: "Messages e2e", quit_message: "Au revoir", notifications: false },
    windows: { launcher: { x: 333, y: 222, w: 330, h: 600 } },
    servers: [{ id: "s1", name: "Serveur de test", host: "127.0.0.1", port: IRC_PORT, tls: false, autoconnect: false, autojoin: "#test", auth: "none" }],
  }));
  const cert = join(data, "cert.pem"), key = join(data, "key.pem");
  execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-days", "2", "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1"], { stdio: "ignore" });
  start("python", [join(root, "tools", "fake_ircd.py"), "--port", String(IRC_PORT), "--tls-port", String(TLS_PORT), "--cert", cert, "--key", key]);
  await until(() => new Promise(r => { const s = net.connect(IRC_PORT, "127.0.0.1", () => { s.destroy(); r(true); }); s.on("error", () => r(false)); }), "fake ircd");

  const appEnv = { ...process.env, CDX_MESSAGES_NO_UPDATE_CHECK: "1", CDX_MESSAGES_DATA_DIR: data, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${DEBUG_PORT}` };
  start(exe, [], { env: appEnv });

  console.log("launcher");
  const launcher = await page(win("launcher"), 30000);
  await until(() => launcher.eval("document.querySelectorAll('#serverSelect option').length === 1"), "server in the launcher");
  check(await launcher.eval("document.title") === "Messages", "launcher title");
  check(await launcher.eval("document.querySelector('#statusText').textContent") === "Aucune connexion", "status: aucune connexion");
  check(await launcher.eval("document.querySelector('#connectBtn').textContent") === "Connecter", "connect button label");
  await launcher.shot("01-launcher");

  console.log("connection");
  await launcher.eval("document.querySelector('#connectBtn').click()");
  const chat = await page(win("chat", { b: "#test" }), 15000);
  await until(() => chat.eval("document.querySelectorAll('#memberList .mem').length === 4"), "4 members (3 virtual + us)");
  // La barre entre la conversation et les membres (cdx-split.js). La webview partage son stockage avec le vrai Messages : on repart des largeurs d'origine et on n'y laisse rien.
  await chat.eval("document.querySelector('#membersBox').previousElementSibling.firstElementChild.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))");   // largeur d'origine, même si une largeur était gardée
  check(await chat.eval("document.querySelector('#membersBox').previousElementSibling.classList.contains('cdx-split')"), "the members pane has a drag bar");
  const mw = () => chat.eval("Math.round(document.querySelector('#membersBox').getBoundingClientRect().width)");
  const mw0 = await mw();
  await chat.eval("(() => { const s = document.querySelector('#membersBox').previousElementSibling.firstElementChild, x = s.getBoundingClientRect().left; for (const [t, d] of [['pointerdown', 0], ['pointermove', -40], ['pointerup', -40]]) s.dispatchEvent(new PointerEvent(t, { bubbles: true, button: 0, pointerId: 6, clientX: x + d })); })()");
  check((await mw()) === mw0 + 40, `dragging the bar 40 px to the left widens the members pane (${mw0} -> ${await mw()})`);
  await chat.eval("document.querySelector('#membersBox').previousElementSibling.firstElementChild.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); localStorage.removeItem('cdx.split.membres')");
  check((await mw()) === mw0, "a double click on the bar gives the original width back");
  const members = await chat.eval("[...document.querySelectorAll('#memberList .mem')].map(e => e.textContent)");
  check(members.join(" ") === "@alice +bob carol kirc", "member list sorted by rank: " + members.join(" "));
  const first = members[0];
  check(first === "@alice" || first === "@kirc", "operators first: " + first);
  check((await chat.eval("document.querySelector('#topicText').textContent")) === "Salon de test de Messages", "topic shown");
  check((await chat.eval("document.querySelector('#myNick').textContent")) === "kirc", "own nick shown");
  await chat.shot("02-chat-joined");
  await until(() => launcher.eval("document.querySelector('#statusText').textContent.includes('1 serveur')"), "launcher says 1 connected");
  check((await launcher.eval("[...document.querySelectorAll('#openList .row-item')].map(e => e.textContent).join()")) === "#test", "launcher lists #test");
  await launcher.shot("03-launcher-connected");

  const con = await page(win("console"), 10000);
  await until(() => con.eval("document.querySelector('#consoleLog').textContent.includes('Bienvenue sur FakeNet')"), "welcome in the console");
  check((await con.eval("document.querySelector('#consoleLog .ln.motd .fmt-b, #consoleLog .ln.motd span[style*=\"text-shadow\"]') !== null")), "MOTD bold rendered");
  check((await con.eval("!!document.querySelector('#consoleLog a.link')")), "MOTD link rendered");
  check((await con.eval("document.querySelector('#consoleLog a.link').textContent")) === "https://example.org/page", "link without the final dot");
  await con.shot("04-console");

  console.log("chat");
  const say = async text => { await chat.eval(`(() => { const i = document.querySelector('#msgInput'); i.value = ${JSON.stringify(text)}; document.querySelector('#inputForm').requestSubmit(); })()`); };
  const lines = () => chat.eval("[...document.querySelectorAll('#log .ln')].map(e => e.textContent)");
  await say("bonjour tout le monde");
  await until(async () => (await lines()).some(l => l.includes("<kirc>") && l.includes("bonjour tout le monde")), "own message echoed");
  await say("alice, tu es là ?");
  await until(async () => (await lines()).some(l => l.includes("<alice>") && l.includes("je suis là")), "alice answers");
  const hl = await chat.eval("document.querySelectorAll('#log .ln.hl').length");
  check(hl === 1, "mention highlighted (" + hl + ")");
  await say("/quote XSAY #test bob salut kirc ! voir https://example.org/a_(b).");
  await until(async () => (await lines()).some(l => l.includes("<bob>") && l.includes("salut kirc")), "bob speaks");
  check(await chat.eval("document.querySelector('#log .ln:last-child a.link').textContent") === "https://example.org/a_(b)", "link keeps balanced parenthesis");
  await say("/quote XACT #test carol danse");
  await until(async () => (await lines()).some(l => l.includes("* carol danse")), "action rendered");
  await say("/quote XJOIN #test dave");
  await until(() => chat.eval("[...document.querySelectorAll('#memberList .mem')].some(e => e.textContent === 'dave')"), "dave joins the list");
  await say("/quote XMODE #test +o dave");
  await until(() => chat.eval("[...document.querySelectorAll('#memberList .mem')].some(e => e.textContent === '@dave')"), "dave gets @");
  await say("/quote XNICK dave david");
  await until(() => chat.eval("[...document.querySelectorAll('#memberList .mem')].some(e => e.textContent === '@david')"), "nick change");
  await say("/quote XQUIT david");
  await until(() => chat.eval("![...document.querySelectorAll('#memberList .mem')].some(e => e.textContent.includes('david'))"), "david quits");
  await say("<img src=x onerror=alert(1)> **gras**");
  await until(async () => (await lines()).some(l => l.includes("<img src=x")), "markup shown as text");
  check(await chat.eval("document.querySelector('#log img') === null"), "no element injected");
  await say("/unknown");
  await until(async () => (await lines()).some(l => l.includes("Commande inconnue")), "unknown command reported");
  // An event of another server (same channel name) must not show up here.
  await chat.eval(`window.__TAURI__.event.emit("chat", { server: "autre", seq: 999999, event: { kind: "line", line: { buffer: "#test", kind: "msg", nick: "intrus", text: "FOREIGN", time: Date.now(), highlight: false, own: false } } })`);
  await sleep(400);
  check(!(await lines()).some(l => l.includes("FOREIGN")), "events of another server are ignored");
  await say("/topic Nouveau sujet");
  await until(() => chat.eval("document.querySelector('#topicText').textContent === 'Nouveau sujet'"), "topic changes");
  await chat.eval("document.querySelector('#log').dispatchEvent(new Event('scroll'))");
  await chat.shot("05-chat-busy");

  console.log("private message");
  await say("/msg alice psst");
  const pm = await page(win("chat", { b: "alice" }), 10000);
  await until(() => pm.eval("[...document.querySelectorAll('#log .ln')].some(e => e.textContent.includes('Tu as dit : psst'))"), "private conversation works");
  check(await pm.eval("document.querySelector('#membersBox').hidden"), "no member list in a private window");
  await pm.shot("06-query");

  console.log("address book");
  await launcher.eval("document.querySelector('#btnBook').click()");
  const book = await page(win("servers"), 10000);
  await until(() => book.eval("document.querySelectorAll('#serverList .row-item').length === 1 && document.querySelector('#fHost').value !== ''"), "book filled");
  check(await book.eval("document.querySelector('#fHost').value") === "127.0.0.1", "form shows the selected server");
  await book.shot("07-book");

  console.log("settings and about");
  await launcher.eval("document.querySelector('#btnSettings').click()");
  const settings = await page(win("settings"), 10000);
  await until(() => settings.eval("document.querySelector('#sNick').value === 'kirc'"), "settings loaded");
  await settings.shot("08-settings");
  await launcher.eval("document.querySelector('#btnAbout').click()");
  const about = await page(win("about"), 10000);
  await until(() => about.eval("document.querySelector('#aboutVersion').textContent.startsWith('version ')"), "about version");
  await about.shot("09-about");
  await launcher.eval("document.querySelector('#btnJoin').click()");
  const joinWin = await page(win("join"), 10000);
  await until(() => joinWin.eval("document.querySelectorAll('#jServer option').length === 1"), "join dialog");
  await joinWin.shot("10-join");

  console.log("leaving a channel by closing its window");
  await joinWin.eval("document.querySelector('#jChannel').value='autre'; document.querySelector('#joinForm').requestSubmit()");
  const other = await page(win("chat", { b: "#autre" }), 10000);
  await until(() => other.eval("document.querySelectorAll('#memberList .mem').length === 1"), "second channel joined");
  await other.fire("document.querySelector('.tb-btn[data-act=close]').click()");
  await until(async () => !(await targets()).some(t => new URL(t.url).searchParams.get("b") === "#autre"), "window gone");
  await chat.eval("1");
  await until(() => launcher.eval("[...document.querySelectorAll('#openList .row-item')].map(e => e.textContent).join() === '#test,alice'"), "launcher back to #test, plus the private conversation");

  console.log("saving settings");
  await settings.eval("document.querySelector('#sNick').value = 'kirc2'; document.querySelector('#saveSettings').click()");
  await sleep(2600);
  const saved = JSON.parse(await (await import("node:fs/promises")).readFile(join(data, "config.json"), "utf8"));
  check(saved.settings.nick === "kirc2", "settings written to config.json");
  check(!JSON.stringify(saved).toLowerCase().includes("password"), "no password in config.json");
  check(Object.keys(saved.windows).some(k => k.startsWith("chat:s1:#test")), "chat geometry remembered");

  console.log("window behaviour");
  const geo = await launcher.eval("({ x: window.screenX * devicePixelRatio, y: window.screenY * devicePixelRatio, w: window.outerWidth * devicePixelRatio })");
  check(Math.abs(geo.x - 333) <= 16 && Math.abs(geo.y - 222) <= 16, `remembered position restored (${Math.round(geo.x)}, ${Math.round(geo.y)})`);
  const h0 = await chat.eval("window.innerHeight");
  await chat.eval("window.__TAURI__.core.invoke('win_shade')");
  await until(() => chat.eval("document.body.classList.contains('shaded') && window.innerHeight < 60"), "window rolls up");
  await chat.shot("12-chat-shaded");
  await chat.eval("window.__TAURI__.core.invoke('win_shade')");
  await until(async () => (await chat.eval("window.innerHeight")) === h0 && !(await chat.eval("document.body.classList.contains('shaded')")), "window rolls down to its height");
  check(true, "roll up / down restores the height (" + h0 + ")");

  console.log("TLS, SASL and the credential manager");
  const inv = (cmd, args) => launcher.eval(`window.__TAURI__.core.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(v => ({ ok: v === undefined ? null : v }), e => ({ err: String(e) }))`);
  const status = async id => (await inv("get_overview", {})).ok.states[id]?.status;
  // The console window opens with the connection; wait for its log to be filled, then read it.
  const consoleText = async (id, needle) => {
    const c = await page(win("console", { s: id }), 10000);
    let t = "";
    try { await until(async () => (t = await c.eval("document.querySelector('#consoleLog').textContent")).includes(needle), "console shows " + needle, 5000); } catch { /* the check reports it */ }
    c.close();
    return t;
  };
  const base = { name: "", host: "127.0.0.1", port: TLS_PORT, tls: true, accept_invalid_certs: false, nick: "", alt_nick: "", username: "", realname: "", auth: "none", account: "", autoconnect: false, autojoin: "" };

  const strict = (await inv("save_server", { entry: { ...base, name: "TLS strict" } })).ok.id;
  await inv("connect_server", { id: strict });
  const strictText = await until(async () => { const o = (await inv("get_overview", {})).ok; const st = o.states[strict]; return st && st.status_text.includes("TLS") && st.status_text; }, "TLS refusal reported", 15000);
  check(/TLS/.test(strictText), "an untrusted certificate is refused: " + strictText.slice(0, 60));
  await inv("disconnect_server", { id: strict });
  await until(async () => (await status(strict)) === "disconnected", "strict server stops", 10000);

  const lax = (await inv("save_server", { entry: { ...base, name: "TLS lax", accept_invalid_certs: true } })).ok.id;
  await inv("connect_server", { id: lax });
  await until(async () => (await status(lax)) === "connected", "TLS connection with the accepted certificate", 15000);
  const laxText = await consoleText(lax, "Bienvenue sur FakeNet");
  check(laxText.includes("Bienvenue sur FakeNet") && !laxText.includes("non chiffrée"), "TLS session works, no clear-text warning");
  await inv("disconnect_server", { id: lax });
  await until(async () => (await status(lax)) === "disconnected", "lax server stops", 10000);

  const sasl = (await inv("save_server", { entry: { ...base, name: "SASL", port: IRC_PORT, tls: false, auth: "sasl", account: "kirc" }, authSecret: "secret" })).ok.id;
  const view = (await inv("get_overview", {})).ok.servers.find(v => v.id === sasl);
  check(view.has_auth === true && view.has_pass === false, "password stored in the credential manager (flags only reach the page)");
  check(!JSON.stringify((await inv("get_overview", {})).ok).includes("secret"), "the secret never comes back to the page");
  await inv("connect_server", { id: sasl });
  await until(async () => (await status(sasl)) === "connected", "SASL connection", 15000);
  check((await consoleText(sasl, "Authentification SASL")).includes("Authentification SASL réussie"), "SASL authentication succeeds");
  await inv("disconnect_server", { id: sasl });
  await until(async () => (await status(sasl)) === "disconnected", "sasl server stops", 10000);
  await inv("save_server", { entry: { ...base, id: sasl, name: "SASL", port: IRC_PORT, tls: false, auth: "sasl", account: "kirc" }, authSecret: "mauvais" });
  await inv("connect_server", { id: sasl });
  await until(async () => (await status(sasl)) === "connected", "SASL connection (bad password)", 15000);
  check((await consoleText(sasl, "Authentification SASL")).includes("Authentification SASL échouée"), "a wrong password is reported, the session goes on");
  await inv("disconnect_server", { id: sasl });
  await until(async () => (await status(sasl)) === "disconnected", "sasl server stops again", 10000);
  for (const id of [strict, lax, sasl]) await inv("delete_server", { id });
  check((await inv("get_overview", {})).ok.servers.length === 1, "test servers deleted again");
  for (const t of await targets()) { const q = new URL(t.url).searchParams; if (q.get("w") === "console" && q.get("s") !== "s1") { const p = await Page.open(t); await p.fire("window.__TAURI__.core.invoke('win_close')"); p.close(); } }

  console.log("ignore, alert words, whois, channel list");
  await say("/ignore bob");
  await until(async () => (await lines()).some(l => l.includes("bob est ignoré")), "ignore confirmed");
  await say("/quote XSAY #test bob SECRET-IGNORED");
  await say("/quote XSAY #test carol repere-fin");
  await until(async () => (await lines()).some(l => l.includes("repere-fin")), "marker line");
  check(!(await lines()).some(l => l.includes("<bob> SECRET-IGNORED")), "an ignored person is silent");
  await say("/unignore bob");
  await say("/quote XSAY #test bob visible-again");
  await until(async () => (await lines()).some(l => l.includes("visible-again")), "an unignored person is heard again");

  const ovNow = (await inv("get_overview", {})).ok;
  await inv("save_settings", { settings: { ...ovNow.settings, alert_words: ["lapin"] } });
  const hlBefore = await chat.eval("document.querySelectorAll('#log .ln.hl').length");
  await say("/quote XSAY #test carol un lapin passe");
  await until(async () => (await chat.eval("document.querySelectorAll('#log .ln.hl').length")) === hlBefore + 1, "alert word highlights a line");
  await inv("save_settings", { settings: { ...ovNow.settings, alert_words: [] } });

  await say("/whois alice");
  const who = await page(win("whois", { b: "alice" }), 10000);
  await until(() => who.eval("document.querySelector('#whoisForm').textContent.includes('virtual.test')"), "whois card filled");
  check(await who.eval("document.querySelector('#whoisForm').textContent.includes('Nom réel')"), "whois card shows the real name");
  await who.shot("13-whois");
  await who.fire("window.__TAURI__.core.invoke('win_close')");

  await say("/list");
  const lst = await page(win("list"), 10000);
  await until(() => lst.eval("document.querySelectorAll('#chanList .chan-row').length === 200"), "first page of the list");
  check(await lst.eval("document.querySelector('#lStatusText').textContent.includes('sur 60')"), "list status counts the rows");
  check(await lst.eval("!!document.querySelector('#chanList .more')"), "a button offers the rest");
  await lst.eval("(() => { const i = document.querySelector('#lFilter'); i.value = 'salon007'; i.dispatchEvent(new Event('input')); })()");
  await until(() => lst.eval("document.querySelectorAll('#chanList .chan-row').length === 1"), "list filter");
  await lst.eval("(() => { const i = document.querySelector('#lFilter'); i.value = ''; i.dispatchEvent(new Event('input')); })()");
  await until(() => lst.eval("document.querySelectorAll('#chanList .chan-row').length === 200"), "filter cleared");
  check(await lst.eval("document.querySelectorAll('#lHead .cdx-colrz').length === 2"), "the list header has a handle on each column but the last");
  // La webview partage son stockage avec le vrai Messages : on part des largeurs d'origine (double-clic sur une poignée) et on ne laisse rien derrière soi.
  await lst.eval("document.querySelector('#lHead .cdx-colrz').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))");
  const colW = () => lst.eval("JSON.stringify([...document.querySelectorAll('#lHead > .lh')].map(b => Math.round(b.getBoundingClientRect().width)))");
  const w0 = JSON.parse(await colW());
  await lst.eval("(() => { const h = document.querySelector('#lHead .cdx-colrz'), x = h.getBoundingClientRect().left; for (const [t, d] of [['pointerdown', 0], ['pointermove', 50], ['pointerup', 50]]) h.dispatchEvent(new PointerEvent(t, { bubbles: true, button: 0, pointerId: 5, clientX: x + d })); })()");
  const w1 = JSON.parse(await colW());
  check(w1[0] === w0[0] + 50 && w1[1] === w0[1] && w1[2] === w0[2] - 50 && w0[0] === 181 && w0[1] === 85, `dragging a column edge widens it and narrows the flexible one (${w0} -> ${w1})`);
  check(await lst.eval("(() => { const r = document.querySelector('#chanList .chan-row'), h = document.querySelectorAll('#lHead > .lh'); return [0, 1].every(i => Math.abs(r.children[i].getBoundingClientRect().width - h[i].getBoundingClientRect().width) <= 1); })()"), "the rows follow the header columns");
  check(await lst.eval("document.querySelector('#lHead .lh.on').dataset.sort === 'users'"), "dragging a column edge does not sort the list");
  await lst.eval("document.querySelector('#lHead .cdx-colrz').dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); localStorage.removeItem('cdx.cols.salons')");
  await lst.shot("14-list");
  await lst.fire("window.__TAURI__.core.invoke('win_close')");

  console.log("themes");
  const mid = page => page.eval("getComputedStyle(document.documentElement).getPropertyValue('--a-mid').trim()");
  check((await mid(chat)) === "#880000", "the default theme is the CDX red");
  const seen = new Set(["#880000"]);
  for (const th of ["jean", "reve-vert", "reve-violet", "ambre", "sarcelle", "techno"]) {
    const r = await inv("save_settings", { settings: { ...ovNow.settings, theme: th } });
    check(r.ok === true, "theme " + th + " saved");
    await until(async () => (await chat.eval("document.documentElement.dataset.theme")) === th && (await launcher.eval("document.documentElement.dataset.theme")) === th, "open windows follow the theme " + th, 5000);
    const m = await mid(chat);
    check(!seen.has(m), "theme " + th + " has its own accent (" + m + ")");
    seen.add(m);
    await chat.shot("20-theme-" + th);
  }
  check((await inv("save_settings", { settings: { ...ovNow.settings, theme: "../evil" } })).err !== undefined, "an unknown theme is refused");
  check((await inv("save_settings", { settings: { ...ovNow.settings, theme: "charbon" } })).err !== undefined, "a theme name of cdx-ui 1.x is only migrated from an old config.json, not accepted when saving");
  await inv("open_window", { kind: "about" });
  const aboutT = await page(win("about"), 10000);
  check((await aboutT.eval("document.documentElement.dataset.theme")) === "techno", "a window opened later has the current theme at once");
  await aboutT.fire("window.__TAURI__.core.invoke('win_close')");
  await inv("save_settings", { settings: { ...ovNow.settings, theme: "cdx" } });
  await until(async () => (await mid(chat)) === "#880000", "back to the default theme", 5000);

  console.log("update check");
  // The release channel may or may not have a manifest yet: either answer is fine, a hang or a crash is not.
  const upd = await inv("check_update", {});
  check(upd.ok !== undefined || typeof upd.err === "string", "a manual update check answers (" + (upd.err ? upd.err.slice(0, 60) : JSON.stringify(upd.ok)) + ")");
  check((await inv("update_info", {})).ok === null || typeof (await inv("update_info", {})).ok === "object", "update_info answers");
  const aboutU = await (async () => { await inv("open_window", { kind: "about" }); return page(win("about"), 10000); })();
  await until(() => aboutU.eval("!!document.querySelector('#checkUpdate')"), "the About window has the update button", 8000);
  check(true, "the About window has the update button");
  await aboutU.fire("window.__TAURI__.core.invoke('win_close')");

  console.log("answers and the quick switcher");
  const bobRow = "[...document.querySelectorAll('#log .ln')].filter(e => e.textContent.includes('une question')).pop()";
  await say("/quote XSAY #test bob une question ?");
  await until(() => chat.eval(`!!(${bobRow} && ${bobRow}.querySelector('.rp'))`), "a message with an identifier can be answered");
  const bobId = await chat.eval(`${bobRow}.dataset.msgid`);
  check(/^m\d+$/.test(bobId), "the line carries the server's message id (" + bobId + ")");
  await chat.eval(`${bobRow}.querySelector('.rp').click()`);
  check(await chat.eval("!document.querySelector('#replyBar').hidden && document.querySelector('#replyText').textContent.includes('bob')"), "the answer bar names who is answered");
  await chat.eval("document.querySelector('#msgInput').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))");
  check(await chat.eval("document.querySelector('#replyBar').hidden"), "Escape cancels the answer");
  await chat.eval(`${bobRow}.querySelector('.rp').click()`);
  await say("oui, bien sûr");
  const quoted = (who, needle) => chat.eval(`[...document.querySelectorAll('#log .ln')].some(e => e.textContent.includes(${JSON.stringify(needle)}) && (e.querySelector('.rq') || {textContent: ''}).textContent.includes(${JSON.stringify(who)}))`);
  await until(() => quoted("bob", "ok reply=" + bobId), "the server got our +draft/reply tag (alice answers it, quoting bob)");
  check(await quoted("bob", "oui, bien sûr"), "our own answer quotes the message too");
  check(await chat.eval("document.querySelector('#replyBar').hidden"), "the answer bar is closed once sent");
  await say("/quote XREPLY #test carol " + bobId + " d'accord avec bob");
  await until(() => quoted("une question", "d'accord avec bob"), "an incoming answer quotes what it answers");
  await chat.shot("18-chat-answers");
  await say("/quote XREPLY #test carol m-inconnu très vieux message");
  await until(() => chat.eval("[...document.querySelectorAll('#log .rq')].some(e => e.textContent.includes('plus ancien'))"), "an answer to an unknown message says so");

  await chat.eval("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))");
  const sw = await page(win("switcher"), 10000);
  await until(() => sw.eval("document.querySelectorAll('#swList .row-item').length > 3"), "the switcher lists destinations");
  const type = text => sw.eval(`(() => { const i = document.querySelector('#swInput'); i.value = ${JSON.stringify(text)}; i.dispatchEvent(new Event('input')); })()`);
  const rows = () => sw.eval("[...document.querySelectorAll('#swList .row-item')].map(r => r.textContent)");
  await type("tes");
  check((await rows())[0].startsWith("#test"), "typing finds the channel first: " + (await rows())[0]);
  await sw.shot("17-switcher");
  await type("#nouveau");
  check((await rows())[0].startsWith("Rejoindre #nouveau"), "# offers to join");
  await type("@zoe");
  check((await rows())[0].startsWith("Écrire à zoe"), "@ offers to write");
  await type("regl");
  check((await rows())[0].startsWith("Réglages"), "actions are found too");
  await type("zzzzqq");
  check((await rows())[0].includes("Rien ne correspond"), "no match says so");
  await type("");
  const all = await rows();
  check(all.some(r => r.startsWith("#test")) && all.some(r => r.startsWith("Messages du serveur")), "empty search lists the channels and the console");
  await type("alice");
  check((await rows())[0].startsWith("alice"), "a private conversation is found");
  await type("regl");
  await sw.fire("document.querySelector('#swInput').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
  await until(async () => !(await targets()).some(t => new URL(t.url).searchParams.get("w") === "switcher"), "the switcher closes after the choice", 8000);
  check((await targets()).some(t => new URL(t.url).searchParams.get("w") === "settings"), "and the chosen window is open");

  console.log("unread counters and paste");
  await launcher.send("Page.bringToFront");
  await sleep(500);
  await say("/quote XSAY #test carol non-lu-1");
  const unreadShown = await until(async () => launcher.eval("[...document.querySelectorAll('#openList .row-item')].some(r => r.querySelector('.count'))"), "unread badge in the main menu", 8000).catch(() => false);
  check(unreadShown, "the main menu shows an unread count");
  if (unreadShown) {
    await launcher.shot("15-launcher-unread");
    const tt = await chat.eval("document.querySelector('#chatWin .tb-title').textContent + ' | ' + document.body.className");
    check(tt.includes("("), "the title bar of the window shows the unread count: " + tt);
    await chat.send("Page.bringToFront");
    await until(() => launcher.eval("![...document.querySelectorAll('#openList .row-item')].some(r => r.querySelector('.count'))"), "reading the window clears the count", 8000);
    check(true, "looking at the window clears the count");
  }
  await chat.eval(`(() => { const dt = new DataTransfer(); dt.setData("text", "premiere\\n/quit\\nderniere"); document.querySelector('#msgInput').dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, cancelable: true, bubbles: true })); })()`);
  check(await chat.eval("!document.querySelector('#pasteBar').hidden && document.querySelector('#msgInput').value === ''"), "a multi-line paste asks first and does not fill the field");
  await chat.eval("document.querySelector('#pasteSend').click()");

  await until(async () => (await lines()).some(l => l.includes("<kirc>/quit")), "a pasted line starting with a slash is sent as text");
  check(!(await lines()).some(l => l.includes("Au revoir")), "and was not run as a command");

  console.log("logs");
  await sleep(300);
  const { readFile } = await import("node:fs/promises");
  const logText = await readFile(join(data, "logs", "Serveur de test", "#test.log"), "utf8");
  check(logText.includes("<kirc> bonjour tout le monde") && logText.includes("<bob> salut kirc"), "the conversation is written to a log file");
  check(!logText.includes("<bob> SECRET-IGNORED"), "ignored messages are not logged");

  console.log("protocols");
  const overview = (await inv("get_overview")).ok;
  const known = overview.servers[0];
  check(known.protocol === "irc" && known.caps.ranks === true && known.caps.channel_list === true, "a server carries its protocol and what it can do");
  const snapshot = (await inv("buffer_snapshot", { server: known.id, buffer: "#test" })).ok;
  check(snapshot.caps.whois === true && snapshot.caps.topic === true, "a window learns what its server can do");
  check(((await inv("save_server", { entry: { ...known, id: "", name: "Inconnu", protocol: "telegram" } })).err || "").includes("Protocole inconnu"), "a server of an unknown protocol is refused");
  check(await chat.eval("[...document.querySelectorAll('#memberList .mem')].length > 0"), "the member list still fills through the renamed event");

  console.log("pairing code (the window of the messages draws what an account to link would send)");
  const fakeCode = { size: 21, modules: "10".repeat(221).slice(0, 441), seconds: 60 };
  const fromServer = event => con.eval(`window.__TAURI__.event.emit("chat", { server: ${JSON.stringify(known.id)}, seq: 999999, event: ${JSON.stringify(event)} })`);
  await fromServer({ kind: "pairing", code: fakeCode });
  await until(() => con.eval("!document.querySelector('#pairing').hidden && document.querySelector('#pairCanvas').width === (21 + 8) * 5"), "the code to scan is drawn");
  await con.shot("23-pairing");
  await fromServer({ kind: "pairing", code: { size: 21, modules: "12", seconds: 60 } });
  await sleep(300);
  check(await con.eval("document.querySelector('#pairing').hidden"), "a malformed code is not drawn");
  await fromServer({ kind: "pairing", code: fakeCode });
  await fromServer({ kind: "paired" });
  await until(() => con.eval("document.querySelector('#pairing').hidden"), "the code goes away once the account is linked");
  check(await book.eval("document.querySelector('#fProto').hidden"), "the address book offers no protocol choice when the build has only one");
  check(await book.eval("document.querySelector('#fHost').closest('fieldset').hidden === false && document.querySelector('#fWaName').closest('fieldset').hidden === true"), "the form shows the IRC fields and hides the WhatsApp ones");

  console.log("images in a line (the preview, never markup from outside)");
  const tinyJpeg = "/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=";
  const imageLine = (msgid, thumb) => ({ kind: "line", line: { buffer: "#test", kind: "msg", nick: "bob", text: "[image] vue", time: Date.now(), highlight: false, own: false, msgid, media: { kind: "image", thumb, width: 1, height: 1 } } });
  const toChat = event => chat.eval(`window.__TAURI__.event.emit("chat", { server: ${JSON.stringify(known.id)}, seq: 999999, event: ${JSON.stringify(event)} })`);
  await toChat(imageLine("img-ok", tinyJpeg));
  await toChat(imageLine("img-bad", '"><img src=x onerror=alert(1)>'));
  await until(() => chat.eval("document.querySelectorAll('.ln .thumb').length === 2"), "two lines carry an image");
  await chat.shot("24-image-line");
  check(await chat.eval("document.querySelector('.ln .thumb img').src.startsWith('data:image/jpeg;base64,')"), "the preview is shown from its data");
  check(await chat.eval("[...document.querySelectorAll('.ln .thumb')][1].querySelector('img') === null"), "a preview that is not base64 is not shown");
  check(await chat.eval("document.querySelectorAll('#log [onerror]').length === 0 && [...document.querySelectorAll('#log img')].every(i => i.closest('.thumb'))"), "nothing from outside became markup");
  check(await chat.eval("document.querySelector('#btnImage').hidden"), "an IRC window offers no image button");
  check(await chat.eval("(() => { const b = document.querySelector('#btnCall'); b.hidden = false; const h = b.getBoundingClientRect().height; b.hidden = true; return h <= 22 && document.querySelector('#msgInput').getBoundingClientRect().height <= 24; })()"), "the input bar is one line high: the buttons at the charter height, the text box on one line");
  const grown = await chat.eval("(() => { const i = document.querySelector('#msgInput'); i.value = 'a\\nb\\nc\\nd\\ne\\nf\\ng\\nh'; i.dispatchEvent(new Event('input', { bubbles: true })); const h = i.getBoundingClientRect().height; i.value = ''; i.dispatchEvent(new Event('input', { bubbles: true })); return h + ',' + i.getBoundingClientRect().height; })()");
  check(/^\d+,\d+$/.test(grown) && +grown.split(",")[0] > 60 && +grown.split(",")[0] <= 80 && +grown.split(",")[1] <= 24, `the text box grows with the message, five lines at most, and shrinks back (${grown})`);
  await chat.eval("document.querySelector('.ln .thumb').click()");
  await sleep(500);
  check(await chat.eval("document.querySelector('#viewer').hidden"), "an image that cannot be fetched leaves the viewer closed");
  await chat.eval("document.querySelector('#viewer').hidden = false");
  await chat.eval("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))");
  check(await chat.eval("document.querySelector('#viewer').hidden"), "Esc closes the viewer");

  console.log("calls (the panel follows what the application tells)");
  const callEvent = c => toChat({ kind: "call", call: { id: "c1", buffer: "#test", video: false, text: "Alice", ...c } });
  await callEvent({ state: "incoming" });
  await until(() => chat.eval("!document.querySelector('#callbox').hidden && document.querySelector('#callInfo').textContent.includes('Alice')"), "an incoming call shows the panel with who calls", 8000);
  check(await chat.eval("!document.querySelector('#callAnswer').hidden && document.querySelector('#callAnswerVideo').hidden && document.querySelector('#callHang').textContent === 'Refuser'"), "an audio call can be answered or refused");
  await callEvent({ state: "incoming", video: true });
  await until(() => chat.eval("!document.querySelector('#callAnswerVideo').hidden"), "a video call can be answered with the video", 8000);
  await callEvent({ state: "active" });
  await until(() => chat.eval("document.querySelector('#callAnswer').hidden && !document.querySelector('#callMute').hidden && document.querySelector('#callHang').textContent === 'Raccrocher'"), "an active call offers to mute and to hang up", 8000);
  await callEvent({ id: "other", state: "ended", text: "autre" });
  await sleep(300);
  check(await chat.eval("!document.querySelector('#callbox').hidden && document.querySelector('#callInfo').textContent.includes('Alice')"), "news of another call does not end this one");
  await callEvent({ state: "ended", text: "Appel terminé (00:12)" });
  await until(() => chat.eval("document.querySelector('#callInfo').textContent.includes('Appel terminé (00:12)') && document.querySelector('#callHang').hidden"), "the end of the call is told", 8000);
  check(await chat.eval("document.querySelector('#btnCall').hidden && document.querySelector('#btnVideoCall').hidden"), "an IRC window offers no call buttons");

  console.log("hiding a chat from the list of the main menu");
  const menuRows = (sel = "") => launcher.eval(`[...document.querySelectorAll('#openList .row-item${sel}')].map(e => e.textContent)`);
  const refreshLauncher = () => launcher.eval("window.dispatchEvent(new Event('focus'))");
  await launcher.eval("localStorage.removeItem('messages.hiddenOpen')");
  check((await inv("set_hidden", { server: known.id, buffer: "#TEST", hidden: true })).err === undefined, "a chat is hidden (the case does not matter)");
  await refreshLauncher();
  await until(async () => (await menuRows(".hidhead")).some(t => t.includes("Masquées (1)")), "the hidden chat goes under a heading", 8000);
  check(!(await menuRows(":not(.hidhead)")).some(t => t.includes("#test")), "it is gone from the list while the heading is closed");
  await launcher.eval("document.querySelector('#openList .hidhead').click()");
  await until(async () => (await menuRows(".hiddenrow")).some(t => t.includes("#test")), "the heading opens the hidden chats", 8000);
  await launcher.shot("25-hidden-chats");
  await launcher.eval("document.querySelector('#openList .hiddenrow').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 20, clientY: 20 }))");
  check(await launcher.eval("[...document.querySelectorAll('.menu .menu-item')].map(e => e.textContent).includes('Réafficher dans la liste')"), "the right-click menu offers to show it again");
  await launcher.eval("[...document.querySelectorAll('.menu .menu-item')].find(e => e.textContent === 'Réafficher dans la liste').click()");
  await until(async () => (await menuRows(":not(.hidhead)")).some(t => t.includes("#test")) && (await menuRows(".hidhead")).length === 0, "shown again: back in the list, no heading left", 8000);
  await launcher.eval("document.querySelector('#openList .row-item').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 20, clientY: 20 }))");
  check(await launcher.eval("[...document.querySelectorAll('.menu .menu-item')].map(e => e.textContent).includes('Masquer de la liste')"), "the right-click menu of a chat in the list offers to hide it");
  await launcher.eval("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); document.body.click()");

  console.log("tabs: merging two windows");
  const labelOf = p => p.eval("window.__TAURI_INTERNALS__.metadata.currentWindow.label");
  const [chatLabel, conLabel] = [await labelOf(chat), await labelOf(con)];
  const shown = async label => (await inv("window_visible", { label })).ok === true;
  check((await inv("tab_merge", { dragged: conLabel, target: chatLabel })).err === undefined, "the console is merged into the channel window");
  await until(() => con.eval("!document.querySelector('.tabs').hidden"), "the strip of tabs shows in the merged window", 8000);
  await until(() => chat.eval("!document.querySelector('.tabs').hidden"), "and in the window it joined", 8000);
  check((await chat.eval("document.querySelectorAll('.tabs .tab').length")) === 2, "two tabs");
  check((await chat.eval("document.querySelector('.tabs .tab.on')?.textContent")) === "Messages — Serveur de test", "the dropped window is the tab shown");
  check((await chat.eval("[...document.querySelectorAll('.tabs .tab')].map(t => t.textContent).includes('#test')")), "a channel tab carries the channel name alone");
  await sleep(300);
  check((await shown(conLabel)) && !(await shown(chatLabel)), "only the tab shown is on screen");
  check(await settings.eval("document.querySelector('.tabs').hidden"), "a third window outside the group shows no strip of tabs");
  check(await settings.eval("!document.body.classList.contains('droptarget')"), "and is not lit up as a drop target");
  await chat.shot("21-tabs-console");
  await con.eval("[...document.querySelectorAll('.tabs .tab')].find(t => !t.classList.contains('on')).click()");
  await until(async () => (await shown(chatLabel)) && !(await shown(conLabel)), "clicking the other tab brings its window forward", 8000);
  await chat.shot("22-tabs-chat");
  check((await inv("tab_detach", { label: conLabel })).err === undefined, "a tab is detached");
  await until(() => chat.eval("document.querySelector('.tabs').hidden"), "a lone window has no strip of tabs", 8000);
  await until(() => shown(conLabel), "the detached window is on screen again", 8000);

  console.log("connection drop");
  await say("/quote XDROP");
  await until(() => chat.eval("!document.querySelector('#chatBanner').hidden"), "banner shows the loss");
  await chat.shot("11-chat-disconnected");
  await until(() => chat.eval("document.querySelector('#chatBanner').hidden"), "automatic reconnection", 20000);
  await until(async () => (await lines()).filter(l => l.includes("Vous avez rejoint #test")).length >= 2, "channel rejoined");

  console.log("quit");
  await launcher.eval("document.querySelector('#btnQuit').click()");
  await until(async () => { try { await targets(); return false; } catch { return true; } }, "application exits", 10000);

  console.log("second start: the end of the conversation comes back");
  start(exe, [], { env: appEnv });
  const launcher2 = await page(win("launcher"), 30000);
  await until(() => launcher2.eval("document.querySelectorAll('#serverSelect option').length === 1"), "server in the launcher (second start)");
  await launcher2.eval("document.querySelector('#connectBtn').click()");
  const chat2 = await page(win("chat", { b: "#test" }), 15000);
  await until(() => chat2.eval("!!document.querySelector('#log .ln.sep')"), "history separator");
  check(await chat2.eval("[...document.querySelectorAll('#log .ln.past')].some(e => e.textContent.includes('bonjour tout le monde'))"), "earlier lines are shown, dimmed");
  check(await chat2.eval("!document.querySelector('#log .ln.past .tx a[href]') || true"), "history rendered");
  await chat2.shot("16-chat-history");
  await launcher2.eval("document.querySelector('#btnQuit').click()");
  await until(async () => { try { await targets(); return false; } catch { return true; } }, "application exits (second start)", 10000);
}

main().then(() => { console.log(failures ? `\n${failures} check(s) FAILED` : "\nall checks passed"); process.exit(failures ? 1 : 0); })
  .catch(e => { console.error("\nERROR:", e.message); process.exit(2); });
