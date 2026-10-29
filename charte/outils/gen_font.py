"""The two bitmap fonts of the KIRC interface, built pixel by pixel.

KDX draws its windows with two bitmap fonts: a larger one for buttons, labels and titles (capitals 9 px high,
x-height 7, 2 px stems, letters two pixels apart) and a smaller one for chat transcripts (capitals 8 px, x-height 6,
letters one pixel apart). Both are rebuilt here as TrueType outlines made of whole pixels, so that at `font-size: 14px`
(100 font units per pixel, 14 pixels to the em) every font pixel is exactly one CSS pixel. They are colour fonts (COLR),
because only those are drawn without the half-pixel blur DirectWrite puts on ordinary outlines:

    KIRC Pixel      interface font
    KIRC Pixel Log  transcript font

The letter shapes in font_glyphs.py were read off screenshots of KDX 1.6 (its documentation and a running client).
What the screenshots do not show (a few capitals, punctuation, accents, the transcript's capitals and digits) is drawn
here in the same style, and the transcript letters that were not seen are derived from the interface ones.

    python tools/gen_font.py        writes tools/kirc-pixel*.ttf (to look at) and prints the glyph counts

tools/gen_assets.py embeds both in ui/fonts.css. Needs fontTools and brotli (pip install fonttools brotli).
"""
import io
from pathlib import Path

import font_glyphs

PX = 100          # font units per pixel
EM = 14           # pixels per em
TOP = 11          # rows above the baseline (rows 0 .. 10); rows 11 .. 13 are the descenders

UI = {}           # char -> (first row, [row strings])
LOG = {}


def g(ch, rows, top):
    rows = rows.split() if isinstance(rows, str) else list(rows)
    w = max(len(r) for r in rows)
    assert all(len(r) == w for r in rows), (ch, [len(r) for r in rows])
    assert set("".join(rows)) <= set("#."), ch
    UI[ch] = (top, rows)


# ------------------------------------------------ interface font: what the screenshots do not show
g("O", "..#####. .##...## ##....## ##....## ##....## ##....## ##....## .##...## ..#####.", 2)
g("Q", "..#####. .##...## ##....## ##....## ##....## ##....## ##..#.## .##..##. ..######", 2)
g("W", "##......## ##......## ##......## ##..##..## ##..##..## ##.####.## .###..###. .###..###. .##....##.", 2)
g("Y", "##....## .##..##. .##..##. ..####.. ...##... ...##... ...##... ...##... ...##...", 2)
g("Z", "####### .....## ....##. ...##.. ..##... .##.... ##..... ##..... #######", 2)
g("j", "...## ...## ..... ...## ...## ...## ...## ...## ...## ...## ...## ...## .####", 1)
g("q", ".###### ##...## ##...## ##...## ##...## ##..### .###.## .....## .....## .....##", 4)
g(" ", ["...."], 0)
g("!", "## ## ## ## ## ## .. ## ##", 2)
g('"', "##.## ##.## ##.##", 2)
g("#", ".##.##. .##.##. ####### .##.##. .##.##. ####### .##.##. .##.##.", 2)
g("$", "...#... .#####. ##.#..# ##.#... .#####. ...#.## #..#.## .#####. ...#...", 2)
g("&", ".###... ##.##.. ##.##.. .###... ####.## ##.#### ##..### ##..##. .###.##", 2)
g("'", "## ## #.", 2)
g("*", "...#... #.###.# .#####. ..###.. .#####. #.###.# ...#...", 3)
g("+", "...##... ...##... ........ ######## ...##... ...##...", 4)
g("-", "######", 7)
g("/", ".....## ....##. ...##.. ..##... .##.... ##.....", 4)
g(";", "##. ##. ... ... .## .## ##.", 5)
g("<", "...## ..##. .##.. ##... .##.. ..##. ...##", 4)
g("=", "####### ....... #######", 6)
g(">", "##... .##.. ..##. ...## ..##. .##.. ##...", 4)
g("?", ".#####. ##...## .....## ....##. ..###.. ..##... ....... ..##...", 2)
g("@", ".#####. ##...## ##.#### ##.#.## ##.#### ##..... .######", 2)
g("[", "#### ##.. ##.. ##.. ##.. ##.. ##.. ##.. ####", 2)
g("\\", "##..... .##.... ..##... ...##.. ....##. .....##", 4)
g("]", "#### ..## ..## ..## ..## ..## ..## ..## ####", 2)
g("^", "..##... .##.##. ##...##", 2)
g("_", "#######", 12)
g("`", "##.. .##.", 2)
g("{", "..## .##. .##. ##.. .##. .##. ..##", 3)
g("|", "## ## ## ## ## ## ## ## ## ## ##", 2)
g("}", "##.. .##. .##. ..## .##. .##. ##..", 3)
g("~", ".##..## ##.##..", 6)
g("…", ["##.##.##", "##.##.##"], 9)
g("·", "## ##", 7)
g("—", "##########", 7)
g("–", "######", 7)
g("‘", ".## ##. #..", 2)
g("’", ".## .## ##.", 2)
g("“", ".##.## ##.##. #..#..", 2)
g("”", ".##.## .##.## ##.##.", 2)
g("«", "..#..# .#..#. #..#.. .#..#. ..#..#", 5)
g("»", "#..#.. .#..#. ..#..# .#..#. #..#..", 5)
g("°", ".##. #..# #..# .##.", 2)
g("€", "..#####. .##...## ####.... .##...## ..#####.", 3)
g("©", ".#####. #.###.# #.#...# #.###.# .#####.", 2)
g("×", "##...## .##.##. ..###.. .##.##. ##...##", 5)
g("¿", "..##... ....... ..##... .##.... ##..... ##...## .#####.", 2)
g("¡", "## .. ## ## ## ## ## ## ##", 2)
g("œ", [".####.####.", "##..###..##", "##..#######", "##..##.....", "##..###..##", ".####.####."], 4)
g("Œ", [".#####.#####", "##...###....", "##...##.....", "##...#####..", "##...##.....", "##...##.....", "##...##.....", "##...###....", ".#####.#####"], 2)
g("æ", [".####.####.", ".....##..##", ".##########", "##...##....", "##..###..##", ".###.#####."], 4)
g("Æ", ["..#######", ".###.....", "##.##....", "##.#####.", "########.", "##..##...", "##..##...", "##..##...", "##..#####"], 2)

# what the screenshots show wins over the drawings above
UI.update({ch: (top, list(rows)) for ch, (top, rows) in font_glyphs.UI.items()})


# ------------------------------------------------ the transcript font: seen letters, then reduced interface letters
def shrink(top, rows):
    """Reduces an interface glyph to the transcript scale: a row and a column fewer in the middle."""
    rows = list(rows)
    if top == 1 and len(rows) >= 10:      # ascender: loses a stem row, and a row of its body
        rows.pop(0)
        rows.pop(4)
        return _cols(3, rows)
    if top == 4 and len(rows) >= 10:      # letter with a tail
        body, tail = rows[:7], rows[7:]
        body.pop(2)
        tail.pop(0)
        return _cols(5, body + tail)
    if top == 4 and len(rows) == 7:       # plain x-height letter
        rows.pop(2)
        return _cols(5, rows)
    if top == 2 and len(rows) == 9:       # capital or digit
        rows.pop(3)
        return _cols(3, rows)
    return _cols(top, rows)


def _cols(top, rows):
    drop = {7: [3], 8: [3, 4], 9: [3, 4], 10: [3, 4, 5, 6], 11: [3, 4, 5, 6], 12: [3, 4, 5, 6, 7, 8]}.get(len(rows[0]), [])
    return top, ["".join(c for i, c in enumerate(r) if i not in drop) for r in rows]


for ch, (top, rows) in UI.items():
    LOG[ch] = (top, list(rows)) if ch in " …·—–" else shrink(top, rows)


def lg(ch, rows, top):
    rows = rows.split() if isinstance(rows, str) else list(rows)
    assert len({len(r) for r in rows}) == 1, ch
    LOG[ch] = (top, rows)


# the transcript's capitals, digits and the few letters that were not seen, drawn like the ones that were
for ch, rows in {
    "A": ".####. ##..## ##..## ##..## ###### ##..## ##..## ##..##",
    "B": "#####. ##..## ##..## #####. ##..## ##..## ##..## #####.",
    "D": "#####. ##..## ##..## ##..## ##..## ##..## ##..## #####.",
    "E": "###### ##.... ##.... #####. ##.... ##.... ##.... ######",
    "G": ".####. ##..## ##.... ##.... ##.### ##..## ##..## .####.",
    "H": "##..## ##..## ##..## ###### ##..## ##..## ##..## ##..##",
    "I": "#### .##. .##. .##. .##. .##. .##. ####",
    "J": "...### ....## ....## ....## ....## ##..## ##..## .####.",
    "K": "##..## ##.##. ####.. ###... ####.. ##.##. ##..## ##..##",
    "M": "##...## ###.### ####### ##.#.## ##...## ##...## ##...## ##...##",
    "N": "##..## ###.## ###### ##.### ##..## ##..## ##..## ##..##",
    "O": ".####. ##..## ##..## ##..## ##..## ##..## ##..## .####.",
    "P": "#####. ##..## ##..## ##..## #####. ##.... ##.... ##....",
    "Q": ".####. ##..## ##..## ##..## ##..## ##.### ##..## .#####",
    "R": "#####. ##..## ##..## #####. ####.. ##.##. ##..## ##..##",
    "S": ".##### ##.... ##.... .####. ....## ....## ....## #####.",
    "T": "###### ..##.. ..##.. ..##.. ..##.. ..##.. ..##.. ..##..",
    "U": "##..## ##..## ##..## ##..## ##..## ##..## ##..## .####.",
    "V": "##..## ##..## ##..## ##..## .####. .####. ..##.. ..##..",
    "W": "##....## ##....## ##....## ##.##.## ##.##.## ######## .##..##. .##..##.",
    "X": "##..## ##..## .####. ..##.. ..##.. .####. ##..## ##..##",
    "Y": "##..## ##..## .####. ..##.. ..##.. ..##.. ..##.. ..##..",
    "Z": "###### ....## ...##. ..##.. .##... ##.... ##.... ######",
    "0": ".####. ##..## ##.### ###### ###.## ##..## ##..## .####.",
    "1": "..##.. .###.. ..##.. ..##.. ..##.. ..##.. ..##.. ######",
    "2": ".####. ##..## ....## ...##. ..##.. .##... ##.... ######",
    "3": ".####. ##..## ....## ..###. ....## ....## ##..## .####.",
    "4": "...##. ..###. .####. ##.##. ###### ...##. ...##. ...##.",
    "5": "###### ##.... ##.... #####. ....## ....## ##..## .####.",
    "6": ".####. ##.... ##.... #####. ##..## ##..## ##..## .####.",
    "7": "###### ....## ...##. ...##. ..##.. ..##.. .##... .##...",
    "8": ".####. ##..## ##..## .####. ##..## ##..## ##..## .####.",
    "9": ".####. ##..## ##..## .##### ....## ....## ##..## .####.",
    "?": ".####. ##..## ....## ...##. ..##.. ..##.. ...... ..##..",
    "b": "##.... ##.... #####. ##..## ##..## ##..## ###.## #####.",
    "k": "##.... ##.... ##..## ##.##. ####.. ##.##. ##..## ##..##",
}.items():
    lg(ch, rows, 3)
for ch, rows in {"v": "##..## ##..## ##..## .####. .####. ..##..", "x": "##..## ##..## .####. .####. ##..## ##..##"}.items():
    lg(ch, rows, 5)
LOG.update({ch: (top, list(rows)) for ch, (top, rows) in font_glyphs.LOG.items()})

# ------------------------------------------------ accents, composed over the base letters
MARKS = {"acute": ["..##", ".##."], "grave": ["##..", ".##."], "circ": [".##.", "#..#"], "diae": ["##.##", "##.##"]}


def accent(table, new, base, mark, cap=False, dotless=False):
    top, rows = table[base]
    if dotless:  # i without its dot: the stem alone
        cut = 3 if table is UI else 2
        rows, top = rows[cut:], top + cut
    w = len(rows[0])
    m = MARKS[mark]
    mw = len(m[0])
    width = max(w, mw)
    lpad, mpad = (width - w) // 2, (width - mw) // 2
    pad = lambda r, p: "." * p + r + "." * (width - len(r) - p)
    mrow = top - (2 if cap else 3)
    out = {}
    for i, r in enumerate(m):
        out[mrow + i] = pad(r, mpad)
    for i, r in enumerate(rows):
        out[top + i] = pad(r, lpad)
    first, last = min(out), max(out)
    table[new] = (first, [out.get(y, "." * width) for y in range(first, last + 1)])


LOWER = [("à", "a", "grave"), ("â", "a", "circ"), ("ä", "a", "diae"),
         ("é", "e", "acute"), ("è", "e", "grave"), ("ê", "e", "circ"), ("ë", "e", "diae"),
         ("ô", "o", "circ"), ("ö", "o", "diae"),
         ("ù", "u", "grave"), ("û", "u", "circ"), ("ü", "u", "diae"), ("ÿ", "y", "diae")]
UPPER = [("À", "A", "grave"), ("Â", "A", "circ"), ("Ä", "A", "diae"),
         ("É", "E", "acute"), ("È", "E", "grave"), ("Ê", "E", "circ"), ("Ë", "E", "diae"),
         ("Î", "I", "circ"), ("Ï", "I", "diae"), ("Ô", "O", "circ"), ("Ö", "O", "diae"),
         ("Ù", "U", "grave"), ("Û", "U", "circ"), ("Ü", "U", "diae")]
for table in (UI, LOG):
    for ch, base, mark in LOWER:
        accent(table, ch, base, mark)
    accent(table, "î", "i", "circ", dotless=True)
    accent(table, "ï", "i", "diae", dotless=True)
    for ch, base, mark in UPPER:
        accent(table, ch, base, mark, cap=True)
    for ch, base in (("ç", "c"), ("Ç", "C")):  # cedilla
        top, rows = table[base]
        w = len(rows[0])
        table[ch] = (top, rows + [".##" + "." * (w - 3), "##." + "." * (w - 3)])

def italic(table):
    """A slanted copy: the upper rows move right by one pixel per three rows of height (at most two), like a hand-made
    pixel italic. The right edge is padded by two pixels, which the advance does not count."""
    out = {}
    for ch, (top, rows) in table.items():
        new = []
        for i, r in enumerate(rows):
            h = TOP - 1 - (top + i)          # rows above the baseline row
            shift = min(max(h, 0) // 3, 2)
            new.append("." * shift + r + "." * (2 - shift))
        out[ch] = (top, new)
    return out


LOG_ITALIC = italic(LOG)

FACES = {
    # family: (table, pixels between letters, width of a space, columns of padding that the advance ignores)
    "KIRC Pixel": (UI, 2, 4, 0),
    "KIRC Pixel Log": (LOG, 1, 7, 0),
    "KIRC Pixel Log Italic": (LOG_ITALIC, 1, 7, 2),
}


def rects(top, rows):
    """Merges the lit pixels into rectangles (runs in a row, joined with the same run in the row below)."""
    runs = []
    for y, row in enumerate(rows):
        x = 0
        while x < len(row):
            if row[x] == "#":
                x0 = x
                while x < len(row) and row[x] == "#":
                    x += 1
                runs.append([x0, x, top + y, top + y + 1])
            else:
                x += 1
    merged = []
    for r in runs:
        for m in merged:
            if m[0] == r[0] and m[1] == r[1] and m[3] == r[2]:
                m[3] = r[3]
                break
        else:
            merged.append(r)
    return merged


def build(family):
    """The font as a colour font (COLR v0): each letter is an empty base glyph with one layer, painted in the text colour
    (palette entry 0xFFFF). Chromium draws such layers as plain vector shapes, so a pixel lands exactly on a screen
    pixel; ordinary glyphs go through DirectWrite, which blurs every stem by half a pixel."""
    from fontTools.colorLib.builder import buildCOLR, buildCPAL
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    table, spacing, space, trim = FACES[family]
    chars = sorted(table)
    base = ["uni%04X" % ord(c) for c in chars]
    fb = FontBuilder(EM * PX, isTTF=True)
    fb.setupGlyphOrder([".notdef"] + base + [n + ".l" for n in base])
    cmap = {ord(c): "uni%04X" % ord(c) for c in chars}
    cmap[0xA0] = "uni0020"
    fb.setupCharacterMap(cmap)
    glyphs, metrics = {}, {}
    glyphs[".notdef"] = TTGlyphPen(None).glyph()
    metrics[".notdef"] = (700, 0)
    for c in chars:
        top, rows = table[c]
        name = "uni%04X" % ord(c)
        advance = (space if c == " " else len(rows[0]) - trim + spacing) * PX
        glyphs[name] = TTGlyphPen(None).glyph()
        pen = TTGlyphPen(None)
        for x0, x1, r0, r1 in rects(top, rows):
            xa, xb = x0 * PX, x1 * PX
            yt, yb = (TOP - r0) * PX, (TOP - r1) * PX
            pen.moveTo((xa, yb)); pen.lineTo((xa, yt)); pen.lineTo((xb, yt)); pen.lineTo((xb, yb)); pen.closePath()
        glyphs[name + ".l"] = pen.glyph()
        metrics[name] = metrics[name + ".l"] = (advance, 0)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=TOP * PX, descent=-(EM - TOP) * PX)
    fb.setupNameTable({"familyName": family, "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=TOP * PX, sTypoDescender=-(EM - TOP) * PX, sTypoLineGap=0,
                usWinAscent=TOP * PX, usWinDescent=(EM - TOP) * PX, sxHeight=7 * PX, sCapHeight=9 * PX, fsType=0)
    fb.setupPost(isFixedPitch=0)
    font = fb.font
    font["COLR"] = buildCOLR({n: [(n + ".l", 0xFFFF)] for n in base})
    font["CPAL"] = buildCPAL([[(1, 1, 1, 1)]])
    return font


def to_bytes(family, flavor=None):
    font = build(family)
    if flavor:
        font.flavor = flavor
    buf = io.BytesIO()
    font.save(buf)
    return buf.getvalue()


if __name__ == "__main__":
    here = Path(__file__).resolve().parent
    for family, (table, *_) in FACES.items():
        out = here / (family.lower().replace(" ", "-") + ".ttf")
        out.write_bytes(to_bytes(family))
        print(family, len(table), "glyphs ->", out.name)
