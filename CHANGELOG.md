# Journal des modifications

## 0.3.0 — 2026-10-28

- **Sélecteur de thème** (Réglages > Affichage) : Rouge KDX (défaut, inchangé), Bleu électrique, Vert électrique, Violet électrique, Ambre CRT, Sarcelle et Charbon. Chaque thème applique à une autre teinte la rampe d'accent de KDX (cadre actif, barre de titre, jauges, cases cochées, couleur du texte des champs et des consoles, ligne sélectionnée). Les teintes viennent des fichiers d'apparence KDX « Electric … », « Amber CRT », « Mach Teal » et « Charcoal ». Aperçu immédiat dans la fenêtre des réglages, puis application à toutes les fenêtres ouvertes à l'enregistrement ; les nouvelles fenêtres s'ouvrent directement avec le bon thème.

## 0.2.0 — 2026-10-28

- **Activité non lue** : un compteur (orange, rouge s'il y a une mention) sur chaque salon et conversation du menu principal, et le nombre de lignes non lues dans la barre de titre d'une fenêtre qu'on ne regarde pas. Il retombe à zéro quand on regarde la fenêtre.
- **Journaux de conversation** : un fichier par salon ou conversation dans `%APPDATA%\KIRC\logs`, conservé 90 jours par défaut (réglable, 0 = toujours). Les dernières lignes réapparaissent, estompées, à l'ouverture de la fenêtre. Désactivable dans les réglages.
- **Liste des salons** (`/list` ou bouton « Salons… ») : fenêtre avec recherche dans les noms et les sujets, minimum de membres, tri, double-clic pour rejoindre. Bornée à 30 000 salons.
- **Ignorer** (`/ignore pseudo|masque`, `/unignore`, liste dans les réglages) et **mots d'alerte** qui comptent comme une mention.
- **Fiche d'informations** : `/whois` s'affiche dans une petite fenêtre (identité, serveur, inactivité, salons, compte, sécurité) au lieu de lignes dans le salon.
- **Collage de plusieurs lignes** : KIRC demande confirmation (50 lignes au plus) ; une ligne collée qui commence par « / » est envoyée comme du texte, jamais exécutée comme une commande.
- Négociation des capacités : une requête par capacité, SASL refusé ou sans mécanisme PLAIN signalé clairement.

## 0.1.0 — 2026-10-28

Première version.

- Plusieurs serveurs, carnet d'adresses, TLS, SASL PLAIN / NickServ, mots de passe dans le coffre Windows.
- Salons et messages privés, une fenêtre chacun ; liste des membres colorée par statut ; fenêtre des messages du serveur.
- Couleurs mIRC, liens, actions, CTCP, notifications, reconnexion automatique.
- Style KDX identique à celui d'AudioLinker : fenêtres sans bordure, enroulement, mémoire des positions.
- Installateur NSIS par utilisateur.
