# Feuille de route

Chaque phase se termine sur une **mesure**, pas sur une impression. Les durées
supposent un développeur à temps plein.

## Phase 0 — Socle mesurable · 2–3 semaines

Workspace Cargo, `tf-nbt` (lecteur zéro-copie), `tf-anvil` en lecture **et
écriture**, round-trip lossless vérifié par un décodeur indépendant réécrit de
zéro, `criterion` avec les scénarios de `we-engine` sous leurs noms actuels.

> **Sortie — l'essentiel est atteint.** Le prototype décode une région pleine
> hauteur en **48 ms** / **54,6 Mo** (cible : < 150 ms). `tf-nbt` et `tf-anvil`
> font la lecture, l'écriture et le round-trip lossless, sous **85 tests**,
> croisés contre deux implémentations indépendantes **et** contre un `.mca`
> produit par le moteur JS.
>
> Les deux dispositions (1.13–1.17 et 1.18+), les deux packings, et les
> chunks surdimensionnés `.mcc` sont couverts. **109 tests.**
>
> Reste : le harnais `criterion`, pour que les chiffres viennent d'une mesure
> continue et non du seul prototype.

## Phase 1 — Monde résident · 3–4 semaines

`tf-world` : cache LRU plafonné en **octets**, streaming piloté par la caméra,
copie de staging, journal d'undo **par section** en zstd (chaîne de révisions
façon MCEdit2, mais à la section et non au chunk), `probeWorldLock`, sauvegarde
zip horodatée avant toute écriture.

> **Sortie.** Monde de 800 régions ouvert ; RAM bornée au budget déclaré ;
> aucune pause > 8 ms sur le fil principal.

## Phase 2 — Rendu de base · 4–6 semaines

`tf-mesh` parallèle (portage du greedy meshing + occlusion ambiante), arène
GPU sous-allouée par section, `multi_draw_indirect`, frustum en compute shader,
atlas en texture-tableau construit depuis le pack de l'utilisateur.

> **Sortie.** 60 FPS soutenus, rayon 512 blocs, sur un vrai monde Minefield.
> < 5 appels de dessin (référence : 1 281).

## Phase 3 — Opérations · 3–4 semaines

Répartition à trois étages, sélections-prédicat (cuboïde, sphère, cylindre,
polygone, libre en RLE), masques et motifs WorldEdit, `//set //replace
//overlay //walls //stack //move`, undo/redo.

> **Sortie.** `//set` et `//replace` sur 100 M blocs en < 500 ms, **undo
> compris**. Le prototype fait déjà 2,78 et 0,26 ms sans undo : toute la marge
> restante est pour le journal et les sections de bordure.

## Phase 4 — Occlusion et LOD · 3–4 semaines

HZB deux passes reprojetée depuis l'image précédente, mips d'octree par région
construits par vote majoritaire sur la palette et mis en cache, éviction
pilotée par le budget.

> **Sortie.** 60 FPS à un rayon de 4 000 blocs. Sous terre, < 3 % des sections
> résidentes effectivement dessinées.

## Phase 5 — Versions et mods · 3–4 semaines

`ChunkFormat` par palier de `DataVersion` (détecté **par chunk**, jamais par
monde), lecture de `mods/*.jar`, **règles** de rotation dérivées des propriétés
plutôt qu'une table écrite à la main, blocs non reconnus signalés et laissés
intacts.

> **Sortie.** Un monde Fabric et un monde NeoForge ouverts et affichés ; un
> escalier moddé pivoté correctement ; round-trip lossless intact sur les deux.

## Phase 6 — Formats et outils · 3 semaines

`.mca`, `.schem` (Sponge v2 et v3), `.schematic` (avec la table d'aplatissement
1.12 → 1.13), `.litematic` (v4–v6, packing **à chevauchement**), pinceaux,
copier/coller avec block entities.

> **Sortie.** Round-trip octet pour octet sur les quatre formats, vérifié
> contre un fichier produit par l'outil d'origine.

---

**MVP = phases 0 à 3**, environ 3 à 4 mois. Ouvrir un vrai monde, y voler à
60 FPS, sélectionner au cuboïde, `//set`, `//replace`, annuler, sauvegarder
sans perte. C'est le point où l'outil devient utilisable sur Minefield.
