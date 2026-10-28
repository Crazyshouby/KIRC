# Publier une version

La version vit à **un seul endroit** : `version` dans `src-tauri/Cargo.toml`. Tauri, l'installateur et la CI la lisent de là ; la CI refuse un tag qui ne correspond pas.

1. Mettre `version` à jour dans `src-tauri/Cargo.toml` (SemVer), dater `CHANGELOG.md`.
2. `python tools/gen_assets.py` et `python tools/make_notices.py` si les polices, pictogrammes ou dépendances ont changé (la CI vérifie que les fichiers générés sont à jour).
3. Fusionner dans `main` par pull request ; la CI doit être verte (clippy, tests, installateur, test de bout en bout).
4. Recette manuelle sur l'installateur de la CI : installation, connexion à un vrai serveur (Libera.Chat en TLS), `/join`, message privé, notification, déplacement/enroulement/fermeture des fenêtres, redémarrage (positions retrouvées), désinstallation. Écran à échelle d'affichage différente de 100 % et deux écrans.
5. `git tag v0.1.0 && git push origin v0.1.0` : la CI publie la release (un tag contenant `-` est une préversion).

L'exécutable n'est pas signé. La même démarche que pour AudioLinker (SignPath Foundation, projet MIT public) s'applique : voir `RELEASING.md` d'AudioLinker.
