# Feuille de route

Chaque phase se termine sur une **mesure**, pas sur une impression. Les durées
supposent un développeur à temps plein.

## Les trois héritages

titiforge n'est pas « un WorldEdit en Rust ». C'est la réunion de trois outils
qui n'ont jamais été réunis, et chacun apporte une chose que les deux autres
n'ont pas :

| | Ce qu'il apporte | Où on en est |
|---|---|---|
| **WorldEdit** | les opérations de masse : sélectionner, remplir, remplacer, tourner, copier. Le geste « change dix millions de blocs d'un coup » | le socle est là, les opérations de déplacement manquent |
| **MCEdit** | l'éditeur de MONDE : ouvrir une save, voler dedans, voir ce qu'on édite, échanger des schématiques | le rendu est là, la coque et les formats manquent |
| **SketchUp** | la **construction** : pousser-tirer une face, l'inférence qui accroche au bon endroit, et des **composants** qu'on modifie une fois pour les mettre à jour partout | à faire — et c'est le plus structurant |

Les deux premiers sont des outils d'ÉDITION : on prend ce qui existe et on le
transforme. SketchUp est un outil de CONCEPTION : on part de rien et on
construit. C'est pour ça qu'il est le plus important des trois à ne pas rater —
un éditeur de blocs de plus n'intéresse personne, un outil où l'on *conçoit*
un build n'existe pas encore sur Minecraft.

## La couture à poser avant la coque

**Un composant suppose que les blocs soient le RÉSULTAT d'un modèle, pas le
modèle.** Aujourd'hui le monde EST le document : une opération écrit des
blocs, le journal sait les défaire. Pour qu'une fenêtre posée quarante fois se
mette à jour partout quand on modifie sa définition, il faut qu'elle soit
**réévaluable**.

La bonne nouvelle est que les deux coutures posées tôt sont exactement
celles-là :

- **les opérations sont des DONNÉES** (`Masque`, `Motif`, `Plan`), pas des
  appels de fonction — donc elles se rejouent ;
- **le journal est typé, en ajout seul**, avec un `op: String` opaque et des
  corrections `Inconnu` qu'il traverse sans comprendre — donc il accueille des
  entrées qu'il ne sait pas interpréter.

Il manque trois choses, et elles ne sont pas petites :

1. **rejouer une entrée depuis ses PARAMÈTRES**, pas seulement l'annuler depuis
   ses octets — le journal garde les deux sens en octets, il devra garder aussi
   le plan ;
2. **une identité stable** par élément, pour que l'instance n° 37 reste la 37
   après vingt modifications ;
3. **l'invalidation par EMPRISE** — modifier l'élément 12 oblige à rejouer
   12..N sur sa seule portée. L'invariant n° 8 (« une opération ne paie que sa
   portée, et ses bornes décrivent ce qu'elle a vraiment écrit ») existe
   précisément pour ça.

Et une décision qui se prend **maintenant**, même si elle s'implémente plus
tard : **le monde reste souverain.** Les composants sont une COUCHE qui s'y
estampe, pas un modèle dont le monde serait la sortie. Sans ça on ne peut plus
poser un bloc à la main, et ce n'est plus un éditeur de monde. Contrepartie
assumée : une retouche manuelle à l'intérieur d'un composant est perdue à la
réévaluation — SketchUp a exactement le même défaut, et personne ne s'en
plaint.

*« Une couture qu'on ne pose pas au départ devient une refonte »* — c'est le
piège qu'on s'est déjà évité une fois avec le journal.

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
> **Atteinte.** Région pleine décodée en **28,2 ms** (`rayon`, 4 cœurs ;
> 107 ms monofil), empreinte **0,27 o/bloc**. Les deux dispositions
> (1.13–1.17 et 1.18+), les deux packings, et les chunks surdimensionnés
> `.mcc` sont couverts. **125 tests**, harnais `criterion` en place avec une
> fixture construite en Rust.
>
> Deux optimisations mesurées depuis : `zlib-rs` sur la compression (× 2,7,
> et 5 % plus petit) et `rayon` sur le chargement (× 3,80 sur 4 cœurs).
> Voir `docs/RESULTATS.md`.

## Phase 1 — Monde résident · 3–4 semaines

`tf-world` : cache LRU plafonné en **octets**, streaming piloté par la caméra,
copie de staging, `probeWorldLock`, sauvegarde zip horodatée avant toute
écriture.

Le **journal d'annulation** est une suite d'entrées réversibles **typées** —
instantané de sections en zstd, avant/après d'un document de projet, charge de
défaisage d'un greffon — et non un journal de blocs. Voir `docs/VISION.md` :
c'est une couture, et la retrofiter coûterait une refonte de toute la pile
d'undo.

Le **document de projet** est extensible dès le départ, avec identité stable et
version : c'est là que vivront les PNJ, les quêtes et les routes, et leur
ancrage doit suivre les blocs qui les portent.

> **En cours.** Cinq des six morceaux sont là, sous **240 tests** :
>
> | Morceau | État |
> |---|---|
> | Adressage (`coords.rs`) | ✅ division plancher partout, `BBox` → sections/chunks/régions |
> | Résidence (`residency.rs`) | ✅ LRU plafonnée en OCTETS, épinglage, garde d'édition |
> | Contrat de source (`source.rs`) | ✅ suite de contrat jouée contre deux implémentations |
> | Dossier de save (`fs_source.rs`) | ✅ écriture atomique, sonde de verrou honnête |
> | Staging (`staging.rs`) | ✅ composition source + couche, pierres tombales, ordre du commit imposé par la signature |
> | Journal typé (`journal.rs`) | ✅ persistant, ajout seul, points de reprise nommés, budget + compactage |
> | Streaming piloté par la caméra | ⬜ demande le viewport (phase 2) |
> | Document de projet | ⬜ |
>
> **Ce que le journal pèse, mesuré.** Une région pleine (100 663 296 blocs),
> un `//replace` de palette : **175 ko** d'éditions dans un sens — contre
> 30,8 Mo si l'on réécrivait les blocs de champs entiers, et 26 Mo si l'on
> copiait la région. L'annulation se paie pour ce qu'une opération a ÉCRIT.
>
> Le bout à bout est bouclé par un test : opérer sur un vrai `.mca`,
> enregistrer, sérialiser le journal, le relire depuis ses OCTETS, annuler —
> et retrouver le fichier de départ octet pour octet.

> **Sortie.** Monde de 800 régions ouvert ; RAM bornée au budget déclaré ;
> aucune pause > 8 ms sur le fil principal.

## Phase 2 — Rendu de base · 4–6 semaines

`tf-mesh` parallèle (portage du greedy meshing + occlusion ambiante), arène
GPU sous-allouée par section, `multi_draw_indirect`, frustum en compute shader,
atlas en texture-tableau construit depuis le pack de l'utilisateur.

Le rendu s'organise en **graphe de passes nommées**, pas en pipeline figé :
c'est ce qui permettra à un greffon d'insérer une passe, et au rendu hors ligne
(captures haute qualité, shaders) d'être un autre graphe plutôt qu'un mode
dégradé du premier.

> **En cours — le maillage est là, mesuré, et il a déjà changé d'architecture
> deux fois sous la mesure.**
>
> | | |
> |---|---|
> | Fixture honnête (`Build`) | ✅ `docs/fixtures.md` |
> | Passe gloutonne | ✅ par rangées de bits |
> | Passe de modèles, en quads | ✅ pour l'export et les tests |
> | Passe de modèles, en **instances** | ✅ le chemin du GPU |
> | Occlusion ambiante | ⬜ |
> | Arène GPU, multi-draw indirect | ⬜ |
> | Graphe de passes nommées | ⬜ |
>
> **Deux résultats qui ont décidé de l'architecture.**
>
> *La passe de modèles fait neuf dixièmes de la géométrie.* 349 k blocs-modèles
> contre 3,5 M de cubes posés, et pourtant 5,8 M de quads contre 630 k. Ça ne
> se déduisait pas de la part du catalogue (66,8 %), qui dit autre chose. Donc
> les modèles n'émettent plus de géométrie mais des **poses** de huit octets :
> 93 Mo de quads remplacés par 2,8 Mo.
>
> *La passe gloutonne payait le volume et non la sortie.* Une section pleine —
> six quads — coûtait 94 µs. L'opacité par rangées de bits ramène la chaîne
> complète de **453 ms à 251 ms** sur un quart de région bâtie.

> **Sortie.** 60 FPS soutenus, rayon 512 blocs, sur un vrai monde Minefield.
> < 5 appels de dessin (référence : 1 281).

## Phase 3 — Les opérations qui manquent · **en cours**

Le socle est là : trois étages, masques booléens complets, motifs pondérés,
staging, journal. Manque ce qui DÉPLACE.

| | État |
|---|---|
| `//set`, `//replace`, mélange pondéré | ✅ mesurés à l'étage palette |
| Masques `et` / `ou` / `non` / parmi | ✅ |
| **Rotation et miroir** | ✅ — appliquées sur la PALETTE de l'extrait, jamais par bloc ; ce que la règle ne sait pas transformer est laissé tel quel et NOMMÉ |
| Copier / coller | ✅ — `copier` est la seule opération qui n'écrit rien ; `Collage` est à l'étage bloc par nature et ne paie que son extrait |
| Déplacer (`//move`), empiler (`//stack`) | ✅ — composées de copier/effacer/coller, sous **une seule** entrée de journal |
| Formes : sphère, cylindre, pyramide | ✅ — une forme répond par SECTION avant de répondre par case, donc le cœur d'une sphère garde l'étage palette |
| Murs et faces (`//walls`, `//faces`) | ✅ — un pavé moins un pavé plus petit ; rien de neuf à tenir juste |
| **Portée d'une opération** (`Portee::Colonne`) | ✅ — une opération déclare ce qu'elle lit, `edition.rs` lui donne la vue correspondante |
| Naturalisation (`//naturalize`) | ✅ — première opération à portée `Colonne` : « où est la surface » ne se décide pas section par section |
| Lissage, creuser | ⬜ — le lissage lit le voisin d'à CÔTÉ, donc au-delà du chunk ; creuser lit tout le volume. Il leur faut une portée de plus, et elle se mesurera avant d'être écrite |
| **Les coffres suivent les blocs** | ✅ — l'entrée voyage par ses OCTETS et seules ses trois coordonnées sont réécrites ; une entité dont la case a changé d'état part avec son bloc |
| Biomes : lecture, écriture, `//setbiome` | ✅ — seconde palette par section, grille de 4 × 4 × 4 ; 1.18+ seulement, et 1.13–1.17 est REFUSÉ plutôt que deviné |
| La COULEUR d'un biome, dérivée du jeu | ✅ — table `colormap/grass.png` × `worldgen/biome/*.json`, formule du jeu à la lettre ; le marais est annoncé APPROCHÉ |
| Les biomes branchés à la teinte du rendu | ✅ pour la passe GLOUTONNE — le biome entre dans la clé de fusion, donc un quad ne peut pas enjamber une frontière là où ça se verrait |
| La teinte des blocs-MODÈLES (feuilles, vignes) | ⬜ — une pose fait 16 octets et n'a pas de place pour une couleur. Trois pistes, aucune mesurée : six bits libres dans `Pose::local`, une table par section, ou un second tampon |

C'est la phase la moins chère du lot : la partie difficile — savoir qu'un
escalier `shape=outer` tourné devient tel autre état — est déjà faite et
mesurée.

> **Sortie.** Un build copié, tourné d'un quart de tour, recollé ailleurs :
> les escaliers regardent au bon endroit, les coffres ont gardé leur contenu,
> et un seul `Ctrl+Z` défait l'ensemble.
>
> Atteinte pour la rotation, les coffres, `//move` et `//stack`, en ligne de
> commande :
> `editer --sel "…" --copier-vers "64,0,64" --tourner 90 --pack <installation>`.
> Le contrôle qui compte est le non-changement : copier, tourner quatre fois,
> reposer à sa propre place doit rendre **zéro chunk modifié** — vérifié sur le
> monde d'essai comme en test.

## Phase 4 — La coque, et le geste SketchUp · le tournant

`tf-app` n'existe pas. C'est ce qui transforme un moteur mesuré en outil.

**Deux règles d'architecture, et elles ne se retrofitent pas :**

- **le moteur vit dans un fil à part, l'interface ne bloque jamais.** Une
  opération de trois secondes ne doit pas figer la fenêtre ;
- **les formulaires se GÉNÈRENT depuis les descripteurs d'opérations.**
  `ExeWorldEdit` l'a prouvé : aucun formulaire n'y est écrit à la main, et
  ajouter une opération n'y demande aucune ligne d'interface. Un test y relie
  le descripteur du moteur à la palette d'outils.

Le socle de fenêtre : `winit` + `egui`, caméra, sélection au cuboïde avec
poignées, palette de blocs avec icônes, historique visible et cliquable.

**Et c'est ici qu'arrive le premier tiers de SketchUp**, parce que c'est un
geste et pas une structure de données :

| | Pourquoi maintenant |
|---|---|
| **Pousser-tirer** une face de la sélection | un geste + un remplissage. La sélection existe déjà, l'opération de remplissage aussi |
| **L'inférence** — accrochage aux coins, arêtes, milieux, axes, plans et alignements de ce qui est déjà bâti | c'est ce qui FAIT SketchUp. Pure géométrie, zéro impact moteur : ça vit entièrement dans la coque |
| **Saisie chiffrée** pendant le geste — taper `12` en tirant | trivial une fois le geste là, et c'est la moitié de la précision de SketchUp |

L'inférence est le morceau le plus difficile à rendre JUSTE de toute la
phase — pas à écrire, à régler. Sur une grille de blocs elle est plus simple
qu'en CAO ; ce qui compte est d'accrocher à ce qui est *bâti* (le nu d'un mur,
l'axe d'une colonne, la hauteur de la fenêtre d'à côté), pas seulement à la
grille.

> **Sortie.** Ouvrir une save, voler dedans, tirer un mur de vingt blocs à la
> souris en accrochant au nu du bâtiment d'en face, annuler. Sans écrire une
> seule commande.

## Phase 5 — Le streaming piloté par la caméra

La fenêtre de résidence existe et est testée ; rien ne la pilote. Tant qu'on
affiche une zone choisie à la main, ça ne se voit pas — dès qu'on vole dans un
monde de 800 régions, c'est bloquant.

Avec ça viennent le remaillage INCRÉMENTAL (une opération ne remaille que ses
bornes, plus une case de débordement) et l'arène GPU par tranches, déjà
préparée.

> **Sortie.** Monde de 800 régions, vol continu, RAM bornée au budget déclaré,
> aucune pause > 8 ms sur le fil principal.

## Phase 6 — Formats d'échange

`tf-formats` : `.schem` (Sponge v2 et v3), `.schematic` (avec la table
d'aplatissement 1.12 → 1.13), `.litematic` (v4–v6, packing **à
chevauchement**), `.nbt` de structure.

C'est ce qui manque pour la parité MCEdit, et c'est ce qui permet d'échanger
un build avec quelqu'un qui n'a pas titiforge.

> **Sortie.** Round-trip octet pour octet sur les quatre formats, vérifié
> contre un fichier produit par l'outil d'origine.

## Phase 7 — SketchUp : le document · **le morceau qui change tout**

Le deuxième tiers de SketchUp, et le seul qui touche à l'architecture. Voir
« La couture à poser avant la coque », en tête de ce document.

| | |
|---|---|
| **Groupes** | une sélection nommée qu'on déplace et duplique d'un bloc |
| **Composants** | une définition + N instances. Modifier la définition met à jour les N |
| **Réévaluation** | une entrée de journal qu'on rejoue depuis ses paramètres, pas depuis ses octets |
| **Identité stable** | l'instance n° 37 reste la 37 |
| **Invalidation par emprise** | rejouer 12..N sur la seule portée de 12 |
| **Calques** | organiser, masquer, verrouiller |

Une maison faite de quatre composants « fenêtre », deux « porte » et un
« toit », où changer la fenêtre change les quatre. C'est ce qu'aucun outil
Minecraft ne sait faire, et c'est la raison d'être du projet.

> **Sortie.** Poser vingt fois un composant, en modifier la définition, voir
> les vingt se mettre à jour — et que `Ctrl+Z` défasse la modification, pas
> les vingt poses.

## Phase 8 — Finitions du rendu

Ce qui a été laissé de côté sciemment, chiffré : les **fluides** (4,5 % des
blocs posés de Mosslorn, dont 6,2 M d'eau, aujourd'hui invisibles), les
**biomes** décodés pour la vraie teinte, l'**occlusion ambiante**, `uvlock`,
le **LOD** par octree de région, et l'occlusion HZB.

> **Sortie.** 60 FPS à un rayon de 4 000 blocs. Sous terre, < 3 % des sections
> résidentes effectivement dessinées.

## Phase 9 — Versions, mods, greffons

`ChunkFormat` par palier de `DataVersion` (détecté **par chunk**, jamais par
monde — déjà le cas), lecture de `mods/*.jar`, et la frontière de greffon en
WASM (`docs/VISION.md`, § 6).

---

## L'ordre, et pourquoi

**Phases 0 à 2 : faites.** Le moteur lit, écrit sans perte, opère à trois
étages et dessine un vrai build en deux appels. Vérifié sur deux vraies saves,
606 millions de blocs réécrits puis annulés au bit près.

**La phase 3 est presque gratuite** vu ce qui est déjà dérivé, et elle complète
le socle WorldEdit.

**La phase 4 est le tournant** : sans coque, rien n'est utilisable ni jugeable,
et c'est là que titiforge commence à ressembler à SketchUp plutôt qu'à une
ligne de commande.

**La phase 7 est la raison d'être.** Elle peut attendre, mais sa couture non :
elle est décidée en tête de ce document, et les phases 3 et 4 doivent être
écrites en la respectant — c'est-à-dire en gardant les paramètres des
opérations, pas seulement leurs octets.
