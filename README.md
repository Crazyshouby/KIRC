# KIRC

Un client IRC pour Windows, dans le style de **KDX** (Haxial) : chaque fonction est sa propre fenêtre sans bordure, avec le cadre gris et rouge, les boutons biseautés, les champs noirs à texte vert et les pictogrammes en pixels de l'original. Même univers visuel qu'[AudioLinker](https://github.com/Crazyshouby/AudioLinker).

Écrit avec [Tauri 2](https://tauri.app) (Rust + WebView2). Interface en français.

## Fonctions

- **Plusieurs serveurs** à la fois, dans un carnet d'adresses (modèles Libera.Chat, OFTC, Rizon, IRCnet).
- **TLS** (Schannel, TLS 1.2 minimum) ; un certificat non valide est refusé, sauf choix explicite pour un serveur personnel.
- **Authentification** : SASL PLAIN, ou NickServ en repli ; mot de passe du serveur. Les mots de passe sont gardés dans le **coffre de Windows**, jamais dans un fichier.
- **Salons** et **messages privés**, chacun dans sa fenêtre ; liste des membres colorée par statut (propriétaire, administrateur, opérateur, demi-opérateur, voix, absent) ; sujet modifiable par double-clic ; complétion des pseudos avec Tab ; historique de saisie avec les flèches.
- **Fenêtre des messages du serveur** (console) pour chaque connexion.
- Couleurs et styles mIRC, liens cliquables (http et https seulement), actions `/me`, CTCP (VERSION, PING, TIME).
- **Commandes** : `/join /part /msg /query /me /nick /away /whois /list /ignore /unignore /topic /kick /ban /op /deop /voice /devoice /mode /invite /names /ctcp /ns /cs /quote /clear /quit` — `/aide` les rappelle.
- **Notifications** de bureau pour les messages privés et les mentions, désactivables ; compteurs de lignes non lues dans le menu principal et la barre de titre.
- **Journaux** de conversation sur disque, avec les dernières lignes rappelées à l'ouverture d'une fenêtre ; **liste des salons** du serveur avec recherche ; **ignorer** des pseudos ou masques, **mots d'alerte** ; fiche d'informations `/whois` ; confirmation avant de coller plusieurs lignes.
- **Reconnexion automatique** avec délai croissant ; les salons sont rejoints de nouveau.
- **Mémoire des fenêtres** : position et taille de chacune, retrouvées au démarrage.
- Zone de notification : le bouton « − » du menu principal range tout, un clic sur l'icône ramène tout.

Fermer la fenêtre d'un **salon** le quitte (comme dans la plupart des clients) ; fermer celle d'une conversation privée ne fait que la fermer.

## Installer

Télécharger `KIRC_<version>_x64-setup.exe` depuis les [releases](https://github.com/Crazyshouby/KIRC/releases), puis le lancer : installation par utilisateur, sans droits d'administrateur. WebView2 est installé s'il manque (Windows 11 l'a déjà).

L'exécutable n'est pas signé : SmartScreen peut demander une confirmation (« Informations complémentaires », puis « Exécuter quand même »).

## Données

| Quoi | Où |
|---|---|
| Carnet d'adresses, réglages, positions des fenêtres | `%APPDATA%\KIRC\config.json` |
| Journaux de conversation | `%APPDATA%\KIRC\logs\<serveur>\<salon>.log` |
| Mots de passe | Gestionnaire d'identifiants Windows (`KIRC`) |
| Cache de la vue web | `%LOCALAPPDATA%\fr.valentinlarge.kirc` |

Chaque fenêtre garde en mémoire 1000 lignes par défaut (réglable). Les conversations sont aussi écrites dans les journaux (réglages : désactivation, durée de conservation) ; les messages privés y sont donc enregistrés tant que l'option est active.

## Compiler

Prérequis : Rust (MSVC), Node.js 20 ou plus, Python 3 (outils seulement). WebView2 est présent sur Windows 10 et 11.

```
npm install
npm run dev                 # lance l'application en mode développement
npm run build               # installateur NSIS dans src-tauri/target/release/bundle/nsis/
cd src-tauri && cargo test  # 87 tests unitaires + 4 tests réseau
node tests/e2e.mjs          # test de bout en bout de la vraie application (voir ci-dessous)
python tools/gen_assets.py  # régénère polices et pictogrammes (fichiers versionnés)
```

`tests/e2e.mjs` démarre `tools/fake_ircd.py` (un petit serveur IRC scripté, avec TLS) puis l'application avec un dossier de données jetable, la pilote par le port de débogage de WebView2 et enregistre une capture de chaque fenêtre dans `tests/shots/`. Il écrit brièvement dans le coffre Windows (puis le vide) pour vérifier SASL.

## Organisation

```
src-tauri/src/irc/message.rs   format des lignes IRC (analyse, écriture, découpage)
src-tauri/src/irc/session.rs   le protocole comme machine à états pure (testée sans réseau)
src-tauri/src/irc/net.rs       connexion TCP/TLS, limiteur de débit, délais, reconnexion
src-tauri/src/app.rs           état, serveurs, fenêtres sans bordure et leur mémoire
src-tauri/src/commands.rs      ce que les pages peuvent appeler
src-tauri/src/config.rs        carnet d'adresses et réglages (JSON), secrets.rs : coffre Windows
src-tauri/src/logs.rs          journaux de conversation (écriture, relecture, purge)
ui/                            les pages (une seule page, une fenêtre par fonction)
tools/                         générateur de pictogrammes, serveur IRC factice
```

## Sécurité

- Le texte venu du réseau n'est jamais interprété comme du HTML : l'interface le pose avec `textContent`.
- Une ligne tapée ne peut pas en contenir une deuxième (CR, LF et NUL sont retirés de tout ce qui part vers le serveur).
- Les lignes de plus de 8 Ko venues du serveur sont ignorées sans être mises en mémoire.
- Politique de contenu stricte (`default-src 'self'`), navigation limitée à l'application, seuls `http` et `https` s'ouvrent depuis un message.
- Les envois sont limités en débit (rafale de 8 lignes puis 2 par seconde) pour ne pas être déconnecté pour « excess flood ».

## Licence

MIT — voir [LICENSE](LICENSE). Polices Archivo et Overpass Mono sous licence SIL OFL 1.1 (`third_party/fonts`). KDX est une marque de Haxial ; KIRC n'en reprend que l'apparence et n'est lié à Haxial d'aucune façon.
