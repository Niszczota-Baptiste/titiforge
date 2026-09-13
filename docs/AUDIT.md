# Audit — pourquoi ce dépôt existe

Version lisible et mise en page : **https://claude.ai/code/artifact/afccec3f-b9d9-4976-8724-6b99630d686b**

Sources auditées : `ExeWorldEdit` (25 319 l. JS, 529 tests, bench figé),
`titisite` (moteur WE serveur), `EngineHub/WorldEdit` (Java), `mcedit/mcedit2`
(Python / Cython).

## Le recadrage

Une région Anvil pleine hauteur fait **100 663 296 blocs** — 512 × 384 × 512.
« 100 millions de blocs » est donc **un seul fichier `r.X.Z.mca`**. Un monde
Minefield de 800 régions en fait 80 milliards.

Conséquence, et elle commande tout le reste : il n'existe aucun état « le monde
est chargé ». La bonne question n'est pas combien de blocs on peut charger,
c'est **combien on peut ne pas charger**.

## Les cinq murs, tous mesurés dans le bench d'ExeWorldEdit

| | Mur | Mesure | Conséquence à l'échelle |
|---|---|---|---|
| 1 | Un bloc est un objet JS `{Name, Properties}` | **154 o/bloc** (1 087 Mo pour 7 077 888 blocs sur `mirror-rotate`) | 15 Go pour pivoter 100 M blocs |
| 2 | L'aperçu est une liste de blocs matérialisée | `previewMaxBlocks: 4 000 000` | le viewport ne *peut pas* afficher plus |
| 3 | Moteur monofil, chaque case visitée | **11,3 M blocs/s** | 9 s pour un remplissage de 100 M |
| 4 | Arbre NBT générique par chunk | **338 ms / 1 024 chunks**, 35 % du décodage | 13 min pour 800 régions |
| 5 | Un appel de dessin par chunk | **1 281 appels** sur un build réel | plafond WebGL2 |

Seul le n° 3 est une question d'optimisation. Les quatre autres sont
structurels : aucun réglage ne les répare.

## Ce que les références apprennent

**WorldEdit** — la chaîne d'`Extent` (masque → historique → batching → cible)
est le bon modèle mental et se reprend. `BlockMap` et
`RegionOptimizedVectorSorter` sont deux aveux déguisés : *le coût dominant,
c'est la position d'un bloc, pas sa valeur.* Mais `Extent.setBlock` est un
appel virtuel **par bloc**, WorldEdit tourne dans un serveur dont le monde est
déjà en RAM (il n'a jamais eu à résoudre la résidence), et il ignore la
palette — un `//replace` y reste O(blocs).

**MCEdit2** — `revisionhistory.py` : l'historique est une chaîne de révisions
**copie-sur-écriture**, chacune ne stockant que ce qui a changé. C'est la bonne
forme pour un undo qui ne tient pas en mémoire. Sa granularité est le *chunk* :
trop gros (un `//set` sur trois blocs sauvegarde 100 Ko). À la **section**,
compressée en zstd, une entrée fait quelques centaines d'octets.

Le reste est un contre-exemple utile : Python + Cython + OpenGL fixe, abandonné
en 2016, jamais passé la 1.13. **MCEdit2 est mort du même mur n° 1.**

## Décisions

| | | Pourquoi |
|---|---|---|
| Cœur | **Rust** | Des milliers de lignes d'arithmétique d'indices et de tranches partagées entre fils. En C++ chacune est un UB potentiel qui se manifestera comme une corruption de la sauvegarde d'un utilisateur — le pire mode de défaillance pour ce produit. Plus `rayon`, et le même langage que le rendu. |
| Rendu | **wgpu direct** | Vulkan / DX12 / Metal d'une seule base, avec compute shaders — non négociables pour le culling d'occlusion. Écarté : **Bevy** (un ECS est la mauvaise forme pour une grille creuse dont on pilote soi-même résidence, LOD et éviction), **Vulkan direct** (3–6 mois de plus, pas de macOS), **OpenGL** (pas de parité compute sur macOS). |
| Coque | **winit + egui** | Une fenêtre, une surface, un seul langage : l'UI se dessine dans la même passe que le viewport. Un viewport wgpu *dans* une webview Tauri, c'est WebGPU-dans-Chromium — le mur n° 5 revient intact. La couche UI reste derrière un trait pour qu'une coque Tauri/React redevienne possible sans toucher au cœur. |
| Dépôt | **neuf** | Aucune ligne partagée entre Rust/wgpu et JS/Electron, donc aucune migration progressive. `ExeWorldEdit` reste l'outil utilisable pendant les 3–4 mois du MVP, et sa référence d'écriture. |

## Ce qui survit intégralement d'ExeWorldEdit

Le round-trip lossless à décodeur indépendant · la distinction `cold_read`
entre « pas de chunk ici » et « chunk non décodé » · `hash3` sur la position ·
la table d'états de blocs · le packing Litematica à chevauchement · la
discipline de mesure (baseline figée, médiane de N, seuil à 25 %) · et les 44
pièges du `CLAUDE.md`, qui ne se compilent pas mais se relisent.
