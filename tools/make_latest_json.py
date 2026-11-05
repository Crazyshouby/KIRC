"""Writes latest.json, the manifest the installed application reads to find the newest release.

    python tools/make_latest_json.py <dist-dir> <tag> <owner/repo>

<dist-dir> holds the installer, its .sig (made by `tauri build` when createUpdaterArtifacts is on and
the private key is set) and, optionally, nothing else of interest. Without a .sig nothing is written:
a release without a signature offers no automatic update. The notes are the CHANGELOG.md section of
the version.
"""
import datetime
import glob
import json
import os
import re
import sys


def main(dist, tag, repo):
    sigs = glob.glob(os.path.join(dist, "*.sig"))
    if not sigs:
        print("Pas de signature : aucune mise à jour automatique pour cette version")
        return 0
    if len(sigs) != 1:
        print("Plusieurs signatures dans", dist, file=sys.stderr)
        return 1
    version = tag[1:] if tag.startswith("v") else tag
    setup = os.path.basename(sigs[0])[:-4]
    changelog = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "CHANGELOG.md")
    notes = ""
    if os.path.exists(changelog):
        text = open(changelog, encoding="utf-8").read()
        m = re.search(r"^## " + re.escape(version) + r"\b[^\n]*\n(.*?)(?=^## |\Z)", text, re.S | re.M)
        notes = m.group(1).strip()[:1500] if m else ""
    manifest = {
        "version": version,
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "windows-x86_64": {
                "signature": open(sigs[0], encoding="utf-8").read().strip(),
                "url": f"https://github.com/{repo}/releases/download/{tag}/{setup}",
            }
        },
    }
    with open(os.path.join(dist, "latest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
    print(json.dumps(manifest, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)
    sys.exit(main(*sys.argv[1:]))
