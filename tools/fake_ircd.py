"""A tiny scripted IRC server for testing KIRC by hand and in tests/e2e.mjs.

    python tools/fake_ircd.py [--port 6667] [--tls-port 6697 --cert c.pem --key k.pem]

Not a real server: just enough of the protocol (CAP, SASL PLAIN with password "secret", NICK/USER,
JOIN/PART/PRIVMSG/NOTICE/TOPIC/MODE/NAMES/WHOIS/KICK/AWAY/QUIT, PING) and three virtual people on #test
(alice is operator, bob has voice, carol is plain). Extra commands let a test script drive the scene:

    XSAY <target> <nick> <text...>       a virtual person speaks (target = channel, or our own nick)
    XACT <target> <nick> <text...>       ... with /me
    XJOIN <channel> <nick>   XPART <channel> <nick>   XQUIT <nick>   XNICK <old> <new>
    XKICK <channel> <nick>   XMODE <channel> <modes> <nick>   XTOPIC <channel> <nick> <text...>
    XNOTICE <nick> <text...>             a notice from a virtual person to us
    XDROP                                closes our connection (to test reconnection)
"""
import argparse
import asyncio
import base64
import ssl

SERVER = "irc.fake.test"
NETWORK = "FakeNet"


class Client:
    def __init__(self, reader, writer):
        self.reader, self.writer = reader, writer
        self.nick = None
        self.user = "user"
        self.registered = False
        self.caps = set()
        self.cap_negotiating = False
        self.sasl = False
        self.authed = None
        self.away = None

    def send(self, line):
        self.writer.write((line + "\r\n").encode("utf-8"))

    @property
    def mask(self):
        return f"{self.nick}!{self.user}@localhost"


class Server:
    def __init__(self):
        self.clients = {}
        self.channels = {}  # name -> {"topic": str, "members": {nick: modes-string}}
        self.virtual = {"alice": "@", "bob": "+", "carol": ""}
        self.log = []

    def chan(self, name):
        key = name.lower()
        if key not in self.channels:
            self.channels[key] = {"name": name, "topic": "", "members": {}}
            if key == "#test":
                self.channels[key]["topic"] = "Salon de test de KIRC"
                for n, m in self.virtual.items():
                    self.channels[key]["members"][n] = m
        return self.channels[key]

    def broadcast(self, ch, line, skip=None):
        for nick in list(ch["members"]):
            c = self.clients.get(nick.lower())
            if c and c is not skip:
                c.send(line)

    def welcome(self, c):
        c.registered = True
        n = c.nick
        c.send(f":{SERVER} 001 {n} :Bienvenue sur {NETWORK}, {c.mask}")
        c.send(f":{SERVER} 002 {n} :Your host is {SERVER}")
        c.send(f":{SERVER} 003 {n} :This server was created for testing")
        c.send(f":{SERVER} 004 {n} {SERVER} fake-1.0 iow beiklmnopstv")
        c.send(f":{SERVER} 005 {n} PREFIX=(ov)@+ CHANTYPES=# CHANMODES=beI,k,l,imnpst NETWORK={NETWORK} CASEMAPPING=rfc1459 :are supported by this server")
        c.send(f":{SERVER} 251 {n} :There are 4 users on 1 server")
        c.send(f":{SERVER} 375 {n} :- {SERVER} Message of the day -")
        c.send(f":{SERVER} 372 {n} :- Bienvenue sur le serveur de test.")
        c.send(f":{SERVER} 372 {n} :- \x02Gras\x02, \x0304rouge\x03, https://example.org/page.")
        c.send(f":{SERVER} 376 {n} :End of /MOTD command.")

    def names(self, c, ch):
        members = ch["members"]
        items = []
        for nick, modes in members.items():
            prefix = "@" if "@" in modes else ""
            if "+" in modes:
                prefix += "+"
            items.append(prefix + nick)
        for i in range(0, len(items), 20):
            c.send(f":{SERVER} 353 {c.nick} = {ch['name']} :{' '.join(items[i:i + 20])}")
        c.send(f":{SERVER} 366 {c.nick} {ch['name']} :End of /NAMES list.")

    def join(self, c, name):
        ch = self.chan(name)
        if c.nick in ch["members"]:
            return
        ch["members"][c.nick] = "@" if len(ch["members"]) == 0 else ""
        self.broadcast(ch, f":{c.mask} JOIN {ch['name']}")
        if ch["topic"]:
            c.send(f":{SERVER} 332 {c.nick} {ch['name']} :{ch['topic']}")
            c.send(f":{SERVER} 333 {c.nick} {ch['name']} alice!a@localhost 1700000000")
        self.names(c, ch)

    def part(self, c, name, reason=""):
        ch = self.channels.get(name.lower())
        if not ch or c.nick not in ch["members"]:
            c.send(f":{SERVER} 442 {c.nick} {name} :You're not on that channel")
            return
        self.broadcast(ch, f":{c.mask} PART {ch['name']}" + (f" :{reason}" if reason else ""))
        del ch["members"][c.nick]

    def quit(self, c, reason="Quit"):
        for ch in self.channels.values():
            if c.nick in ch["members"]:
                del ch["members"][c.nick]
                self.broadcast(ch, f":{c.mask} QUIT :{reason}", skip=c)
        self.clients.pop((c.nick or "").lower(), None)

    def vmask(self, nick):
        return f"{nick}!{nick}@virtual.test"

    async def handle(self, c, line):
        self.log.append(line)
        parts = line.split(" ")
        cmd = parts[0].upper()
        rest = line[len(parts[0]) + 1:] if len(parts) > 1 else ""

        def trailing():
            if " :" in " " + rest:
                head, _, tail = (" " + rest).partition(" :")
                return head.split(), tail
            return rest.split(), ""

        args, tail = trailing()
        if cmd == "CAP":
            sub = args[0].upper() if args else ""
            if sub == "LS":
                c.cap_negotiating = True
                c.send(f":{SERVER} CAP * LS :multi-prefix away-notify server-time sasl=PLAIN")
            elif sub == "REQ":
                wanted = tail.split()
                c.caps.update(wanted)
                c.send(f":{SERVER} CAP * ACK :{tail}")
            elif sub == "END":
                c.cap_negotiating = False
                if c.nick and not c.registered:
                    self.welcome(c)
        elif cmd == "AUTHENTICATE":
            if args and args[0] == "PLAIN":
                c.send("AUTHENTICATE +")
            else:
                raw = base64.b64decode(args[0]).split(b"\0")
                if len(raw) == 3 and raw[2] == b"secret":
                    c.authed = raw[1].decode()
                    c.send(f":{SERVER} 900 {c.nick or '*'} {c.mask} {c.authed} :You are now logged in as {c.authed}")
                    c.send(f":{SERVER} 903 {c.nick or '*'} :SASL authentication successful")
                else:
                    c.send(f":{SERVER} 904 {c.nick or '*'} :SASL authentication failed")
        elif cmd == "PASS":
            pass
        elif cmd == "NICK":
            new = args[0] if args else tail
            if new.lower() in self.clients or new.lower() in self.virtual:
                c.send(f":{SERVER} 433 * {new} :Nickname is already in use")
                return
            if c.registered:
                old = c.mask
                self.clients.pop(c.nick.lower(), None)
                for ch in self.channels.values():
                    if c.nick in ch["members"]:
                        ch["members"][new] = ch["members"].pop(c.nick)
                c.send(f":{old} NICK :{new}")
                c.nick = new
            else:
                c.nick = new
            self.clients[new.lower()] = c
            if not c.registered and not c.cap_negotiating and c.user_seen:
                self.welcome(c)
        elif cmd == "USER":
            c.user = args[0] if args else "user"
            c.user_seen = True
            if c.nick and not c.registered and not c.cap_negotiating:
                self.welcome(c)
        elif not c.registered:
            return
        elif cmd == "PING":
            c.send(f":{SERVER} PONG {SERVER} :{tail or (args[0] if args else '')}")
        elif cmd == "PONG":
            pass
        elif cmd == "JOIN":
            for name in args[0].split(","):
                self.join(c, name)
        elif cmd == "PART":
            self.part(c, args[0], tail)
        elif cmd in ("PRIVMSG", "NOTICE"):
            target = args[0]
            text = tail if tail else (args[1] if len(args) > 1 else "")
            if target.startswith("#"):
                ch = self.channels.get(target.lower())
                if ch and c.nick in ch["members"]:
                    self.broadcast(ch, f":{c.mask} {cmd} {ch['name']} :{text}", skip=c)
                    if cmd == "PRIVMSG" and "alice" in text.lower() and not text.startswith("\x01"):
                        await asyncio.sleep(0.2)
                        self.broadcast(ch, f":{self.vmask('alice')} PRIVMSG {ch['name']} :{c.nick}: je suis là !")
                else:
                    c.send(f":{SERVER} 404 {c.nick} {target} :Cannot send to channel")
            elif target.lower() in self.virtual and cmd == "PRIVMSG" and not text.startswith("\x01"):
                await asyncio.sleep(0.2)
                c.send(f":{self.vmask(target.lower())} PRIVMSG {c.nick} :Tu as dit : {text}")
            elif target.lower() in self.clients:
                self.clients[target.lower()].send(f":{c.mask} {cmd} {target} :{text}")
            elif target.lower() in ("nickserv", "chanserv"):
                c.send(f":{target}!{target}@services NOTICE {c.nick} :You are now identified for {c.nick}.")
            else:
                c.send(f":{SERVER} 401 {c.nick} {target} :No such nick/channel")
        elif cmd == "TOPIC":
            ch = self.channels.get(args[0].lower())
            if not ch:
                return
            if tail or (len(args) > 1):
                ch["topic"] = tail
                self.broadcast(ch, f":{c.mask} TOPIC {ch['name']} :{tail}")
            else:
                c.send(f":{SERVER} 332 {c.nick} {ch['name']} :{ch['topic']}")
        elif cmd == "NAMES":
            ch = self.channels.get(args[0].lower())
            if ch:
                self.names(c, ch)
        elif cmd == "MODE":
            if args and args[0].startswith("#"):
                ch = self.channels.get(args[0].lower())
                if ch and len(args) >= 3:
                    sign = "+"
                    ai = 2
                    for m in args[1]:
                        if m in "+-":
                            sign = m
                        elif m in "ov" and ai < len(args):
                            sym = "@" if m == "o" else "+"
                            who = args[ai]
                            ai += 1
                            cur = ch["members"].get(who, "")
                            ch["members"][who] = (cur + sym) if sign == "+" and sym not in cur else cur.replace(sym, "")
                    self.broadcast(ch, f":{c.mask} MODE {ch['name']} {' '.join(args[1:])}")
                elif ch:
                    c.send(f":{SERVER} 324 {c.nick} {ch['name']} +nt")
        elif cmd == "KICK":
            ch = self.channels.get(args[0].lower())
            if ch and args[1] in ch["members"]:
                self.broadcast(ch, f":{c.mask} KICK {ch['name']} {args[1]} :{tail}")
                del ch["members"][args[1]]
        elif cmd == "WHOIS":
            who = args[-1]
            if who.lower() in self.virtual or who.lower() in self.clients:
                c.send(f":{SERVER} 311 {c.nick} {who} {who} virtual.test * :{who} (personne virtuelle)")
                c.send(f":{SERVER} 312 {c.nick} {who} {SERVER} :Serveur de test")
                c.send(f":{SERVER} 317 {c.nick} {who} 42 1700000000 :seconds idle, signon time")
                c.send(f":{SERVER} 318 {c.nick} {who} :End of /WHOIS list.")
            else:
                c.send(f":{SERVER} 401 {c.nick} {who} :No such nick/channel")
        elif cmd == "LIST":
            c.send(f":{SERVER} 321 {c.nick} Channel :Users  Name")
            for ch in self.channels.values():
                c.send(f":{SERVER} 322 {c.nick} {ch['name']} {len(ch['members'])} :[+nt] {ch['topic']}")
            for i in range(600):
                c.send(f":{SERVER} 322 {c.nick} #salon{i:03d} {600 - i} :Sujet du salon numéro {i}")
            c.send(f":{SERVER} 323 {c.nick} :End of /LIST")
        elif cmd == "AWAY":
            c.away = tail or (args[0] if args else None)
            c.send(f":{SERVER} {'306' if c.away else '305'} {c.nick} :You have been marked as {'away' if c.away else 'back'}")
        elif cmd == "QUIT":
            c.send(f"ERROR :Closing Link: localhost (Quit: {tail})")
            self.quit(c, tail)
            c.writer.close()
        # ---- test driver
        elif cmd == "XSAY" or cmd == "XACT":
            target, nick = args[0], args[1]
            text = rest.split(" ", 2)[2] if len(rest.split(" ", 2)) > 2 else ""
            if cmd == "XACT":
                text = f"\x01ACTION {text}\x01"
            line_out = f":{self.vmask(nick)} PRIVMSG {target} :{text}"
            if target.startswith("#"):
                ch = self.channels.get(target.lower())
                if ch:
                    self.broadcast(ch, line_out)
            else:
                c.send(line_out)
        elif cmd == "XJOIN":
            ch = self.chan(args[0])
            ch["members"][args[1]] = ""
            self.broadcast(ch, f":{self.vmask(args[1])} JOIN {ch['name']}")
        elif cmd == "XPART":
            ch = self.chan(args[0])
            self.broadcast(ch, f":{self.vmask(args[1])} PART {ch['name']} :à plus")
            ch["members"].pop(args[1], None)
        elif cmd == "XQUIT":
            for ch in self.channels.values():
                if args[0] in ch["members"]:
                    del ch["members"][args[0]]
                    self.broadcast(ch, f":{self.vmask(args[0])} QUIT :Ping timeout")
        elif cmd == "XNICK":
            for ch in self.channels.values():
                if args[0] in ch["members"]:
                    ch["members"][args[1]] = ch["members"].pop(args[0])
                    self.broadcast(ch, f":{self.vmask(args[0])} NICK :{args[1]}")
        elif cmd == "XKICK":
            ch = self.chan(args[0])
            self.broadcast(ch, f":{self.vmask('alice')} KICK {ch['name']} {args[1]} :dehors")
            ch["members"].pop(args[1], None)
        elif cmd == "XMODE":
            ch = self.chan(args[0])
            self.broadcast(ch, f":{self.vmask('alice')} MODE {ch['name']} {args[1]} {' '.join(args[2:])}")
        elif cmd == "XTOPIC":
            ch = self.chan(args[0])
            text = rest.split(" ", 2)[2] if len(rest.split(" ", 2)) > 2 else ""
            ch["topic"] = text
            self.broadcast(ch, f":{self.vmask(args[1])} TOPIC {ch['name']} :{text}")
        elif cmd == "XNOTICE":
            text = rest.split(" ", 1)[1] if " " in rest else ""
            c.send(f":{self.vmask(args[0])} NOTICE {c.nick} :{text}")
        elif cmd == "XDROP":
            c.writer.close()
        else:
            c.send(f":{SERVER} 421 {c.nick} {cmd} :Unknown command")


async def serve_client(server, reader, writer):
    c = Client(reader, writer)
    c.user_seen = False
    try:
        while True:
            data = await reader.readline()
            if not data:
                break
            line = data.decode("utf-8", "replace").rstrip("\r\n")
            if line:
                await server.handle(c, line)
                await writer.drain()
    except (ConnectionError, asyncio.IncompleteReadError):
        pass
    finally:
        if c.nick:
            server.quit(c, "Connection closed")
        try:
            writer.close()
        except Exception:
            pass


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=6667)
    ap.add_argument("--tls-port", type=int, default=0)
    ap.add_argument("--cert")
    ap.add_argument("--key")
    a = ap.parse_args()
    server = Server()
    handler = lambda r, w: serve_client(server, r, w)
    servers = [await asyncio.start_server(handler, "127.0.0.1", a.port)]
    msg = f"fake_ircd: plain on 127.0.0.1:{a.port}"
    if a.tls_port:
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(a.cert, a.key)
        servers.append(await asyncio.start_server(handler, "127.0.0.1", a.tls_port, ssl=ctx))
        msg += f", TLS on 127.0.0.1:{a.tls_port}"
    print(msg, flush=True)
    await asyncio.gather(*(s.serve_forever() for s in servers))


if __name__ == "__main__":
    asyncio.run(main())
