# cdx-frames — le cadre CDX autour des fenêtres de Windows

Un petit programme qui dessine le cadre de la charte CDX (bordure en relief à quatre lignes, barre de titre à dégradé,
boutons de 14 px qui s'éclaircissent sous la souris, titre dans la police pixel) autour des fenêtres des programmes que
vous lui nommez. **Seules** les fenêtres de ces programmes sont touchées ; rien d'autre sur le bureau ne change, et
rien n'est installé ni patché.

## Utiliser

```
cargo build --release
target\release\cdx-frames.exe                       # décore les programmes de la liste (voir plus bas), sans console
target\release\cdx-frames.exe charmap.exe mmc.exe   # décore ces programmes, au lieu de la liste
target\release\cdx-frames.exe --console ...         # garde la console : Ctrl+C remet les fenêtres comme avant
target\release\cdx-frames.exe --preview image.png   # dessine un cadre dans une image, sans toucher aucune fenêtre
```

## La liste des programmes

`%APPDATA%\CDX-Frames\programmes.txt`, créé au premier lancement : un programme par ligne (`charmap.exe`, le `.exe` peut
manquer), `#` pour un commentaire. Le fichier est relu dès qu'il change : on peut l'éditer pendant que le programme tourne ;
retirer une ligne rend aussitôt à la fenêtre son aspect de Windows.

## L'icône de la zone de notification

Un clic (gauche ou droit) ouvre un menu :
- **Suspendre les cadres** : tout revient comme avant, sans quitter le programme ;
- **Modifier la liste des programmes…** : ouvre le fichier ;
- **Démarrer avec Windows** : coche ou décoche une entrée dans `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` ;
- **Quitter** : remet les fenêtres comme avant (coins arrondis et bordure de Windows rétablis).

Un seul exemplaire tourne à la fois (un second lancement s'arrête de lui-même).

## Comment ça marche

Chaque fenêtre décorée reçoit une fenêtre transparente posée juste au-dessus d'elle (Windows « en couches »). Elle porte
le cadre ; là où elle est transparente, la souris passe au travers. Elle cache la barre de titre de Windows avec la
barre CDX et suit la fenêtre à 60 images par seconde. Un glissement par la barre de titre, un redimensionnement par le
cadre, la croix, le « + » (agrandir), le « − » (réduire), le menu hachuré (menu système) et le double-clic sont rendus à la
fenêtre par les mêmes commandes qu'une barre de titre normale. Les coins arrondis de Windows 11 sont désactivés pour la
fenêtre décorée ; le dessin est exactement celui de `cdx-ui/cdx-ui.css (le dépôt voisin cdx-ui)`, la police est celle de `cdx-ui/gen/gen_font.py`
(exportée par `tools/export_font_json.py`) dessinée pixel par pixel.

## À savoir

- **Choisir les programmes** : les applications qui dessinent elles-mêmes leur barre de titre (Explorateur, Edge,
  Chrome, Terminal Windows, les applications du Store) seraient amputées de leurs onglets : ne les mettez pas dans la liste.
  Les applications Win32 classiques (Table des caractères, Gestionnaire de périphériques via mmc.exe, outils
  d'administration, anciens logiciels) conviennent.
- **Fenêtres agrandies** : le cadre disparaît (la fenêtre reprend son aspect Windows).
- **Écrans à forte résolution** : le cadre est agrandi par pas entiers (×1, ×2, ×3).
- **Limite de principe** : Windows ne laisse pas dessiner *dans* le client d'une application : les barres de défilement,
  boutons et cases cochées restent ceux de l'application.
- **Essayé** : dessin (15 tests), position exacte, suivi, ordre des fenêtres, clic sur la croix, éclaircissement des boutons,
  liste en fichier (création, rechargement, fichier avec BOM), instance unique. **Pas encore essayé à la main** :
  glissement, redimensionnement, menu hachuré, double-clic, icône de la zone de notification, démarrage avec Windows,
  écrans à plusieurs échelles.
