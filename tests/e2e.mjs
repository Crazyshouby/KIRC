// End-to-end test of the real application (node >= 22): starts tools/fake_ircd.py and KIRC with its
// own data directory, drives the pages through the WebView2 debugging port, checks what they show
// and saves a screenshot of each window in tests/shots/.
//
//   node tests/e2e.mjs [path-to-kirc.exe]
//
// Nothing here touches the user's real configuration (KIRC_DATA_DIR) or credentials.
import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.argv[2] || join(root, "src-tauri", "target", "debug", "kirc.exe");
const shots = join(root, "tests", "shots");
mkdirSync(shots, { recursive: true });
const data = mkdtempSync(join(tmpdir(), "kirc-e2e-"));
const DEBUG_PORT = 9333, IRC_PORT = 16667, TLS_PORT = 16697;

const sleep = ms => new Promise(r => setTimeout(r, ms));
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
  send(method, params = {}) { const id = ++this.id; this.ws.send(JSON.stringify({ id, method, params })); return new Promise((resolve, reject) => this.waiting.set(id, { resolve, reject })); }
  async eval(expr) { const r = await this.send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || "eval failed"); return r.result.value; }
  async shot(name) { const r = await this.send("Page.captureScreenshot", { format: "png" }); writeFileSync(join(shots, name + ".png"), Buffer.from(r.data, "base64")); }
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
function cleanup() { for (const c of children) { try { c.kill(); } catch { /* gone */ } } try { execFileSync("taskkill", ["/F", "/IM", "kirc.exe"], { stdio: "ignore" }); } catch { /* none */ } try { rmSync(data, { recursive: true, force: true }); } catch { /* busy */ } }
process.on("exit", cleanup);

// A raw client to drive the fake server (XSAY & co.) on behalf of the app's connection is not
// possible: the commands must come from the app's own connection, so the app sends them with /quote.

async function main() {
  if (!existsSync(exe)) throw new Error("no exe: " + exe);
  writeFileSync(join(data, "config.json"), JSON.stringify({
    version: 1,
    settings: { nick: "kirc", realname: "KIRC e2e", quit_message: "Au revoir", notifications: false },
    windows: { launcher: { x: 333, y: 222, w: 330, h: 600 } },
    servers: [{ id: "s1", name: "Serveur de test", host: "127.0.0.1", port: IRC_PORT, tls: false, autoconnect: false, autojoin: "#test", auth: "none" }],
  }));
  const cert = join(data, "cert.pem"), key = join(data, "key.pem");
  execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-days", "2", "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1"], { stdio: "ignore" });
  start("python", [join(root, "tools", "fake_ircd.py"), "--port", String(IRC_PORT), "--tls-port", String(TLS_PORT), "--cert", cert, "--key", key]);
  await until(() => new Promise(r => { const s = net.connect(IRC_PORT, "127.0.0.1", () => { s.destroy(); r(true); }); s.on("error", () => r(false)); }), "fake ircd");

  start(exe, [], { env: { ...process.env, KIRC_DATA_DIR: data, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${DEBUG_PORT}` } });

  console.log("launcher");
  const launcher = await page(win("launcher"), 30000);
  await until(() => launcher.eval("document.querySelectorAll('#serverSelect option').length === 1"), "server in the launcher");
  check(await launcher.eval("document.title") === "KIRC", "launcher title");
  check(await launcher.eval("document.querySelector('#statusText').textContent") === "Aucune connexion", "status: aucune connexion");
  check(await launcher.eval("document.querySelector('#connectBtn').textContent") === "Connecter", "connect button label");
  await launcher.shot("01-launcher");

  console.log("connection");
  await launcher.eval("document.querySelector('#connectBtn').click()");
  const chat = await page(win("chat", { b: "#test" }), 15000);
  await until(() => chat.eval("document.querySelectorAll('#memberList .mem').length === 4"), "4 members (3 virtual + us)");
  const members = await chat.eval("[...document.querySelectorAll('#memberList .mem')].map(e => e.textContent)");
  check(members.join(" ") === "@alice +bob carol kirc", "member list sorted by rank: " + members.join(" "));
  const first = members[0];
  check(first === "@alice" || first === "@kirc", "operators first: " + first);
  check((await chat.eval("document.querySelector('#topicText').textContent")) === "Salon de test de KIRC", "topic shown");
  check((await chat.eval("document.querySelector('#myNick').textContent")) === "kirc", "own nick shown");
  await chat.shot("02-chat-joined");
  await until(() => launcher.eval("document.querySelector('#statusText').textContent.includes('1 serveur')"), "launcher says 1 connected");
  check((await launcher.eval("[...document.querySelectorAll('#openList .row-item')].map(e => e.textContent).join()")) === "#test", "launcher lists #test");
  await launcher.shot("03-launcher-connected");

  const con = await page(win("console"), 10000);
  await until(() => con.eval("document.querySelector('#consoleLog').textContent.includes('Bienvenue sur FakeNet')"), "welcome in the console");
  check((await con.eval("document.querySelector('#consoleLog .ln.motd .fmt-b, #consoleLog .ln.motd span[style*=\"800\"]') !== null")), "MOTD bold rendered");
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
  await other.eval("document.querySelector('.tb-btn[data-act=close]').click()");
  await until(async () => !(await targets()).some(t => new URL(t.url).searchParams.get("b") === "#autre"), "window gone");
  await chat.eval("1");
  await until(() => launcher.eval("[...document.querySelectorAll('#openList .row-item')].map(e => e.textContent).join() === '#test'"), "launcher back to #test");

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

  const strict = (await inv("save_server", { entry: { ...base, name: "TLS strict" } })).ok;
  await inv("connect_server", { id: strict });
  const strictText = await until(async () => { const o = (await inv("get_overview", {})).ok; const st = o.states[strict]; return st && st.status_text.includes("TLS") && st.status_text; }, "TLS refusal reported", 15000);
  check(/TLS/.test(strictText), "an untrusted certificate is refused: " + strictText.slice(0, 60));
  await inv("disconnect_server", { id: strict });
  await until(async () => (await status(strict)) === "disconnected", "strict server stops", 10000);

  const lax = (await inv("save_server", { entry: { ...base, name: "TLS lax", accept_invalid_certs: true } })).ok;
  await inv("connect_server", { id: lax });
  await until(async () => (await status(lax)) === "connected", "TLS connection with the accepted certificate", 15000);
  const laxText = await consoleText(lax, "Bienvenue sur FakeNet");
  check(laxText.includes("Bienvenue sur FakeNet") && !laxText.includes("non chiffrée"), "TLS session works, no clear-text warning");
  await inv("disconnect_server", { id: lax });
  await until(async () => (await status(lax)) === "disconnected", "lax server stops", 10000);

  const sasl = (await inv("save_server", { entry: { ...base, name: "SASL", port: IRC_PORT, tls: false, auth: "sasl", account: "kirc" }, authSecret: "secret" })).ok;
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
  for (const t of await targets()) { const q = new URL(t.url).searchParams; if (q.get("w") === "console" && q.get("s") !== "s1") { const p = await Page.open(t); await p.eval("window.__TAURI__.core.invoke('win_close')"); p.close(); } }

  console.log("connection drop");
  await say("/quote XDROP");
  await until(() => chat.eval("!document.querySelector('#chatBanner').hidden"), "banner shows the loss");
  await chat.shot("11-chat-disconnected");
  await until(() => chat.eval("document.querySelector('#chatBanner').hidden"), "automatic reconnection", 20000);
  await until(async () => (await lines()).filter(l => l.includes("Vous avez rejoint #test")).length >= 2, "channel rejoined");

  console.log("quit");
  await launcher.eval("document.querySelector('#btnQuit').click()");
  await until(async () => { try { await targets(); return false; } catch { return true; } }, "application exits", 10000);
}

main().then(() => { console.log(failures ? `\n${failures} check(s) FAILED` : "\nall checks passed"); process.exit(failures ? 1 : 0); })
  .catch(e => { console.error("\nERROR:", e.message); process.exit(2); });
