"""Pixel-art icon set (16x16, black outline, flat colours) of the KDX-style UI.

Same drawing engine and palette as AudioLinker's, with icons for an IRC client.
Each icon is drawn from a few primitives, outlined automatically and exported
as a compact SVG (one <path> per colour). Imported by gen_assets.py, which
writes them into ui/icons.js (and the application icon).
"""
import math

PAL = {'K': '#000000', 'Y': '#fcc21c', 'y': '#c4840c', 'W': '#fff6dc', 'w': '#f4f4f4', 'g': '#c8c8c8', 'G': '#8c8c8c',
       'd': '#4a4a4a', 'D': '#2a2a2a', 'R': '#e02020', 'r': '#8a0202', 'O': '#f58a1a', 'o': '#b85a00',
       'B': '#2a5ae0', 'b': '#86a6ff', 'N': '#0a1a80', 'L': '#3ccf3c', 'l': '#0a7a14', 'P': '#ff5cf0', 'p': '#a0189a'}


class C:
    def __init__(s):
        s.p = {}

    def set(s, x, y, c):
        if 0 <= x < 16 and 0 <= y < 16:
            s.p[(x, y)] = c

    def rect(s, x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                s.set(x, y, c)

    def circ(s, cx, cy, r, c):
        for y in range(16):
            for x in range(16):
                if (x - cx) ** 2 + (y - cy) ** 2 <= r * r:
                    s.set(x, y, c)

    def line(s, x0, y0, x1, y1, c):
        n = max(abs(x1 - x0), abs(y1 - y0)) or 1
        for i in range(n + 1):
            s.set(round(x0 + (x1 - x0) * i / n), round(y0 + (y1 - y0) * i / n), c)

    def pat(s, x0, y0, rows):
        for j, row in enumerate(rows):
            for i, ch in enumerate(row):
                if ch != '.':
                    s.set(x0 + i, y0 + j, ch)

    def outline(s):
        add = {}
        for (x, y), c in s.p.items():
            if c == 'K':
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                q = (x + dx, y + dy)
                if q not in s.p and 0 <= q[0] < 16 and 0 <= q[1] < 16:
                    add[q] = 'K'
        s.p.update(add)
        return s

    def svg(s):
        by = {}
        for y in range(16):
            x = 0
            while x < 16:
                c = s.p.get((x, y))
                if c is None:
                    x += 1
                    continue
                x2 = x
                while x2 + 1 < 16 and s.p.get((x2 + 1, y)) == c:
                    x2 += 1
                w = x2 - x + 1
                by.setdefault(c, []).append(f'M{x} {y}h{w}v1h-{w}z')
                x = x2 + 1
        paths = ''.join(f'<path fill="{PAL[c]}" d="{"".join(d)}"/>' for c, d in by.items())
        return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16" '
                f'shape-rendering="crispEdges">{paths}</svg>')


def server():
    """A globe: blue ball, green continents, white glint."""
    c = C()
    c.circ(7.5, 7.5, 7, 'B')
    c.pat(3, 3, ['.LLL.', 'LLLL.', '.LL..', '..L..'])
    c.pat(9, 6, ['.LL.', 'LLLL', '.LL.'])
    c.pat(5, 11, ['.LLL'])
    for y in range(16):
        for x in range(16):
            if math.hypot(x - 7.5, y - 7.5) > 5.8 and (x + y) > 15:
                if (x, y) in c.p:
                    c.p[(x, y)] = 'N'
    c.set(4, 3, 'b'); c.set(5, 2, 'b'); c.set(3, 4, 'b')
    return c.outline()


def channel():
    """A hash sign on a coloured tile."""
    c = C()
    c.rect(1, 1, 14, 14, 'N'); c.rect(1, 1, 14, 1, 'B'); c.rect(1, 1, 1, 14, 'B')
    c.rect(5, 3, 6, 12, 'w'); c.rect(9, 3, 10, 12, 'w')
    c.rect(3, 6, 12, 7, 'w'); c.rect(3, 9, 12, 10, 'w')
    return c.outline()


def users():
    """Two heads and shoulders."""
    c = C()
    c.circ(10.2, 4.8, 2.6, 'O'); c.rect(7, 8, 14, 14, 'y'); c.rect(8, 8, 13, 9, 'o')
    c.circ(5.2, 5.6, 2.9, 'Y'); c.rect(1, 9, 9, 14, 'R'); c.rect(2, 9, 8, 10, 'r')
    c.set(4, 5, 'W'); c.set(5, 4, 'W')
    return c.outline()


def bubble():
    """A speech bubble with three dots."""
    c = C()
    c.rect(1, 2, 14, 10, 'w'); c.rect(1, 2, 14, 2, 'W'); c.rect(14, 3, 14, 10, 'g'); c.rect(1, 10, 14, 10, 'g')
    c.rect(3, 11, 6, 11, 'g'); c.rect(3, 12, 4, 12, 'g'); c.set(3, 13, 'g')
    for x in (4, 7, 10):
        c.rect(x, 5, x + 1, 6, 'R')
    return c.outline()


def book():
    """The address book: red cover, page edge, a tab."""
    c = C()
    c.rect(2, 0, 13, 14, 'R'); c.rect(2, 0, 2, 14, 'r'); c.rect(13, 1, 13, 14, 'r'); c.rect(2, 14, 13, 14, 'r')
    c.rect(4, 2, 11, 12, 'W'); c.rect(4, 2, 4, 12, 'g')
    c.rect(6, 4, 10, 4, 'd'); c.rect(6, 6, 10, 6, 'd'); c.rect(6, 8, 9, 8, 'd'); c.rect(6, 10, 10, 10, 'd')
    c.rect(14, 3, 15, 5, 'Y'); c.rect(14, 8, 15, 10, 'O')
    return c.outline()


def terminal():
    """Console screen: black, green prompt."""
    c = C()
    c.rect(0, 1, 15, 14, 'g'); c.rect(0, 13, 15, 14, 'G')
    c.rect(2, 3, 13, 11, 'K'); c.rect(2, 3, 13, 3, 'D')
    c.line(3, 5, 5, 7, 'L'); c.line(5, 7, 3, 9, 'L'); c.rect(7, 9, 10, 9, 'L')
    return c.outline()


def plug():
    """A plug: connect."""
    c = C()
    c.rect(3, 0, 3, 4, 'g'); c.rect(8, 0, 8, 4, 'g')
    c.rect(1, 5, 10, 9, 'G'); c.rect(1, 5, 10, 5, 'g'); c.rect(1, 9, 10, 9, 'd')
    c.rect(4, 10, 7, 11, 'd'); c.rect(5, 12, 6, 15, 'D')
    c.rect(11, 6, 14, 8, 'L'); c.set(14, 7, 'l')
    return c.outline()


def unplug():
    """A plug broken in two halves: disconnect."""
    c = C()
    c.rect(1, 1, 1, 4, 'g'); c.rect(5, 1, 5, 4, 'g')
    c.rect(0, 5, 6, 8, 'G'); c.rect(0, 5, 6, 5, 'g'); c.rect(2, 9, 4, 10, 'd')
    c.rect(9, 7, 15, 9, 'G'); c.rect(9, 7, 15, 7, 'g'); c.rect(10, 10, 11, 12, 'd')
    c.line(7, 12, 9, 14, 'R'); c.line(8, 12, 10, 14, 'R')
    return c.outline()


def door():
    """Quit: an open door."""
    c = C()
    c.rect(2, 1, 9, 14, 'o'); c.rect(2, 1, 2, 14, 'O'); c.rect(3, 2, 8, 13, 'y')
    c.rect(4, 3, 7, 6, 'o'); c.rect(4, 8, 7, 12, 'o'); c.set(7, 8, 'Y')
    c.rect(10, 6, 14, 8, 'R'); c.set(13, 5, 'R'); c.set(13, 9, 'R'); c.set(12, 4, 'R'); c.set(12, 10, 'R')
    return c.outline()


def info():
    """About: a blue disc with an 'i'."""
    c = C()
    c.circ(7.5, 7.5, 7, 'B'); c.circ(7.5, 7.5, 6, 'B')
    c.rect(7, 3, 8, 4, 'w'); c.rect(6, 6, 8, 6, 'w'); c.rect(7, 7, 8, 11, 'w'); c.rect(6, 11, 9, 11, 'w')
    c.set(4, 4, 'b'); c.set(5, 3, 'b')
    return c.outline()


def gear():
    c = C()
    for y in range(16):
        for x in range(16):
            dx, dy = x - 7.5, y - 7.5
            r = math.hypot(dx, dy)
            a = math.degrees(math.atan2(dy, dx)) % 45
            tooth = r <= 7.4 and (a <= 14 or a >= 31)
            if r <= 5.4 or tooth:
                c.set(x, y, 'Y' if (x + y) < 15 else 'O')
            if r <= 2.4:
                c.p.pop((x, y), None)
    return c.outline()


def key():
    """Authentication: a key."""
    c = C()
    c.circ(4.5, 4.5, 3.6, 'Y'); c.circ(4.5, 4.5, 1.4, 'K')
    c.rect(7, 7, 14, 8, 'Y'); c.rect(7, 7, 14, 7, 'W'); c.rect(11, 9, 12, 11, 'y'); c.rect(14, 9, 14, 12, 'y')
    return c.outline()


def moon():
    """Away: a crescent."""
    c = C()
    c.circ(7.5, 7.5, 6.5, 'Y')
    c.circ(10.5, 5.5, 5.2, 'K')
    for k in list(c.p):
        if c.p[k] == 'K':
            del c.p[k]
    c.set(3, 5, 'W'); c.set(2, 7, 'W'); c.set(3, 9, 'W'); c.set(4, 11, 'W')
    return c.outline()


def journal():
    c = C()
    c.rect(3, 0, 12, 14, 'w')
    for x, y in ((10, 0), (11, 0), (12, 0), (11, 1), (12, 1), (12, 2)):
        c.p.pop((x, y), None)
    c.rect(9, 1, 10, 3, 'g'); c.rect(11, 3, 12, 3, 'g')
    c.rect(4, 5, 10, 5, 'd'); c.rect(4, 7, 11, 7, 'd'); c.rect(4, 9, 9, 9, 'd'); c.rect(4, 11, 11, 11, 'd')
    c.rect(12, 4, 12, 14, 'g'); c.rect(3, 14, 12, 14, 'G')
    return c.outline()


def bell():
    """Notifications."""
    c = C()
    c.rect(7, 0, 8, 1, 'y')
    c.rect(5, 2, 10, 3, 'Y'); c.rect(4, 4, 11, 8, 'Y'); c.rect(3, 9, 12, 11, 'Y'); c.rect(1, 11, 14, 12, 'Y')
    c.rect(5, 3, 5, 9, 'W'); c.rect(11, 5, 11, 11, 'y'); c.rect(1, 12, 14, 12, 'y')
    c.rect(6, 13, 9, 14, 'o')
    return c.outline()


def pencil():
    """Edit."""
    c = C()
    c.line(2, 13, 11, 4, 'Y'); c.line(3, 13, 12, 4, 'Y'); c.line(2, 12, 11, 3, 'y'); c.line(4, 13, 13, 4, 'y')
    c.rect(12, 2, 14, 4, 'R'); c.set(11, 4, 'R'); c.set(13, 5, 'r')
    c.set(1, 14, 'K'); c.set(2, 14, 'd'); c.set(1, 13, 'd'); c.set(2, 13, 'W')
    return c.outline()


def trash():
    """Delete."""
    c = C()
    c.rect(2, 2, 13, 3, 'g'); c.rect(6, 0, 9, 1, 'G')
    c.rect(3, 4, 12, 14, 'g'); c.rect(3, 4, 3, 14, 'w'); c.rect(12, 5, 12, 14, 'G'); c.rect(3, 14, 12, 14, 'G')
    for x in (5, 8, 10):
        c.rect(x, 6, x, 12, 'd')
    return c.outline()


def kirc_app():
    """The application icon: a red chat bubble holding a hash sign."""
    c = C()
    c.rect(1, 1, 14, 11, 'R'); c.rect(1, 1, 14, 1, 'O'); c.rect(1, 1, 1, 11, 'O')
    c.rect(14, 2, 14, 11, 'r'); c.rect(2, 11, 14, 11, 'r')
    c.rect(3, 12, 6, 12, 'r'); c.rect(3, 13, 4, 13, 'r'); c.set(3, 14, 'r')
    c.rect(5, 3, 5, 9, 'w'); c.rect(9, 3, 9, 9, 'w'); c.rect(3, 5, 12, 5, 'w'); c.rect(3, 7, 12, 7, 'w')
    return c.outline()


ICONS = dict(serveur=server, salon=channel, membres=users, message=bubble, carnet=book, console=terminal,
             connecter=plug, deconnecter=unplug, quitter=door, info=info, reglages=gear, cle=key, absent=moon,
             journal=journal, cloche=bell, editer=pencil, supprimer=trash)


def all_svgs():
    return {name: draw().svg() for name, draw in ICONS.items()}
