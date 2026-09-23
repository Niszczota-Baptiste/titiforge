# Feuille de route

Chaque phase se termine sur une **mesure**, pas sur une impression. Les durées
supposent un développeur à temps plein.

## Les trois héritages

titiforge n'est pas « un WorldEdit en Rust ». C'est la réunion de trois outils
qui n'ont jamais été réunis, et chacun apporte une chose que les deux autres
n'ont pas :

| | Ce qu'il apporte | Où on en est |
|---|---|---|
| **WorldEdit** | les opérations de masse : sélectionner, remplir, remplacer, tourner, copier. Le geste « change dix millions de blocs d'un coup » | ✅ le socle ET les déplacements ; reste à les brancher à des boutons |
| **MCEdit** | l'éditeur de MONDE : ouvrir une save, voler dedans, voir ce qu'on édite, échanger des schématiques | le rendu est là, la coque VOLE et SÉLECTIONNE ; les formats d'échange manquent |
| **SketchUp** | la **construction** : pousser-tirer une face, l'inférence qui accroche au bon endroit, et des **composants** qu'on modifie une fois pour les mettre à jour partout | l'inférence est là et DIT à quoi elle tient ; pousser-tirer et les composants restent — et c'est le plus structurant |

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

Il manquait trois choses. **Deux sont posées ; la troisième appartient au
document, qui n'existe pas encore.**

1. ✅ **rejouer une entrée depuis ses PARAMÈTRES**, pas seulement l'annuler
   depuis ses octets. `Genre::Operation` porte un `params: Vec<u8>` **opaque au
   journal**, comme `op` l'est déjà : le cœur ne l'interprète pas, celui qui a
   écrit l'opération sait le relire, et un greffon y met ce qu'il veut. Écrit
   en DERNIER dans le corps de l'entrée, pour qu'un journal d'avant reste
   lisible au lieu d'être décalé — un journal s'arrête à la première entrée
   qu'il ne comprend pas, donc un décalage coûterait tout l'historique.
   `Entree::est_rejouable` pose la question en toutes lettres.
2. ⬜ **une identité stable** par élément, pour que l'instance n° 37 reste la 37
   après vingt modifications. Rien à poser aujourd'hui : elle vit dans le
   document de projet, qui est de la phase 7. L'`id` monotone du journal n'en
   est pas une — il numérote des ACTIONS, pas des objets.
3. ✅ **l'invalidation par EMPRISE** — modifier l'élément 12 oblige à rejouer
   12..N sur sa seule portée. L'invariant n° 8 (« une opération ne paie que sa
   portée, et ses bornes décrivent ce qu'elle a vraiment écrit ») existe
   précisément pour ça ; `Entree::bounds()` les rend, et
   `RapportRegion::genre` les y met depuis ce que l'opération a VRAIMENT
   écrit, jamais depuis la sélection.

Et une quatrième, qui ne figurait sur aucune liste parce qu'elle avait l'air
faite : **la jonction entre une opération et le journal n'existait nulle
part.** Les tests la recomposaient à la main, en commentant « comme
l'application le tiendra » — c'est-à-dire que chaque hôte allait la réécrire,
avec trois occasions de se tromper dont une que ce dépôt a déjà payée (l'ordre
des correctifs, invisible tant qu'aucune opération ne repasse sur un chunk).
`RapportRegion::genre` / `journaliser` la posent une fois, et un test la
MUTE pour le prouver.

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

## Phase 3 — Les opérations qui manquent · ✅ **faite**

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
| Lissage (`//smooth`) | ✅ — résolu par DEUX PASSES plutôt qu'une vue plus large : une lecture qui n'écrit rien, un calcul pur, une écriture à portée `Colonne` |
| Creuser (`//hollow`) | ✅ — topologique et non géométrique : est vidé ce qu'aucun chemin de VIDE ne relie au dehors, donc une salle déjà percée d'une porte ne se remplit pas. La seule opération qui matérialise tout le volume — assumé, borné, et ANNONCÉ avant de commencer. La diffusion est itérative : une récursion déborderait la pile, et un débordement de pile n'est pas rattrapable en Rust |
| **Les coffres suivent les blocs** | ✅ — l'entrée voyage par ses OCTETS et seules ses trois coordonnées sont réécrites ; une entité dont la case a changé d'état part avec son bloc |
| Biomes : lecture, écriture, `//setbiome` | ✅ — seconde palette par section, grille de 4 × 4 × 4 ; 1.18+ seulement, et 1.13–1.17 est REFUSÉ plutôt que deviné |
| La COULEUR d'un biome, dérivée du jeu | ✅ — table `colormap/grass.png` × `worldgen/biome/*.json`, formule du jeu à la lettre ; le marais est annoncé APPROCHÉ |
| Les biomes branchés à la teinte du rendu | ✅ pour la passe GLOUTONNE — le biome entre dans la clé de fusion, donc un quad ne peut pas enjamber une frontière là où ça se verrait |
| La teinte des blocs-MODÈLES (feuilles, vignes) | ✅ — et par AUCUNE des trois pistes envisagées : la table de géométrie est déjà mémoïsée, il suffit de la mémoïser sur `(état, biome)`. La pose reste à 16 octets, le shader ne bouge pas d'une ligne, et il n'y a ni nouveau tampon ni nouvel empaquetage à tenir juste. Le coût est borné par le ZÉRO que le mailleur écrit pour un état non teinté : la quasi-totalité du catalogue garde une table, comme avant |

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

## Phase 4 — La coque, et le geste SketchUp · ✅ **faite**

C'est ce qui transforme un moteur mesuré en outil.

**Ce qui tient debout aujourd'hui** (`cargo run --release -p tf-app -- <assets>`) :
une fenêtre `winit`, le viewport `wgpu` — le MÊME code que la capture hors
écran — et l'interface `egui` par-dessus. On vole (molette enfoncée pour
tourner, +Maj panoramique, roulée pour avancer ; ZQSD/WASD), on vise au
réticule, on lit la case qu'on casserait et celle où l'on poserait, on pose
les deux coins d'une sélection, on l'accroche à ce qui est bâti en voyant
POURQUOI axe par axe, on allume le quadrillage des chunks et celui des `.mca`,
et le panneau annonce ce que la sélection coûtera — en sections entières
comptées, jamais en « alignée ».

**Les formulaires sont ENGENDRÉS.** `tf-ops/src/catalogue.rs` dit ce que les
opérations prennent en paramètre — nom, noms WorldEdit, bornes, coût — et la
coque ne connaît AUCUNE opération : elle sait dessiner une saisie (un bloc, un
entier borné, une direction, un mélange pondéré) et lit le reste. Ajouter
`//deform` au moteur demandera zéro ligne de renderer. Deux tests tiennent la
couture : chaque descripteur doit se construire (contre *déclaré, branché,
testé — et inatteignable*), et chaque genre de paramètre doit avoir un champ
(contre *un type déclaré sans champ pour le saisir*, qui a livré « Remplacer »
et « Mélange » inutilisables dans `ExeWorldEdit`, sans une erreur à l'écran).
Le normaliseur est SUR le chemin de toute construction — il n'existe pas de
façon de fabriquer un travail sans passer par sa description — ce qui exige
qu'il soit idempotent, et un test l'exige pour chaque opération.

**Le FIL MOTEUR est là**, et c'était une règle d'architecture qui ne se
retrofite pas : une opération de trois secondes ne doit pas figer la fenêtre.
`moteur.rs` prend des commandes et rend des réponses par `mpsc`, la coque ne
fait que des `try_recv` — un test le vérifie dans un fil TÉMOIN à attente
bornée, parce qu'une propriété qui se dit « ça ne doit pas attendre » a pour
seul symptôme, quand elle casse, l'absence de symptôme. « Appliquer »,
« Annuler », « Refaire » et l'écriture dans la save passent tous par là, sur la
copie de travail, et la coque RELIT le staging — jamais la source.

**La coque sait se dessiner dans une TEXTURE** (`--capture ecran.png`),
interface comprise. Ce n'est pas un mode dégradé : c'est ce qui permet de la
regarder depuis une machine sans écran, et de la vérifier au pixel comme le
rendu. Ce que la coque DÉCIDE vit dans `etat.rs`, en types purs, et se teste
sans monter un GPU — treize tests, cinq mutations, zéro survivant.

### Deux MODES, et un bouton pour passer de l'un à l'autre

**Éditer un monde et concevoir un bâtiment ne se font pas pareil.** Ce qui
change n'est PAS la caméra : c'est ce que font les boutons.

| | **Édition** (WorldEdit, MCEdit) | **Conception** (SketchUp) |
|---|---|---|
| Clic gauche / droit | les deux coins d'un VOLUME | des ENTITÉS : face, arête, composant |
| Ce qu'on manipule | une `BBox` | une face qu'on pousse-tire, un composant |
| Le survol montre | la case visée | un point d'inférence — coin, milieu, axe |
| L'unité mentale | le bloc | la face et le composant |

Les modes sont nommés d'après ce qu'ils FONT, pas d'après les trois logiciels
qu'ils héritent : ces noms-là survivront aux leurs.

### La caméra : le point fixe est le JOUEUR

**Dans les deux modes.** Tourner fait pivoter le regard autour de l'œil ;
l'œil ne bouge pas. C'est ce que fait Minecraft, donc ce que la main de
quiconque construit sait déjà faire — et une habitude de jeu ne se rééduque
pas, elle se sert.

Une orbite autour d'un pivot posé sur le build a été écrite, puis RETIRÉE :
c'est la convention de la CAO, pas celle d'un monde où l'on vole. Pour tourner
autour d'un bâtiment, on vole autour, exactement comme en jeu. Elle obligeait
en plus à décider d'un pivot à chaque bascule de mode — le seul morceau de
tout le pilotage qui était difficile à rendre juste, et il a disparu avec elle.

**La répartition des boutons, et elle est fixe :**

| Geste | Effet |
|---|---|
| **Molette ENFONCÉE**, glisser | tourner le regard |
| Molette enfoncée + Maj, glisser | panoramique, dans le plan de l'écran |
| Molette, rouler | avancer et reculer |
| **Clic gauche** | aux outils et à la sélection |
| **Clic droit** | aux outils et à la sélection |

C'est la caméra de SketchUp (molette) et la sélection de WorldEdit (gauche =
coin 1, droit = coin 2) réunies sans se marcher dessus. Donner la caméra au
clic droit coûterait l'un des deux coins de WorldEdit, et c'est le geste que
tout utilisateur de WorldEdit connaît par cœur. Le piège d'`ExeWorldEdit` —
*un outil qui coupe la caméra entière enferme l'utilisateur* — ne peut pas se
produire ici : la caméra a un bouton à elle, qu'aucun outil ne prend.

✅ Posé et testé sans écran : `tf-render/src/controles.rs`. La pente est
bornée à un cheveu de la verticale (à π/2 pile, la base de la vue dégénère et
l'image bascule d'un quart de tour), « monter » suit la verticale du MONDE
(sinon monter en piqué fait reculer), et le panoramique suit celle de
l'ÉCRAN (sinon il part à la verticale quand on regarde déjà le ciel).

**Ce que la décision impose, et qui n'est pas dans le bouton :**

1. **Le moteur n'a PAS de mode.** La faute tentante est de le laisser
   descendre dans `tf-ops` — une opération qui se comporterait autrement en
   Conception. Une opération prend une portée et rend des bornes ; elle ne
   sait pas quel geste l'a déclenchée, et une sélection de Conception se
   RÉSOUT en volumes avant de l'atteindre. C'est le corollaire direct de « le
   monde reste souverain ».
2. **La sélection ne se jette pas au changement de mode**, sinon le bouton
   coûte un travail. Une `BBox` reste une `BBox` ; elle devient simplement une
   chose qu'on pousse-tire au lieu d'une chose qu'on remplit.
3. **Le mode se persiste** avec le projet — le remettre à chaque ouverture
   serait le rendre pénible pour rien.
4. **Un greffon déclare dans QUEL mode vit son outil.** Sans ça, la barre de
   l'Édition finit par porter les outils de conception, et réciproquement.

**Deux règles d'architecture, et elles ne se retrofitent pas :**

- **le moteur vit dans un fil à part, l'interface ne bloque jamais.** Une
  opération de trois secondes ne doit pas figer la fenêtre — **reste à
  faire, et c'est ce qui bloque « Appliquer » ;**
- ✅ **les formulaires se GÉNÈRENT depuis les descripteurs d'opérations.**
  `ExeWorldEdit` l'a prouvé : aucun formulaire n'y est écrit à la main, et
  ajouter une opération n'y demande aucune ligne d'interface. Fait, et tenu
  par deux tests plutôt que par une convention.

Le socle de fenêtre : `winit` + `egui`, caméra, sélection au cuboïde avec
poignées, palette de blocs avec icônes, historique visible et cliquable.

**Et c'est ici qu'arrive le premier tiers de SketchUp**, parce que c'est un
geste et pas une structure de données :

| | Pourquoi maintenant |
|---|---|
| ✅ **Pousser-tirer** une face de la sélection | un geste + un remplissage. Fait : on attrape une face au clic, on tire, et l'opération n'écrit que la TRANCHE. Tirer pose la matière, pousser pose de l'air |
| ✅ **L'inférence** — accrochage aux coins, arêtes, milieux, axes, plans et alignements de ce qui est déjà bâti | c'est ce qui FAIT SketchUp. Pure géométrie, zéro impact moteur : ça vit entièrement dans la coque. Faite, et elle DIT à quoi elle tient, axe par axe |
| **Saisie chiffrée** pendant le geste — taper `12` en tirant | trivial une fois le geste là, et c'est la moitié de la précision de SketchUp |

L'inférence est le morceau le plus difficile à rendre JUSTE de toute la
phase — pas à écrire, à régler. Sur une grille de blocs elle est plus simple
qu'en CAO ; ce qui compte est d'accrocher à ce qui est *bâti* (le nu d'un mur,
l'axe d'une colonne, la hauteur de la fenêtre d'à côté), pas seulement à la
grille.

> **Sortie.** Ouvrir une save, voler dedans, tirer un mur de vingt blocs à la
> souris en accrochant au nu du bâtiment d'en face, annuler. Sans écrire une
> seule commande.
>
> ✅ **Atteinte**, à une réserve près qui est écrite dans les trous : les
> références d'accrochage sont celles de la sélection de DÉPART — ses coins,
> ses arêtes, son milieu — et pas encore celles du bâtiment d'en face, parce
> qu'il n'existe aucune notion d'objet voisin. Le mécanisme est le même ; il
> lui manque des points à mordre.

## Phase 5 — Le streaming piloté par la caméra

La fenêtre de résidence existe et est testée ; rien ne la pilote. Tant qu'on
affiche une zone choisie à la main, ça ne se voit pas — dès qu'on vole dans un
monde de 800 régions, c'est bloquant.

✅ **Le remaillage INCRÉMENTAL est fait** — une opération ne remaille que ses
bornes, plus une case de débordement — et il a coûté une leçon : écrit seul, il
gagnait **× 1,1**, parce que la relecture pesait cent fois le maillage. Une fois
`sections_de` corrigé (filtrer avant d'inflater, honorer la hauteur), c'est
× 7 sur 64 chunks, et le chargement en profite autant. Détail et chiffres :
`docs/ETAT.md`.

✅ **Les deux arènes se remplacent par TRANCHES.** Le chiffre « 3,5 ms sur
4,4, donc pas encore le bon combat » avait été pris sur du terrain, où une
section homogène rend six quads. Repris sur du BÂTI, à 256 chunks : une
édition de trois blocs coûtait **80 ms**, dont 52 dans l'arène des quads et
23 à 29 dans la passe de modèles. **80 → 27 ms.** Ce qui reste est de la
bande passante — recopier 276 k instances et 178 k poses — et descendre sous
8 ms demande des tampons GPU par SECTION au lieu d'un tableau à plat. C'est
la même pièce dont la résidence a besoin.

**Ce que coûte une région, mesuré avant d'écrire quoi que ce soit**
(`cargo run --release -p tf-app --example residence`, médiane de 3, écart
entre passes < 2 %, détail dans `docs/ETAT.md`) :

| | lire | décoder | mailler | TOTAL | résident |
|---|---:|---:|---:|---:|---:|
| `Terrain` | 4,2 ms | 114 ms | 102 ms | **220 ms** | 27,9 Mo |
| `Build` | 13 ms | 449 ms | 406 ms | **867 ms** | **186 Mo** |

Quatre conclusions, et elles décident la forme de la phase :

1. **La contrainte qui mord est la MÉMOIRE.** Deux gigaoctets ne tiennent que
   ONZE régions bâties sur les 800 annoncées. L'éviction n'est pas une
   finition, c'est le sujet — et c'est pour ça que `Residency` est plafonnée
   en octets depuis le premier jour.
2. **Aucune région ne se charge en une image** : 867 ms font 108 images à
   8 ms. Le chargeur vit donc dans un FIL, et le fil principal ne fait que
   poser ce qui est prêt. Même règle d'architecture que le fil moteur, et elle
   ne se retrofite pas davantage.
3. **On lit à la RÉGION, on décode au chunk.** Un chunk demandé seul coûte
   × 10 d'un chunk amorti, parce que le `.mca` est relu à chaque appel :
   4,4 s gaspillées par région. L'unité de LECTURE n'est pas l'unité
   d'affichage, et c'est la mesure qui le dit.
4. **Dimensionner sur `Terrain` déborderait d'un ordre de grandeur.** Sur du
   sous-sol la grille pèse 45 fois le maillage ; sur du bâti le maillage passe
   DEVANT. Les deux postes du budget se mesurent sur `Build`.

✅ **La DEMANDE est faite** (`tf-world/src/demande.rs`) : ce que la caméra
veut, dans quel ordre, et ce qu'il faut jeter. Pure, donc vérifiable sans
monter une machine — dix tests, cinq mutations, zéro survivant après deux
corrections. Un DISQUE de cellules et non un carré (30 % de moins à horizon
égal, et invariant par rotation) ; l'appartenance décidée sur la GRILLE de
cellules, l'urgence sur la position réelle — les mélanger jetait la cellule
où l'on se tient ; devant avant le dos, continûment ; et `planifier` par
ensembles, parce qu'en tranches il prenait 11,4 ms à rayon 40 pour décider
quatre-vingts chargements. Et `par_region` groupe la demande en LECTURES —
une région lue une fois — parce qu'un chunk demandé seul coûte × 10 d'un
chunk amorti.

✅ **Le FIL de chargement existe** (`tf-app/src/chargeur.rs`) : une lecture
par RÉGION, une réponse par CELLULE, la plus urgente d'abord, et une demande
neuve remplace la périmée au lieu de s'y ajouter. Huit tests, quatre
mutations, zéro survivant — dont deux propriétés qui ne se voient nulle part
ailleurs : qu'il ne fait jamais attendre l'hôte (fil témoin à attente bornée)
et qu'il ne lit un `.mca` qu'une fois (source qui COMPTE ses lectures, là où
un chronomètre dépendrait de la machine).

✅ **Le BRANCHEMENT est fait** (`Ouvert::integrer`) : les tables d'états
rendues sont fusionnées, les sections posées, les tranches remplacées — et la
scène streamée est identique, quad par quad, à celle qu'un chargement d'un
bloc donne. Par LOT et non par cellule : la recopie d'arène est en O(scène),
donc une par une on la paie N fois — mesuré, 4 577 ms contre 826 pour
197 cellules.

✅ **La MÉMOIRE est bornée** (`Ouvert::peser`, `a_degager`). La fenêtre de
résidence sert de COMPTABLE et non de magasin : les blocs restent dans la
grille, le maillage dans le chantier, et elle ne tient que le poids et la
récence. Une cellule pèse ses sections, son `Vec<Quad>` et sa copie packée —
les trois, parce qu'en ne comptant que la forme GPU on sous-compte le
maillage d'un tiers.

Trois choses s'y sont décidées à la mesure :

1. **Le poids ne se connaît qu'APRÈS le maillage.** Le maillage d'une région
   bâtie pèse 168 fois celui d'une région de terrain (101 Mo contre 0,6) :
   l'estimer avant reviendrait à inventer un facteur que la mesure dément.
   L'éviction qu'une pesée déclenche part donc à l'appel SUIVANT, dans le
   même remaillage que les arrivées — une seule recopie d'arène par appel au
   lieu de deux, et un `integrer` sur lot vide suffit à converger.
2. **Une cellule MAIGRIT quand sa voisine arrive**, ses faces de bord cessant
   d'être exposées. Ne repeser que les arrivées laisserait chaque cellule
   inscrite au poids qu'elle avait SEULE. On repèse donc toute résidente
   qu'un remaillage a touchée — par `Residency::update`, qui corrige le poids
   sans toucher à la récence : `insert` ferait garder au LRU exactement ce
   qu'il faudrait lâcher, et la traînée d'un vol en ligne droite ne serait
   jamais rendue.
3. **La zone d'OUVERTURE s'inscrit comme le reste.** Sans ça elle n'est
   jamais évinçable : on ouvre un monde, on vole cinq mille blocs plus loin,
   et les chunks du départ restent là pour toujours — la fuite même que la
   fenêtre existe pour empêcher, invisible tant qu'on ne regarde que ce qui
   arrive.

Six tests, huit mutations, zéro survivant. Deux propriétés portent le reste :
la comptabilité est EXACTE (ce que la fenêtre croit tenir et ce que la scène
porte sont le même nombre à l'octet près), et ce qui survit à l'éviction est
quad pour quad ce qu'un chargement direct donnerait — c'est le seul test qui
voit la marge du dégagement, retirer une cellule DÉCOUVRANT les faces de ses
voisines. Mesuré sur du bâti : 197 cellules, 20,2 Mo ramenés à 6,6 sous un
tiers de budget, 39 évictions, **zéro rechargement de zone**.

Au passage, un défaut latent : la scène retrouvait ce qu'une cellule portait
en filtrant la grille sur `adresse.0 == cellule.x`. Vrai au niveau chunk,
faux d'un facteur mille au niveau RÉGION — une cellule de région en couvre
32 × 32, donc 1 023 colonnes sur 1 024 seraient restées dans la grille pour
toujours, invisibles puisque l'affichage, lui, aurait été juste. Les adresses
se déduisent maintenant de la GÉOMÉTRIE de la cellule, ce qui supprime aussi
un balayage en O(scène × cellules).

✅ **La CAMÉRA pilote** (`tf-app/src/pilote.rs`, `--rayon`). La boucle
caméra → demande → fil → scène vit dans la BIBLIOTHÈQUE et pas dans le
gestionnaire d'image : écrite dans la fenêtre, elle aurait été derrière un
serveur graphique, donc jamais vérifiée. On lui donne un œil et un regard, ce
qui permet de faire VOLER une caméra dans un test.

Elle a immédiatement montré trois défauts qu'aucune des trois pièces ne
pouvait voir seule, et qui étaient tous silencieux :

1. **Redemander à chaque image** remplace la file du chargeur soixante fois
   par seconde, donc annule en boucle la région qu'il est en train de lire.
   `Suivi` (pur, sept tests) ne redemande qu'en FRANCHISSANT une frontière de
   cellule, ou quand le fil est au repos — ce second cas étant ce qui rattrape
   une cellule évincée alors qu'elle est encore dans le champ. Mesuré : **une
   seule lecture de `.mca` pour 326 images**.
2. **Un budget plus petit que le champ de vision fait tourner la machine à
   vide** : le LRU évince une cellule qu'on regarde, la demande la redemande,
   elle en évince une autre, sans fin. Mesuré **180 évictions en 100 images**
   sur une scène qui n'avance pas d'un bloc — la fenêtre répond, le fil
   tourne, le disque chauffe, et rien ne le dit. Ce que la caméra regarde est
   désormais ÉPINGLÉ : le LRU ne prend que dans la traînée, et si le champ
   seul ne tient pas, on dépasse le budget en le DISANT (`deborde`). Après :
   zéro éviction en 100 images. Corollaire mesuré par mutation : le champ
   épinglé se recalcule à chaque image, pas seulement quand une demande part —
   sinon voler au-dessus d'un terrain déjà chargé ne l'actualise jamais et la
   mémoire ne redescend plus.
3. **Le canal des réponses n'était pas borné.** Le fil lit une région d'un
   coup et émet ses cellules ; l'hôte n'en intègre que deux par image. Mesuré
   sur un vol de soixante chunks, il recevait encore des cellules **300 images
   après s'être arrêté**, et les sections décodées s'empilaient hors de tout
   budget — la fenêtre de résidence ne compte que ce qui est POSÉ. Le canal
   est borné à 64 cellules d'avance : le fil attend quand l'hôte est en
   retard, ce qu'il aurait décodé de plus étant de toute façon périmé au
   premier mouvement de caméra.

Six tests de vol, six mutations, zéro survivant.

**Ce qui reste** : l'arène GPU par SECTION, dernière dépense en O(scène) du
chemin d'édition (27 ms pour recopier 276 k instances) — c'est elle qui fixe
aujourd'hui les deux cellules par image, et la même pièce permettrait de
lâcher un maillage sans recopier le reste.

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
| **Réévaluation** | une entrée de journal qu'on rejoue depuis ses paramètres, pas depuis ses octets — la couture est posée (`Genre::Operation::params`), reste à écrire ce qu'on y met |
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

**La phase 3 est faite**, et elle a bien été presque gratuite vu ce qui était
déjà dérivé : le socle WorldEdit est complet, déplacements compris.

**La phase 4 est le tournant, et elle est commencée** : la fenêtre existe, on
vole dans une vraie save, on vise, on sélectionne et on s'accroche à ce qui est
bâti. Ce qui reste est ce qui la rend UTILE — brancher les opérations, par des
formulaires engendrés depuis les descripteurs du moteur, et le pousser-tirer.
Sans ça rien n'est jugeable, et c'est là que titiforge commence à ressembler à
SketchUp plutôt qu'à une ligne de commande.

**La phase 7 est la raison d'être.** Elle peut attendre, mais sa couture non :
elle est décidée en tête de ce document, et les phases 3 et 4 doivent être
écrites en la respectant — c'est-à-dire en gardant les paramètres des
opérations, pas seulement leurs octets.
