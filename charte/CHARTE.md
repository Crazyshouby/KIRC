# Charte graphique KDX-UI

L'interface de KIRC, mise au point pour être indiscernable de celle de KDX (Haxial Software), consignée ici pour
servir à d'autres projets. Tout ce qui suit est mesuré au pixel sur KDX 1.6 ; les valeurs « finales » sont dans
`kdx-ui.css`, la page `exemple.html` montre chaque élément.

## 1. L'esprit

Des **fenêtres sans bordure système** posées sur un fond, chacune une fonction : cadre en relief, barre de titre,
panneau gris. Un monde **plat et gris**, éclairé **du haut gauche** : tout ce qui se manipule est en relief, tout ce
qui se lit ou se saisit est creusé. L'écran est lu comme un instrument : des **pixels nets** (police bitmap, aucun
lissage), **aucun arrondi**, une **seule couleur d'accent** qui dit quelle fenêtre est active.

Les cinq règles qui font que cela « ressemble à KDX » :

1. **Relief éclairé en haut à gauche.** Clair en haut et à gauche, sombre en bas et à droite, pour les boutons,
   cadres, barres de titre. Inverser l'éclairage (creux) pour les champs et listes.
2. **Gris neutres.** Jamais de teinte dans les gris (`#222`, `#333`, `#444`, `#555`, `#666`, `#888`). Le noir des
   champs est `#000`, le blanc du texte `#fff`.
3. **Aucun arrondi.** Les coins sont droits, ou chanfreinés de quelques pixels (deux pour les boutons de dialogue).
4. **Une couleur par rôle.** L'accent (rouge par défaut) marque l'activité (fenêtre active, bouton par défaut, barre
   de jauge, case cochée). Le vert, l'ambre et le rouge de statut ne signifient *que* ok, attention et problème.
5. **Une taille, une graisse.** Tout le texte fait 14 px de police bitmap, interligne 16 px (13 px en console).
   Pas de gras. La hiérarchie se fait par la couleur, la position et le relief.

## 2. Couleurs

### Neutres (jetons de `kdx-ui.css`)

| Jeton | Valeur | Rôle |
|---|---|---|
| `--frame` | `#222222` | fond de fenêtre, cadre |
| `--panel`, `--btn` | `#333333` | panneaux, boutons, barres de défilement |
| `--card-2` | `#2a2a2a` | menus déroulants, onglet actif, bouton enfoncé |
| `--ink` | `#000000` | champs, consoles, afficheurs |
| `--line` | `#555555` | filets entre les lignes d'une liste |
| `--faint` | `#888888` | texte désactivé ou secondaire, flèches |
| `--text` | `#ffffff` | texte |
| `--field-ink` | `#00ff00` | texte dans les champs et consoles (vert KDX) |
| relief clair | `#666`, `#444` | arêtes claires d'un bouton (extérieure puis intérieure) |
| relief sombre | `#222`, `#111`, `#000` | arêtes sombres, contour |

### Accent (une rampe, par thème)

Neuf valeurs dérivent d'une seule teinte : bord, milieu, sombre, survol, titre (début et fin), barre (haut, milieu,
bas). Rouge KDX par défaut, mesuré sur KDX :

`--a-edge #cc0000` · `--a-mid #880000` · `--a-dark #440000` · `--a-hover #660000` · titre `#320000 → #c30000` ·
barre `#e83c3c / #c01010 / #8a0202`.

Six autres thèmes appliquent la même rampe à une autre teinte (`themes.css`, généré par `outils/gen_themes.py`) :
**bleu**, **vert**, **violet** (« électrique »), **ambre** (CRT), **sarcelle**, **charbon**. Pour un projet qui a sa
propre teinte, ajouter une ligne dans `THEMES` de `gen_themes.py` : teinte, saturation, ton du texte des champs,
ligne sélectionnée.

### Statut (ne jamais détourner)

| Jeton | Valeur | Sens |
|---|---|---|
| `--green` | `#00ff00` | ok, connecté, fait |
| `--warn` | `#faa320` | attention, occupé, non lu |
| `--red` | `#ff5a4a` | problème, erreur |

Sélection « marquée » (texte sélectionné, ligne mise en évidence) : fond `--sel-bg` `#001177`, texte `--sel-fg`
`#ff00ff`. Les **lignes de liste** ne se colorent pas : sélection = gris `#3f3f3f` cerné d'un filet `#777`, survol =
gris `#3a3a3a`.

## 3. Typographie

| Police | Usage | Taille / interligne | Particularités |
|---|---|---|---|
| **KIRC Pixel** (`--font`) | toute l'interface | 14 px / 16 px | majuscules de 9 px, hauteur d'x de 7 px, espace entre lettres de 2 px |
| **KIRC Pixel Log** (`--font-log`) | console, saisie sous la console | 14 px / 13 px | majuscules de 8 px, espace de 1 px, mots séparés de 7 px |
| **KIRC Pixel Log Italic** (`--font-log-italic`) | actions, présence | 14 px / 13 px | version inclinée par lignes ; l'italique est une police à part |

Règles :

- **Rester à 14 px** (ou un multiple, si une version agrandie est un jour nécessaire) : un pixel de la police est
  un pixel CSS. Toute autre taille floute.
- **Pas de gras.** Une police bitmap n'a qu'une graisse ; l'épaissir d'un pixel fait se toucher les hampes de 2 px.
  Les boutons gardent `font-weight: 700` dans le CSS : sans effet visible sur la police, mais utile en repli
  (Segoe UI) si elle ne se charge pas.
- **Pas de lissage ClearType.** Dans Chromium / WebView2, lancer avec `--disable-lcd-text` (Tauri :
  `additional_browser_args`). Les polices du kit sont des **polices en couches (COLR v0)** : seul moyen trouvé pour
  obtenir des pixels purs sous WebView2, car DirectWrite floute en demi-pixel horizontal les glyphes ordinaires
  (ni le CSS ni une option ne l'empêchent). Conséquence : leur couleur vient de la couleur du texte CSS, mais elles ne
  se rendent pas dans un outil qui ne gère pas COLR.
- **Libellés de formulaire** terminés par « : » (`.form label[for]::after`).
- **Jeu de caractères** : latin complet avec accents, ponctuation française (« », …, —), flèches et quelques symboles.
  Régénérer avec `outils/gen_font.py` pour ajouter un glyphe.
- Repli : `"Segoe UI Symbol", "Segoe UI", sans-serif`.

## 4. Mesures

| Élément | Valeur |
|---|---|
| Barre de titre | 21 px : filet clair 1, dégradé 18, filet sombre 1 ; contour noir sous elle |
| Boutons de barre de titre | 14 × 14 px, glyphe 8 × 8, trait de 2 px blanc |
| Cadre de fenêtre | 5 px : quatre lignes concentriques + contour noir |
| Marge panneau → contenu | 5 px (8 px dans les dialogues de réglages, 12 px dans les dialogues simples) |
| Espacement entre éléments | 4–6 px (formulaires : 6 px de rang, 10 px de colonne) |
| Bouton de dialogue | hauteur 20 px, marge intérieure 14 px |
| Bouton de barre / menu déroulant | hauteur 22 px, filet noir partagé (−1 px de marge) |
| En-tête de colonne | hauteur 22 px |
| Champ de texte | marge intérieure 3 × 6 px ; ligne de saisie de salon : 56 px de haut |
| Case à cocher | 14 × 14 px |
| Barre de défilement | 15 px, un bouton de flèche à chaque bout |
| Poignée de redimensionnement | triangle de 12 px dans le coin bas droit |
| Icône | 16 × 16 px (14 px dans une barre de titre) |
| Aimantation des fenêtres | 10 px d'attraction, 2 px d'écart (bord de l'écran : 0) |

## 5. Le relief

Trois recettes à `box-shadow`, toutes dans `:root` :

- **Relief** `--bevel` — bouton, onglet, en-tête, poignée de curseur :
  `inset 1px 1px #666, inset 2px 2px #444, inset -1px -1px #222, inset -2px -2px #111`, contour noir de 1 px.
- **Enfoncé** `--bevel-down` — bouton pressé, onglet actif, case : le même relief inversé et plus sombre.
- **Creux** `--sunken-border` `#111 #666 #666 #111` + `--sunken-shadow` `inset 1px 1px #000` — champ, liste,
  console, afficheur.
- **Cadre de fenêtre** `--ring` (grise) et `--ring-focus` (accent) — quatre lignes, la plus claire à l'extérieur
  en haut et à gauche, à l'intérieur en bas et à droite.

## 6. Composants

| Composant | Classe | Règle |
|---|---|---|
| Fenêtre | `.win` > `.tb` + `.cb` | `.win.active` (ou `body.active`) pour la fenêtre qui a le focus ; `.shaded` pour l'enrouler |
| Barre de titre | `.tb` | titre centré, texte blanc ombré noir ; inactive = dégradé gris, active = accent |
| Boutons de titre | `.tb-btn` + `.tb-l` (fermer) `.tb-r` (enrouler) `.tb-r2` (agrandir) `.tb-m` (menu, hachuré) | |
| Bouton | `.btn` | relief, coins chanfreinés de 2 px, texte blanc ; désactivé `#888` |
| Bouton par défaut | `.primary` | un seul par dialogue ; anneau noir / accent foncé / accent clair / noir |
| Pile de boutons | `.stack > .sbtn` | colonne de boutons carrés partageant un filet ; une icône 16 px à gauche |
| Champ | `.text-input` | creux, noir, texte vert ; focus : anneau d'accent d'1 px |
| Menu déroulant | `select.btn` | bouton avec case de flèche séparée par un filet noir |
| Case à cocher | `.switch > input + .track` | plot en relief ; cochée : coche blanche seule |
| Curseur | `input[type=range]` | rigole noire, barre d'accent, poignée en relief |
| Groupe | `fieldset.grp > legend` | cadre gravé ; titre sur l'arête |
| Liste | `.list > .row-item` | lignes à filet ; barre d'état à gauche (6 px) |
| En-tête de colonne | `.lhead > .lh` | bouton carré qui trie ; `.on` = triangle blanc |
| Onglets | `.tabs > .tab` | boutons carrés ; actif : enfoncé, texte blanc ; autres : `#888` |
| Afficheur | `.status` | bande noire creusée, texte vert, voyant (`.orb`) à gauche |
| Bandeau d'alerte | `.banner` | noir, cadre d'accent, texte ambre |
| Notification | `.toast` (`.ok`) | boîte noire, cadre rouge (problème) ou vert (fait), ombre portée dure |
| Menu contextuel | `.menu` | mini-cadre ; titre au dégradé de la barre de titre |
| Console | `.log > .ln.<genre>` | voir ci-dessous |
| Voyant | `.orb` (`.green`, `.amber`, `.red`) | 11 px, sphère dégradée ; `pulse` quand occupé |

### Genres de lignes d'une console

Nom de l'auteur dans sa couleur, texte en vert ; **ses propres lignes en blanc** ; **action** rose en italique ;
**présence** (arrivée, départ) vert sombre en italique ; **sujet et modes** bleu ; **annonce** ambre ; **serveur**
gris ; **mise en évidence** fond bleu marine, filet rose ; **erreur et expulsion** rouge. Aucun gras.

### États d'un contrôle

| État | Rendu |
|---|---|
| Normal | relief, texte blanc |
| Survol | fond `#3b3b3b` (listes : `#3a3a3a`) |
| Pressé | enfoncé : `--bevel-down`, fond `#2a2a2a` |
| Désactivé | texte `#888`, curseur par défaut, pas de survol |
| Focus clavier | contour orange `--warn` de 2 px (champs : anneau d'accent d'1 px à la place) |
| Sélectionné | liste : gris clair + filet `#777` ; onglet : enfoncé |

## 7. Icônes

16 × 16 px, dessinées pixel par pixel, **contour noir d'1 px**, couleurs plates (palette de 20 valeurs dans
`outils/pixel_icons.py`), jamais lissées ni redimensionnées autrement qu'en multiples entiers. Exportées en SVG, une
`<path>` par couleur (`icons.js` : `pix("nom", taille)` renvoie le balisage, `fillPix()` remplit les
`[data-ic]`). Pour une nouvelle icône : ajouter son dessin dans `pixel_icons.py` et relancer la génération.

## 8. À ne pas faire

- Pas de `border-radius`, pas de dégradé « doux », pas d'ombre floue (les ombres sont dures : `4px 5px 0 rgba(0,0,0,.5)`).
- Pas de couleur de statut pour décorer ; pas de bleu « lien » hors des liens de console (`#5cf0ff`).
- Pas de `font-weight` pour hiérarchiser ; pas de taille autre que 14 px ; pas de `text-rendering` ou lissage.
- Pas de gris teinté, pas de blanc cassé : `#fff` ou `#888`.
- Pas de sélection colorée dans une liste ; pas de survol qui change la couleur du texte.
- Pas de bordure système : la fenêtre se dessine elle-même (voir section 9).
- Pas de plusieurs « boutons par défaut » dans un même dialogue.

## 9. Intégration dans un nouveau projet

1. Copier `kdx-ui.css`, `fonts.css`, `themes.css` et `icons.js` (ou tout le dossier `charte/`).
2. Dans la page : `<link>` vers `fonts.css`, `kdx-ui.css`, puis `themes.css` ; `<html data-theme="…">` pour l'accent.
3. Structurer chaque fenêtre en `.win > .tb + .cb` (voir `exemple.html`). Mettre `body.active` (fenêtre unique) ou
   `.win.active` (plusieurs) selon le focus ; un écouteur `focus`/`blur` suffit sur le web.
4. **Application de bureau (Tauri 2 / WebView2)** : fenêtres sans décoration (`decorations(false)`), glisser par
   la barre de titre (commande native, pas `data-tauri-drag-region`, pour pouvoir aimanter), `additional_browser_args`
   avec `--disable-lcd-text`. KIRC (`src-tauri/src/app.rs`) montre l'aimantation, l'enroulement, l'agrandissement et
   les onglets par fusion de fenêtres.
5. Les mises en page propres à l'application (colonnes, volets) vont dans le CSS du projet, pas dans `kdx-ui.css` :
   ce fichier ne contient que des composants.
6. Vérifier : aucune ligne de la page n'a `border-radius`, `font-weight` utile, ni couleur hors des jetons.
   Comparer à `exemple.html` côte à côte.

Régénérer les ressources : voir `README.md` (polices, icônes, thèmes).

## 10. Réserves de licence

- **Les formes des lettres de KIRC Pixel et KIRC Pixel Log reproduisent celles des polices de KDX** (relevées sur
  des captures de KDX 1.6) ; les glyphes manquants ont été dessinés pour KIRC. Elles restent l'œuvre de Haxial :
  avant de publier un projet qui les embarque, décider si on les garde ou si l'on redessine une police propre
  (`outils/gen_font.py` en est l'unique source).
- Les icônes (`pixel_icons.py`), les couleurs et les mesures sont dessinées ou relevées pour KIRC.
- KDX est une marque de Haxial ; aucun lien avec Haxial. Un projet qui adopte cette charte doit le préciser s'il
  parle de KDX.
