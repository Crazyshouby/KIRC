"""Regenerates THIRD-PARTY-NOTICES.txt (run from the repo root).

    python tools/make_notices.py

Lists the Rust crates that end up in the Windows build (name, version, licence, home page, from
`cargo metadata`) and reproduces the full text of the font licences.
"""
import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent

HEAD = """MENTIONS LÉGALES ET LICENCES DES COMPOSANTS TIERS
=================================================

KIRC inclut ou utilise les composants suivants.

L'interface reprend l'apparence du logiciel KDX (Haxial). Aucune ressource de KDX (image, police,
fichier d'apparence) n'est reprise : tout est redessiné. KDX est une marque de Haxial ; KIRC n'est
lié à Haxial d'aucune façon.

------------------------------------------------------------------------------
KIRC
------------------------------------------------------------------------------
KIRC est distribué sous licence MIT (fichier LICENSE).

"""


def crates():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc",
         "--manifest-path", str(root / "src-tauri" / "Cargo.toml")],
        capture_output=True, encoding="utf-8", check=True).stdout
    meta = json.loads(out)
    reachable = {n["id"] for n in meta["resolve"]["nodes"]}
    rows = []
    for p in meta["packages"]:
        if p["id"] not in reachable or p["name"] == "kirc":
            continue
        rows.append((p["name"], p["version"], p.get("license") or "(voir le dépôt)", p.get("repository") or p.get("homepage") or ""))
    return sorted(set(rows))


def main():
    parts = [HEAD]
    parts.append("------------------------------------------------------------------------------\n"
                 "Polices\n"
                 "------------------------------------------------------------------------------\n"
                 "Archivo (Omnibus-Type) et Overpass Mono (Delve Withrington, Red Hat), sous licence SIL Open Font\n"
                 "License 1.1, intégrées à l'interface sous forme de sous-ensembles latins.\n\n")
    for name in ("OFL-Archivo.txt", "OFL-OverpassMono.txt"):
        parts.append(f"--- {name} ---\n")
        parts.append((root / "third_party" / "fonts" / name).read_text(encoding="utf-8").strip() + "\n\n")
    rows = crates()
    parts.append("------------------------------------------------------------------------------\n"
                 f"Bibliothèques Rust ({len(rows)}) liées à la version Windows\n"
                 "------------------------------------------------------------------------------\n"
                 "Nom, version, licence (expression SPDX), dépôt. Le texte de chaque licence se trouve dans le\n"
                 "dépôt de la bibliothèque et dans son paquet sur https://crates.io. Celles sous MPL-2.0 sont\n"
                 "utilisées sans modification : leur code source est disponible à l'adresse indiquée.\n\n")
    for name, ver, lic, repo in rows:
        parts.append(f"{name} {ver} — {lic}" + (f" — {repo}" if repo else "") + "\n")
    parts.append("\nWebView2 (Microsoft) est fourni par Windows ou installé par l'installateur ; il reste soumis à ses propres conditions.\n")
    (root / "THIRD-PARTY-NOTICES.txt").write_text("".join(parts), encoding="utf-8")
    print(f"{len(rows)} crates")


if __name__ == "__main__":
    sys.exit(main())
