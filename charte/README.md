# KDX-UI — la charte graphique de KIRC, réutilisable

L'apparence KDX (Haxial) de KIRC, extraite pour d'autres projets : fenêtres sans bordure, relief gris éclairé en
haut à gauche, polices bitmap, icônes pixel, une couleur d'accent par thème.

| Fichier | Rôle |
|---|---|
| `CHARTE.md` | **la charte** : principes, couleurs, typographie, mesures, composants, interdits, intégration, licences |
| `exemple.html` | guide de style vivant : tous les composants, sélecteur de thème ; ouvrir dans un navigateur |
| `kdx-ui.css` | les jetons et les composants (valeurs finales) |
| `themes.css` | six autres couleurs d'accent (`<html data-theme="bleu">`) ; le rouge KDX est le défaut |
| `fonts.css` | les trois polices pixel, incorporées en base64 (woff2) |
| `icons.js` | 19 icônes 16 × 16 en SVG : `pix("salon", 16)`, `fillPix()` |
| `polices/` | les deux polices en TTF (pour un outil de dessin ; elles sont en couches COLR) |
| `outils/` | les générateurs : `gen_font.py` + `font_glyphs.py` (polices), `pixel_icons.py` (icônes), `gen_themes.py` (thèmes) |

## Démarrer un projet

```html
<link rel="stylesheet" href="fonts.css">
<link rel="stylesheet" href="kdx-ui.css">
<link rel="stylesheet" href="themes.css">
<body class="active">
  <section class="win">
    <div class="tb"><span class="tb-title">Mon outil</span></div>
    <div class="cb"> … </div>
  </section>
</body>
```

Copier le dossier, ou ces quatre fichiers (`kdx-ui.css`, `fonts.css`, `themes.css`, `icons.js`). `CHARTE.md`
section 9 détaille l'intégration (surtout pour Tauri / WebView2).

## Régénérer

Depuis `outils/`, avec Python et `fonttools` + `brotli` (`pip install fonttools brotli`) :

```
python gen_assets.py     # réécrit ../fonts.css (polices en base64) et ../icons.js (icônes SVG)
python gen_themes.py     # réécrit ../themes.css (une rampe d'accent par thème)
python gen_font.py       # écrit les TTF ; à copier dans ../polices/
```

Régénérer les polices produit des fichiers équivalents mais pas identiques octet pour octet (l'encodage woff2 varie
de quelques octets d'une exécution à l'autre).

## Où est la source de vérité

KIRC (`ui/theme.css`, `ui/kirc.css`) reste sa propre source : ce dossier en est une **photographie consolidée**,
sans mise en page propre à KIRC. Si l'apparence évolue dans KIRC, reporter le changement ici et dans `CHARTE.md`.

## Réserve de licence

Les formes des lettres des polices reproduisent celles de KDX (Haxial) : à trancher avant toute diffusion d'un
projet qui les embarque. Voir `CHARTE.md`, section 10.
