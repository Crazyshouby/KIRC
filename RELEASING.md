# Publier une version

La version vit à **un seul endroit** : `version` dans `src-tauri/Cargo.toml`. Tauri, l'installateur et la CI la lisent de là ; la CI refuse un tag qui ne correspond pas.

1. Mettre `version` à jour dans `src-tauri/Cargo.toml` (SemVer), dater `CHANGELOG.md`.
2. `python tools/gen_assets.py` et `python tools/make_notices.py` si les polices, pictogrammes ou dépendances ont changé (la CI vérifie que les fichiers générés sont à jour).
3. Fusionner dans `main` par pull request ; la CI doit être verte (clippy, tests, installateur, test de bout en bout).
4. Recette manuelle sur l'installateur de la CI : installation, connexion à un vrai serveur (Libera.Chat en TLS), `/join`, message privé, notification, déplacement/enroulement/fermeture des fenêtres, redémarrage (positions retrouvées), désinstallation. Écran à échelle d'affichage différente de 100 % et deux écrans.
5. `git tag v0.1.0 && git push origin v0.1.0` : la CI publie la release (un tag contenant `-` est une préversion).

L'exécutable n'est pas signé. La même démarche que pour AudioLinker (SignPath Foundation, projet MIT public) s'applique : voir `RELEASING.md` d'AudioLinker.

## Mises à jour automatiques et clé de signature

Les copies installées cherchent `latest.json` dans la dernière release GitHub et n'installent qu'un installateur dont la **signature** correspond à la clé publique inscrite dans `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). Une release faite par quelqu'un qui n'a pas la clé privée est refusée par toutes les copies.

- **Clé privée** : un fichier `kirc.key` généré avec `npx tauri signer generate --ci -w <fichier>`, **gardé hors du dépôt** (sauvegarde personnelle, par exemple un gestionnaire de mots de passe). Elle est aussi dans le secret GitHub **`TAURI_SIGNING_PRIVATE_KEY`** du dépôt (`gh secret set TAURI_SIGNING_PRIVATE_KEY < kirc.key`). La CI signe l'installateur avec, et produit `latest.json` (`tools/make_latest_json.py`).
- **Si la clé est perdue ou divulguée** : générer une nouvelle paire, changer `pubkey`, publier une version. Les copies déjà installées, qui ne connaissent que l'ancienne clé, ne pourront plus se mettre à jour toutes seules : il faudra les réinstaller à la main. D'où l'importance de la sauvegarde.
- **Sans le secret** (pull request d'un fork, ou secret absent) : la CI construit l'installateur sans signature d'update, et la release n'a pas de `latest.json` (aucune mise à jour automatique pour cette version).
- Une préversion (tag avec `-`) n'est jamais « la dernière release » : elle n'est jamais proposée.
- Ce que ça ne remplace pas : la signature de code Windows (SmartScreen). Elle reste à faire (voir plus haut).
