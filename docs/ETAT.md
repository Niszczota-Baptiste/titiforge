# État de titiforge — ce qui est mesuré, ce qui est vérifié, ce qui manque

Une page, tenue à jour. `RESULTATS.md` garde le détail chronologique et le
raisonnement ; celle-ci donne les chiffres et **la commande qui les rejoue**.

Un chiffre sans sa commande n'est pas une mesure, c'est une affirmation.

Machine de référence : Intel Xeon 2,10 GHz, **4 cœurs**, 15 Go, Linux.

**Les temps ABSOLUS dérivent avec la charge de la machine** — mesuré le même
jour, à code identique, un facteur **2,4** entre deux séances sur les six
scénarios d'étages à la fois. Seule une comparaison A/B dans la MÊME séance
prouve quelque chose ; c'est le seuil de 25 % du dépôt, en plus brutal que
prévu.
Adaptateur graphique : **llvmpipe** (rastériseur LOGICIEL) — les temps de rendu
n'y valent rien, **les comptes si**.

---

## En un coup d'œil

| | |
|---|---:|
| Tests | **783**, zéro échec |
| `cargo clippy --all-targets` | propre |
| Crates finis | tf-nbt · tf-anvil · tf-world · tf-blocks · tf-ops · tf-mesh · tf-assets · tf-render |
| Crates commencés | **tf-app** — la coque : fenêtre `winit`, interface `egui`, et le même rendu hors écran |
| Crates non commencés | **tf-formats** (schematics) |
| Vraies saves vérifiées | 2 — Minefield 1.18, vanilla 1.20.1 |
| Blocs réellement réécrits puis annulés au bit près | **606 M** |
| Coût du suivi des block entities sur le balayage | **nul** (6,04 ms contre 6,07, médiane de 3) |
| Appels de dessin, quelle que soit la scène | **2** |
| Résident par région bâtie | **186 Mo** — 2 Go n'en tiennent que **11** |
| Charger une région bâtie | **867 ms**, soit 108 images à 8 ms |

---

## 1. Les tests

```bash
cargo test --workspace
```

783 tests, répartis par ce qu'ils PROUVENT :

| Famille | Tests | Ce qu'elle tient |
|---|---:|---|
| `tf-nbt` reader/writer | 29 | longueurs signées, profondeur, charges forgées |
| `tf-anvil` region/section/lossless/versions/external/robustesse | 102 | round-trip octet pour octet, 1.13→1.21, `.mcc` |
| `tf-anvil` biomes | 11 | la SECONDE palette : liste de chaînes, 64 cellules, pas de plancher à 4 bits |
| `tf-anvil` entites | 13 | les block entities : repérage, déplacement, disposition `Level` |
| `tf-anvil` croisement | 2 | un `.mca` écrit par un **producteur tiers** (le moteur JS) |
| `tf-world` journal/staging/residency/coords/source/lecture | 115 | annuler ↔ refaire sur le CONTENU, division plancher, emprise bornée ; et qu'une entrée porte de quoi se REJOUER |
| `tf-blocks` regles | 21 | lois du groupe, et le contrôle de FORME indépendant |
| `tf-ops` etages/edition/presse/tirage + 3 unitaires | 64 | les trois étages, la jonction rapport → journal, le presse-papiers, le hachage par plan ; et qu'une sélection démesurée est REFUSÉE ou raccourcie, jamais tentée |
| `tf-ops` coffres | 11 | copier → tourner → coller emporte le contenu des coffres |
| `tf-ops` deplacer | 6 | `//move` et `//stack`, et l'annulation d'une opération à PLUSIEURS passes |
| `tf-ops` biome | 10 | `//setbiome`, et que sa grille est de 4 blocs et pas d'un |
| `tf-ops` relief | 10 | la carte de hauteurs, et l'unité qui traverse la frontière |
| `tf-ops` naturaliser | 11 | la portée `Colonne`, et qu'une colonne n'a qu'UNE surface |
| `tf-ops` forme | 16 | le verdict par section d'une forme, croisé aux 4 096 cases |
| `tf-ops` creuser | 12 | `//hollow` : le critère est TOPOLOGIQUE, et un coffre vidé ne revient pas en fantôme |
| `tf-assets` climat | 12 | la couleur d'un biome, DÉRIVÉE : table × température |
| `tf-assets` pack/textures/rotation/jeu/codex_reel | 65 | parents, uv, atlas, `.jar`, détection d'installation |
| `tf-mesh` biomes | 7 | le biome traverse jusqu'au quad, et ne coupe QUE les teintés |
| `tf-mesh` mailler/chantier | 33 | glouton contre naïf, case par case ; et le remaillage PARTIEL : la marge d'une case, l'ordre qui reste trié, et une section vidée qui perd son maillage |
| `tf-render` rendu | 26 | **au pixel** : ombrage, teinte, dalle, alignement WGSL ; que la teinte de biome atteint AUSSI les blocs-modèles ; que le quadrillage est dans la MÊME unité que la géométrie ; et qu'une DEMI-teinte ne se délave pas — les primaires saturées sont des points fixes de la conversion sRGB et ne prouvaient rien |
| `tf-render` controles | 12 | le pilotage : le JOUEUR est le point fixe, et les bornes qui évitent une vue dégénérée |
| `tf-render` viser | 19 | quel bloc et quelle FACE sous le curseur ; que poser et casser ne visent pas la même case ; que les DEUX tables de directions disent la même chose ; et le GESTE SketchUp complet, de bout en bout |
| `tf-world` decoupe | 17 | les cellules de chunk et de `.mca` ; que `//chunk` ÉTEND sans rétrécir ; et qu'il ne suffit PAS à l'étage palette sans la hauteur |
| `tf-world` selection | 24 | deux coins, `//expand` qui ne se retourne pas, la face qu'on attrape, le VERROU que la pose impose, et le POUSSER-TIRER : combien de blocs un rayon désigne le long d'un axe, et la TRANCHE que le geste écrit |
| `tf-world` inference | 15 | accrocher à ce qui est bâti : un axe = un plan, deux = une droite, trois = un point |
| `tf-app` etat | 37 | la JONCTION que la coque fait : viser → accrocher → poser, et que l'axe de POSE ne s'accroche pas ; que le verdict de sélection se COMPTE |
| `tf-app` chantier | 9 | la jonction coque ↔ fil : ce que le fil écrit, la coque le RELIT — depuis la copie de travail, jamais depuis la source ; que la SAUVEGARDE porte le monde d'AVANT octet pour octet ; et que le remaillage INCRÉMENTAL donne exactement la même scène qu'un rechargement complet, y compris quand une section se vide ou qu'un état inconnu apparaît (demande `TF_PACK`) |
| `tf-app` moteur | 9 | le fil : que l'interface ne bloque JAMAIS, qu'une commande rend exactement une réponse, et qu'une commande fautive revient en échec sans tuer le moteur |
| `tf-app` interface | 9 | que CHAQUE genre de paramètre a son champ — le formulaire se génère, il ne s'écrit pas ; et qu'une valeur du mauvais genre est refusée au lieu d'être convertie |
| `tf-ops` executer | 12 | la boucle complète depuis un NOM : chaque opération du catalogue s'exécute vraiment, la source reste intacte, annuler rend le monde d'avant OCTET pour octet — et un `//move` qui se chevauche s'annule dans le bon ORDRE |
| `tf-ops` catalogue | 18 | que la description et l'opération ne peuvent pas diverger : chaque descripteur se construit, construit CE qu'il nomme, et passe par un normaliseur idempotent que personne ne peut sauter |
| `tf-world` demande | 10 | ce que la caméra demande et dans quel ORDRE : un disque et pas un carré, devant avant le dos, l'appartenance décidée sur la GRILLE et l'urgence sur la position réelle — et qu'un regard vertical classe à la distance plutôt qu'en `NaN` |
| `tf-app` menage | 1 | qu'une copie de travail abandonnée par un arrêt brutal finit par partir — et qu'une séance qui édite depuis plus d'un jour NE part pas, parce que l'âge se mesure sur le fichier le plus récent et pas sur le dossier |
| `tf-bench` fixture/build | 12 | l'échantillon reste représentatif du pack |

Trois propriétés valent d'être nommées :

- **Les tests de rendu comparent des PIXELS.** Un ombrage inversé a vécu des
  années dans `we-engine` parce qu'il était invisible sur un build gris.
- **Certains tests ne tournent que si on leur donne la donnée** — le pack réel
  (`TF_PACK=…`) et le `.mca` du moteur tiers (`TF_MCA_TIERS=…`). Un test qu'on
  ne peut pas jouer sans une donnée privée ne doit pas casser la suite de
  quelqu'un qui ne l'a pas, mais il doit EXISTER.
- **Deux passes sur le même chunk s'annulent À L'ENVERS.** Chaque correctif
  est gardé par l'empreinte de l'état qu'il attend ; `//move` repasse sur les
  chunks que source et destination ont en commun, et rejouer ses correctifs
  dans l'ordre d'enregistrement fait échouer le second sur `Divergence`. Le
  sens est donné une seule fois, par `Entree::a_annuler`, et le test de
  `deplacer` le prouve — aucune opération simple ne repasse deux fois sur un
  chunk, donc rien d'autre ne pouvait le voir.
- **Les tests de `coffres` ont été vérifiés PAR MUTATION.** Quatre pièces de la
  chaîne ont été cassées une par une — la copie ne ramasse plus, le collage ne
  pose plus, l'orphelin n'est plus détecté, la rotation ne déplace plus les
  cases — et on a exigé qu'un test tombe à chaque fois. La quatrième
  (« une entité posée s'AJOUTE au lieu de remplacer en place ») ne tombait
  d'abord chez personne : c'est la fixture qui était trop clémente, ses `y`
  croissant dans l'ordre du fichier. Un test vert ne dit rien tant qu'on n'a
  pas vu ce qui le fait rougir.
- **L'AUDIT de fin de phase a trouvé quatre trous, tous du même genre.** Les
  formes (sphère, cylindre, pyramide, murs, faces), le comptage, la graine et
  l'accrochage à la POSE étaient écrits, testés, offerts par la ligne de
  commande — et inatteignables depuis l'interface : trois réglages câblés en
  dur dans un bouton, et une fonction que seuls ses propres tests appelaient.
  « Déclaré, branché, testé — et inatteignable » ne se referme pas une fois
  pour toutes ; il se rouvre chaque fois qu'un hôte nouveau n'expose pas ce que
  le moteur offre. Quatre mutations le tiennent maintenant fermé.
- **L'accrochage du pousser-tirer aussi, et c'est lui qui a le plus appris.**
  Quatre mutations, TROIS survivantes au premier tour. La première disait que
  j'avais écrit du code mort : les deux axes verrouillés « parce qu'une face ne
  bouge que le long de sa normale » n'avaient aucun effet, puisqu'on ne relit
  que l'axe de la face. Ils sont partis. Les deux autres disaient que la
  FIXTURE ne séparait pas les cas — tous mes tirages visaient la face EST, donc
  le signe d'une face négative pouvait être faux sans que rien ne rougisse ; et
  la tolérance nulle n'était vérifiée que sur un tirage qui ne tombait sur
  aucune référence, donc la garde pouvait disparaître. Deux tests de plus, et
  les trois tombent.
- **Le pousser-tirer aussi.** Huit mutations : la garde de colinéarité ramenée
  à zéro pile, le tirage tronqué au lieu d'arrondi, un signe faux dans la
  projection, la tranche décalée d'un bloc dans les deux sens, une tranche
  rendue pour un non-changement, le tirage qui cumule, la tranche remplacée par
  la sélection entière, et « pousser » qui ne retire plus la matière. Une
  neuvième a SURVÉCU, et c'est noté dans le code : remplacer le refus d'un
  rayon colinéaire par un `unwrap_or(0)` ne fait rougir personne, parce que
  `agrandir` rend faux sur un non-changement et fait sortir au même endroit.
  Le refus reste — sans lui, le geste dépendrait de ce que `agrandir` décide
  d'un zéro, qui n'est pas une promesse faite là.
- **Le fil moteur aussi.** Six mutations : `recevoir` qui bloque, le compteur
  d'en-vol qui ne retombe pas, `annuler` qui prend le mauvais sens, une
  opération sans effet annoncée comme faite, la forme perdue en route. La
  première a d'abord fait PENDRE la suite au lieu de la faire rougir — un test
  qui pend ne dit pas ce qui ne va pas, il dit seulement qu'on a attendu. Le
  test tourne donc dans un fil témoin avec une attente bornée, et la même faute
  sort maintenant une phrase.
- **L'exécuteur et le rejeu aussi.** Six mutations : `//hollow` qui ne pose
  plus d'air (donc ne change rien), `//stack` qui avance d'un bloc au lieu de
  la taille de la sélection, le rejeu qui ignore l'empreinte de garde, le rejeu
  qui prend les correctifs dans le sens d'enregistrement, le lissage qui ne
  déborde plus sur Z, la forme ignorée. La quatrième a d'abord SURVÉCU : tant
  qu'une opération ne touche chaque chunk qu'une fois, les deux sens rendent le
  même ensemble et seul l'ORDRE diffère. Il a fallu un `//move` dont la source
  et la destination se chevauchent — c'est le seul motif qui les sépare, et
  c'est la leçon déjà écrite dans ce dépôt, re-payée à l'identique.
- **Le catalogue d'opérations aussi, et son formulaire.** Cinq mutations sur
  `tf-ops/catalogue` (la normalisation sautée, un bras de `match` recopié,
  `//replace` qui perd son masque, l'inconnu ignoré au lieu d'être refusé, un
  serrage sans borne basse) et cinq sur la coque (le mélange sans champ, la
  conversion silencieuse du mauvais genre, les paramètres d'une opération qui
  survivent à un changement d'opération, `//hollow` qui n'annonce plus ce
  qu'il matérialise, une portée `Colonne` qui promet l'étage palette). Dix
  mutations, aucun survivant.
- **Les tests de la coque aussi.** Cinq mutations, cinq tests rouges : le
  verrou d'axe retiré, la conversion `Face → Direction` décalée d'un axe, la
  pose qui ignore l'accroche, le réticule qui n'est pas remis à zéro quand on
  ne vise plus rien, et le verdict de sélection DÉDUIT de « alignée sur les
  chunks » au lieu d'être compté. Le premier test porte en plus son propre
  TÉMOIN : il vérifie que sans le verrou la pose tomberait vraiment dans le
  mur, sinon il serait vert même si le danger n'existait pas.

---

## 2. Vérifié sur de vraies saves

C'est ce qui compte le plus : jusqu'à ces relevés, le décodeur et l'encodeur ne
s'étaient jamais mesurés qu'à eux-mêmes.

```bash
cargo run --release -p tf-ops --example verite_terrain -- <monde> <bloc>
```

On n'exige pas la même empreinte, on exige les mêmes **octets** — une empreinte
qui collerait sur des octets différents serait une collision.

| | Minefield 1.18 | Mosslorn 1.20.1 |
|---|---:|---:|
| Régions · taille | 5 · 6,2 Mo | 5 · **82,5 Mo** |
| Chunks · sections | 1 481 · 35 546 | 5 120 · **127 921** |
| `//replace`, étage palette | 71 ms · 582 861 blocs | **1 015 ms** · 7 409 907 blocs |
| étages touchés | rien 34 403 · palette 1 141 · **bloc 0** | rien 100 166 · palette 22 714 · **bloc 0** |
| mélange, étage bloc | 1 121 ms · 145 296 392 blocs | **3 706 ms · 460 876 778 blocs** |
| chunks refaits puis annulés, **octet pour octet** | 1 141 + 1 481 | 5 052 + 5 120 |
| chunks non modifiés, vérifiés **au ZLIB près** | 288 | 68 |
| **contrôles · fautes** | 15 034 · **0** | **40 756 · 0** |

**606 millions de blocs** réécrits, recompressés et annulés au bit près, sur
deux versions du format et deux mondes sans rien de commun.

Corroboration croisée : le recensement compte 582 861 blocs de terre en
dépaquetant chaque indice ; l'opération en compte 582 861 par sa palette, sans
en dépaqueter un seul. Deux chemins sans code commun, le même nombre.

### Ce que ces mondes contiennent

```bash
cargo run --release -p tf-assets --example recenser_monde -- <monde> [assets] [--zone …] [--mailler]
```

| | Minefield (plot plat) | Mosslorn (ville dense) |
|---|---:|---:|
| Blocs balayés | 145 588 224 | **503 316 480** |
| Blocs posés | 1 199 316 — **0,8 %** | 152 033 934 — **30,2 %** |
| États distincts | 1 124 | **3 602** |
| Palette d'une section | médiane 1 · moy. 1,2 · max 134 | médiane 1 · moy. **9,7** · max **215** |
| Bits par indice | 4 → 35 423, jamais au-delà de 8 | 4 → 102 718 · 5 → 8 923 · 6 → 7 683 · 7 → 2 998 · **8 → 558** |
| Posés par chunk | médiane 1 024 | médiane **31 021** · max 63 192 |

Le second monde fait enfin travailler les chemins de dépack que le premier ne
touchait jamais.

---

## 3. Performance du moteur

### Les trois étages

```bash
cargo bench -p tf-ops --bench etages
```

Une section Anvil porte une palette et 4 096 indices. Presque toute opération
WorldEdit s'exprime sur la palette seule : le chemin rapide n'est pas « itérer
plus vite », c'est **ne pas itérer**.

Sur une **région pleine** — 24 576 sections, 100 663 296 blocs — médianes
criterion :

| Scénario | Étage atteint | Temps |
|---|---|---:|
| cible ABSENTE de la save | rien — O(palette) | **994 µs** |
| `//set` uniforme | section — O(1) | **1,072 ms** |
| `//replace` | **palette** — O(palette) | **1,381 ms** |
| `//replace`, sélection bordée d'un bloc | bloc sur 2 892 sections | 20,29 ms |
| mélange pondéré (dépend de la position) | **bloc** — O(blocs) | 951 ms |
| `//replace` en COMPTANT les blocs | palette + parcours | 29,60 ms |

Le bench **annonce la répartition des étages avant de mesurer** : un chiffre
sans le compte des étages ne prouve pas que le chemin rapide s'est déclenché.

L'étage palette **ne dédoublonne pas**, et c'est ce qui le rend possible : la
longueur de palette ne bouge pas, donc `bits` non plus, donc aucun indice n'est
touché. Dédoublonner faisait retomber 9 216 sections sur 9 216 au chemin lent.

Pour situer : le moteur JS (`we-engine`) fait le même `//replace` par bloc, en
coordonnées monde, en **11 718 ms**.

### La chaîne complète, et où part le temps

```bash
cargo bench -p tf-ops --bench edition
```

`0_chaine_complete` est le VRAI chemin — `appliquer_region` : lecture par le
staging, application, recollement, correctif de journal, écriture dans la copie
de travail. Sur une région pleine :

| | |
|---|---:|
| **chaîne complète** (parallèle, compression de staging niveau 2) | **92,84 ms** |

Les phases sont mesurées **séparément**, et en séquentiel — « sans ça on
optimise au hasard ». Elles ne s'additionnent donc pas à la ligne ci-dessus ;
elles en donnent la FORME :

| Phase, isolée | Temps |
|---|---:|
| `deflate` (niveau 6, séquentiel) | **357,32 ms** |
| `inflate` | 55,52 ms |
| appliquer + encoder les sections | 33,55 ms |
| balayer + décoder | 17,45 ms |
| recoller (`splice`) | **1,71 ms** |

C'est la découpe qui a évité de travailler pour rien : j'allais paralléliser le
CALCUL — les 1,381 ms ci-dessus — alors que la recompression pèse l'essentiel.
Le calcul soigneusement optimisé représentait **0,17 %** de la chaîne d'alors.

Ce qui l'a réglé : paralléliser par CHUNK (× 3,65 sur 4 cœurs, 91 %
d'efficacité), puis choisir le niveau de compression de la copie de travail
plutôt que de le subir — niveau 2 contre 6, deux fois plus rapide pour 11,8 %
de taille en plus. Le niveau 9 coûte **huit fois** le niveau 6 pour 6 % de
gain : jamais. Une copie de travail se réécrit à chaque opération pendant que
l'utilisateur attend ; la sauvegarde finale ne s'écrit qu'une fois.

```bash
cargo run --release -p tf-ops --example empreinte                          # parallèle
cargo run --release -p tf-ops --example empreinte --no-default-features    # séquentiel
```

Les deux rendent le **même octet**. Un résultat qui dépendrait du nombre de
cœurs donnerait deux mondes différents de la même opération, et un journal qui
ne défait pas ce qu'il croit défaire.

### Deux coûts à connaître

| | |
|---|---:|
| Compter les blocs modifiés | **× 21,4** l'opération (1,381 → 29,60 ms) |
| Sélection décalée d'un bloc | **× 14,7** (1,381 → 20,29 ms), 2 892 sections tombent à l'étage bloc |

À l'étage palette, compter est exactement le parcours qu'on vient d'éviter : le
comptage exact est donc une OPTION (`Plan::en_comptant`), jamais un service
rendu d'office. Un rapport qui rend `None` est plus honnête qu'un chiffre qui a
coûté vingt fois le travail.

Et la sélection non alignée est la prochaine optimisation désignée — une
sélection d'utilisateur ne tombe presque jamais sur un multiple de 16. Une
section partiellement couverte dont la partie couverte est uniforme peut encore
éviter le parcours.

### Une forme répond par SECTION avant de répondre par case

```bash
cargo run --release -q -p tf-ops --example editer -- /tmp/monde-essai --sel "0,-40,0,63,20,63" --poser minecraft:stone --sphere 20
```

Une sphère, un cylindre, une pyramide ne sont pas des masques : un masque est
un prédicat sur l'ÉTAT, une forme sur la POSITION. Les mélanger ferait
retomber `//replace` à l'étage bloc. Une forme a donc son propre chemin
rapide, sur l'autre axe : là où le masque répond sur la PALETTE, la forme
répond sur la SECTION, et en une seule fois pour ses 4 096 cases.

| Verdict | Ce qu'il évite |
|---|---|
| `Dehors` | la section n'est même pas décodée |
| `Dedans` | l'étage palette reste ouvert — une sphère pleine s'écrit en O(palette) en son cœur |
| `Partielle` | seule la coque paie le parcours par bloc |

Les trois sont EXACTS, et c'est le test qui le dit : il compare le verdict aux
4 096 cases, section par section, sur toutes les formes et une couronne
autour. Un « Dedans » faux écrirait hors de la forme, un « Dehors » faux y
laisserait un trou, et ni l'un ni l'autre ne se voit sur une capture d'écran.

Le volume est croisé avec un compteur INDÉPENDANT : une sphère de rayon 20
écrit **36 137** blocs et sa coque d'épaisseur 2 en écrit **9 392** —
exactement ce qu'un parcours naïf compte, au bloc près.

**Ce que les formes coûtent aux opérations qui n'en ont pas.** Mesuré binaire
contre binaire, alternés, médiane de 3 — pas de `git stash` entre deux
exécutions, les deux binaires sont construits une fois pour toutes :

| | avant | après | |
|---|---:|---:|---|
| `//replace` cible absente | 1,87 ms | 1,95 ms | +4,5 % |
| `//set` uniforme | 1,75 ms | 1,94 ms | +10,6 % |
| mélange, étage bloc | 1 217 ms | **1 190 ms** | **−2,2 %** |

L'étage BLOC — le seul qui compte à grande échelle — est à parité, et ça a
demandé deux corrections : `couverture` en ligne (le cas `Boite` se réduit à
un test de discriminant) et le test de forme passé en paramètre de
COMPILATION (`etage_bloc::<BORDE>`). Un branchement par bloc, même
parfaitement prédit, coûtait 5,9 % sur cent millions de cases.

Les deux premiers sont la répartition d'étage, et **elle pèse 0,17 % d'une
opération réelle** — 1,35 ms sur 800, le reste étant la recompression.
Continuer à les chasser serait le piège n° 1 du dépôt, pour la troisième
fois.

### Les coffres suivent les blocs, et ça ne coûte rien

```bash
cargo run --release -q -p tf-ops --example semer  -- /tmp/monde-essai 1
cargo run --release -q -p tf-ops --example editer -- /tmp/monde-essai --sel "0,-40,0,31,-20,31" --copier-vers "0,0,0" --avec-air
```

Le contenu d'un coffre n'est pas dans la grille de blocs : c'est une liste à
part du chunk, dont chaque entrée porte ses propres coordonnées MONDE. Une
entrée voyage par ses OCTETS et **seules ses trois coordonnées sont réécrites**
— douze octets, à une position que le balayage a relevée. Rien n'est ré-encodé,
donc le contenu du coffre, le texte d'un panneau et les données d'un mod ne
peuvent pas être abîmés : c'est la même propriété structurelle que le splice.

| | |
|---|---:|
| Balayage d'une région pleine, sans le suivi | 6,07 ms |
| Balayage d'une région pleine, avec | **6,04 ms** (médiane de 3, A/B dans la même séance) |

L'écart est sous le bruit, et loin des 25 % qu'il faudrait pour annoncer quoi
que ce soit. Mesuré à `--quick`, le même A/B annonçait **−25 %** : un raccourci
de mesure n'est pas une mesure.

Le contrôle qui compte n'est pas un temps, c'est un NON-changement. Copier un
extrait, le tourner quatre fois — donc l'identité — et le reposer à sa propre
place doit rendre **zéro chunk modifié**, coffres compris :

```text
copié : 32 × 21 × 32 en 2 ms · 21 état(s) distinct(s) · 8 block entities
2 ms · 0 chunks modifiés · étages : rien 8 · section 0 · palette 0 · bloc 0
block entities : 8 posée(s) · 0 retirée(s) (leur bloc a disparu)
portée réelle : rien n'a été écrit
```

Deux règles tiennent ce zéro, et chacune a coûté une faute :

- **une entité posée prend la place EXACTE de celle qu'elle remplace.**
  Ajoutée à la fin, elle réordonnerait la liste : mêmes entrées, autres octets,
  donc un correctif de journal pour rien ;
- **le retrait se déduit de la CASE, jamais de la boîte.** Une entité dont la
  case a changé d'état part avec son bloc ; juger sur les `bornes` de
  l'opération détruirait le coffre qu'un `//replace` n'a pas touché.

---

## 4. Le rendu

```bash
cargo run --release -p tf-render --example mesurer  -- <assets> [--region]
cargo run --release -p tf-render --example capture  -- <assets> vue.png 1600 --monde <monde> --zone "…"
```

### Sur une région PLEINE d'un vrai build (Mosslorn)

| | |
|---|---:|
| Quads gloutons | 3 241 796 → **51,87 Mo** |
| Poses de modèles | 1 775 664 → 10 169 366 faces → **29,86 Mo** |
| Les mêmes faces en quads | **162,7 Mo** — × 5,4 |
| **Total GPU** | **81,7 Mo** |
| Maillage | 219 ms |
| **Appels de dessin** | **2** |

### Sur la fixture, région pleine, décor par défaut

| | |
|---|---:|
| Quads | 4 141 777 → 66,27 Mo |
| Poses | 2 385 953 → 48 520 638 faces → **39,04 Mo** |
| en quads | **776,33 Mo** — **× 19,9** |

La fixture surestime la géométrie d'un facteur deux (voir § 5) ; ce qui reste
vrai des deux côtés, c'est qu'émettre ces faces en quads coûte un ordre de
grandeur de plus que les poser.

### Deux décisions de conception, et leur mesure

**Une POSE par bloc, pas des quads.** La géométrie d'un état vit UNE fois :
deux dalles de chêne côte à côte n'ont pas deux modèles, elles ont deux
positions. La table de géométrie pèse 868 ko pour 670 états de modèle, quel que
soit le nombre de blocs qui les portent.

**Un seul appel de dessin par passe.** Chaque pose a un nombre de faces
différent — six pour une dalle, jusqu'à 492 pour le pire bloc du serveur — donc
« n faces par instance » n'existe pas. La pose porte le RANG de sa première
face dans le flot global, et le sommet retrouve la sienne par dichotomie :
vingt itérations sur un million de poses, zéro octet par face.

**Et l'instance gloutonne fait 16 octets**, pas 32 : un quad tombe toujours sur
un bord de bloc et tient dans sa section — cinq bits par axe, quatre par
dimension, trois pour la face. Sur une région, 132,54 Mo deviennent 66,27. La
vérification n'est pas un compte d'octets : la capture d'un vrai build est
**identique octet pour octet** avant et après.

### Fidélité

La couleur de l'herbe rendue est **(82, 109, 48)** contre **(84, 109, 51)** dans
le jeu. L'écart est celui entre multiplier en sRGB (ce que fait Minecraft) et
multiplier en linéaire (ce que fait un pipeline correct).

---

## 5. Les règles de transformation de blocs

```bash
cargo run --release -p tf-blocks --example deriver -- <pack>
```

910 blocs Minefield portent un état, avec quatre propriétés que le vanilla n'a
pas. Une table écrite à la main est impraticable : les règles sont **dérivées**
des `blockstates`.

| | rot 90° | rot 180° | rot 270° | miroir E-O | miroir N-S |
|---|---:|---:|---:|---:|---:|
| **minefield** (910 blocs) | 99,3 % | 99,3 % | 99,3 % | **97,0 %** | 97,0 % |
| **minecraft** (345 blocs) | 99,4 % | 99,7 % | 99,4 % | 99,7 % | 100,0 % |

Et surtout, **deux contrôles qui ne relisent pas le raisonnement** :

| | |
|---|---:|
| Lois du groupe, sur 33 844 états | **0 faute** |
| Formes comparées au SOLIDE transformé, sur 132 380 couples | 1 351 fausses |
| … dont **déjà annoncées** par la dérivation elle-même | **1 351 — toutes** |

Les lois du groupe ne savent pas voir une règle fausse : une règle qui
tournerait tout d'un quart de trop les passerait toutes. C'est le second
contrôle qui décide, et il a trouvé 3 431 couples faux pendant que le premier
était au vert. Ce qui reste est NOMMÉ (`Manque::FormeApprochee` et cie), pas
tu.

---

## 6. Ce que valent les fixtures

```bash
cargo run --release -p tf-bench --example profil_build
```

Aucune fixture binaire dans le dépôt : tout se construit depuis une graine.
Deux profils, et les confondre fausse tout — `Terrain` mesure Anvil, `Build`
mesure le rendu.

**Une correction mesurée**, sur deux mondes réels sans rien de commun :

| | fixture `Build` | Minefield | Mosslorn |
|---|---:|---:|---:|
| Blocs posés | 31,4 % | 1,3 % | **30,2 %** |
| Blocs-modèles, part des posés | 9,1 % | 7,6 % | **5,8 %** |
| Cuboïdes par bloc-modèle | 3,58 | 1,54 | **1,75** |

La densité était juste. Le nombre de cuboïdes est **deux fois trop
pessimiste** : les blocs-modèles qu'on pose vraiment sont des plantes en croix
à deux cuboïdes (`grass` 615 976 × 2, `moss_carpet` 559 948 × 1), pas des
meubles à trente.

**La fixture n'est pas recalibrée pour autant.** Lui inventer une distribution
de placement qu'on n'a mesurée que sur du vanilla la rendrait fausse sans qu'on
sache dans quel sens. Le dimensionnement ABSOLU vient des vrais mondes ; le
bench reste un instrument de COMPARAISON, où un pessimisme uniforme ne trompe
sur rien. Détail : `fixtures.md`.

---

## 6 bis. Ce qu'une région coûte à rendre résidente

```bash
cargo run --release -p tf-app --example residence
cargo run --release -p tf-app --example residence -- 16   # côté en chunks
```

La phase 5 promet « 800 régions, vol continu, RAM bornée, aucune pause > 8 ms ».
Ce chiffre se budgète, il ne s'espère pas — et ce dépôt a payé trois fois le
piège n° 1. Mesuré sur une région pleine (1 024 chunks, 24 576 sections),
médiane de 3, écart entre passes **< 2 %** :

| | disque | lire | décoder | mailler | **TOTAL** |
|---|---:|---:|---:|---:|---:|
| `Terrain` (sous-sol) | 8,4 Mo | 4,2 ms | 114 ms | 102 ms | **220 ms** |
| `Build` (bâtiment) | 24,1 Mo | 13 ms | 449 ms | 406 ms | **867 ms** |

Maillage parallèle contre séquentiel : **× 3,9** et **× 3,7** (4 cœurs). Les
deux chemins rendent le même nombre de quads — l'exemple l'affirme en
assertion, pas en commentaire.

**Ce qui reste résident**, puisque la fenêtre est plafonnée en OCTETS :

| | grille | maillage | **TOTAL** | par chunk | 2 Go tiennent |
|---|---:|---:|---:|---:|---:|
| `Terrain` | 27,2 Mo | 0,6 Mo | **27,9 Mo** | 27 ko | 72 régions |
| `Build` | 85,3 Mo | 100,7 Mo | **186,0 Mo** | 182 ko | **11 régions** |

Quatre conclusions, et elles décident la phase 5 :

1. **La contrainte qui mord est la MÉMOIRE, pas le temps.** Deux gigaoctets ne
   tiennent que **onze régions bâties** sur les 800 annoncées. L'éviction
   n'est donc pas une optimisation à ajouter plus tard, c'est le sujet.
2. **Les deux fixtures diffèrent d'un facteur 6,7 en résidence**, et elles
   n'ont pas le même poste dominant : sur du sous-sol la grille pèse 45 fois le
   maillage, sur du bâti le maillage passe DEVANT la grille (100,7 contre
   85,3). Dimensionner une fenêtre de résidence sur `Terrain` la ferait
   déborder d'un ordre de grandeur sur un vrai build. `docs/fixtures.md` le
   disait déjà pour le rendu ; ça vaut aussi pour la mémoire.
3. **Aucune région ne se charge en une image.** 867 ms font **108 images à
   8 ms**, soit 1,8 s à 60 im/s. Un vol qui traverse une région plus vite que
   ça distance le chargeur : il doit donc vivre dans un FIL, et le fil
   principal ne faire que poser ce qui est prêt.
4. **On lit à la RÉGION, on décode au chunk.** Un chunk demandé seul coûte
   4,74 ms contre 0,47 ms amorti — **× 10**, parce que le `.mca` est relu à
   chaque appel. Streamer au chunk gaspillerait **4,4 s par région** en
   relectures. C'est la mesure qui choisit l'unité de lecture, et elle dit
   l'inverse de l'unité d'affichage.

**Ce que ce relevé ne dit pas** : la part de `Build` dépend de
`densite_decor`, qui est un RÉGLAGE et non une mesure (55 % le long des murs).
Et les blocs-modèles de la fixture portent 3,58 cuboïdes là où deux mondes
réels en donnent 1,54 et 1,75 : la résidence d'un vrai build sera donc plus
basse que 186 Mo, sans qu'on sache de combien. Le chiffre qui engage est
l'ORDRE de grandeur et le rapport entre les deux postes, pas la décimale.

---

## 7. Ce qui n'est pas fait, et ce qui n'est pas mesuré

Chiffré quand c'est possible — un trou nommé vaut mieux qu'un trou tu.

| Manque | Ce que ça coûte aujourd'hui |
|---|---|
| **Les fluides ne sont pas dessinés** | 4,5 % des blocs posés de Mosslorn, dont 6,2 M d'eau. Les fluides n'ont pas de modèle de bloc dans le format : il leur faut leur propre passe |
| **`uvlock` non appliqué** | une dalle tournée montre la bonne portion de texture, pas forcément dans le bon sens |
| **Pas d'occlusion ambiante, pas de LOD** | le rendu est plat, et tout ce qui est résident est dessiné |
| **Pas de rendu indirect ni de HZB** | 2 appels de dessin suffisent aujourd'hui ; ils ne suffiront plus avec un remaillage partiel |
| **La fenêtre de résidence n'est pas branchée au rendu** | `tf-world::residency` existe et est testé ; rien ne le pilote encore depuis une caméra |
| **`tf-formats` n'existe pas** | ni `.schem`, ni `.litematic`, ni `.nbt` |
| **L'arène GPU se reconstruit en ENTIER** | le remaillage est incrémental jusqu'aux arènes, qui se rebâtissent en O(quads de la scène). Mesuré sur 64 chunks : 3,5 ms sur 4,4 — sous le budget de 8 ms de la phase 5, donc pas encore le bon combat, mais c'est le prochain. Les `tranches` de l'arène sont déjà par section |
| **Rien ne PILOTE la fenêtre de résidence** | la zone est choisie à la main (`--zone`) ; `tf-world::residency` existe et est testé, et rien ne le nourrit depuis la caméra. C'est le cœur de la phase 5 |
| **Ni composants ni saisie chiffrée** | le pousser-tirer est là ; taper « 12 » pendant le geste, et les composants qu'on modifie une fois pour les mettre à jour partout, restent à écrire (phase 7) |
| **La coque n'ouvre pas de save par un menu** | le monde arrive par la ligne de commande (`--monde`, `--zone`), ou c'est la fixture de BUILD |
| **Blocs hors pack** | 0,9 % sur Mosslorn (`reinforced_deepslate`, `mud`, `sculk`…) : un codex d'époque 1.18 ne connaît pas le 1.20. C'est pourquoi lire l'installation de l'utilisateur vaut mieux qu'un catalogue préparé |

Et une limite de méthode : **tous les temps de rendu sont mesurés sur un
rastériseur logiciel** et ne valent rien. Les comptes — quads, poses, appels de
dessin, octets — sont transposables ; les millisecondes, non.

### Le remaillage, et ce qu'il a appris

```bash
TF_PACK=<pack> cargo test -p tf-app --test chantier mesurer -- --nocapture
TF_PHASES=1 TF_PACK=<pack> cargo test -p tf-app --test chantier mesurer_les_deux -- --nocapture
```

| | avant | après | |
|---|---:|---:|---|
| Décoder UNE section d'une région bâtie | 17,5 ms | **0,2 ms** | × 87 |
| Remaillage après une opération de 3 blocs (zone de 64 chunks) | 30,7 ms | **4,4 ms** | × 7 |

**Le gain n'est pas venu d'où je l'avais mis.** J'ai écrit le remaillage
incrémental — ne remailler que les sections touchées plus une case de
débordement — et mesuré **× 1,1**. La découpe par phases a dit pourquoi :
18 ms sur 18,3 partaient dans la RELECTURE, 0,2 dans le maillage que je venais
d'optimiser. C'est le piège n° 1 du dépôt, à la lettre.

La relecture coûtait cher pour deux raisons, et toutes deux pèsent aussi sur le
CHARGEMENT :

- **`sections_de` inflatait chaque chunk AVANT de filtrer.** Une région bâtie
  porte 256 chunks : lire une section en décompressait 256. L'index donne les
  coordonnées sans rien décompresser.
- **La HAUTEUR de la sélection était ignorée.** Lire une section décodait
  toutes celles de son chunk, biomes compris — vingt-quatre pour une sur un
  monde 1.18.

Après quoi le poids s'est déplacé une fois de plus, et c'est écrit dans les
trous : les arènes GPU se reconstruisent en entier, 3,5 ms sur 4,4. Sous le
budget de 8 ms de la phase 5, donc pas encore le bon combat.

**Et une mesure qui a servi à NE PAS écrire du code.** Un état que l'atlas ne
connaît pas force un rechargement complet — j'allais rendre l'atlas
extensible. Mesuré d'abord : le pack coûte 540 ms *une fois*, et le
rechargement d'une zone de 64 chunks **31 ms**, une fois par type de bloc et
par séance. Ça ne vaut pas un atlas incrémental aujourd'hui ; ça le vaudra
quand la résidence sera pilotée par la caméra, parce qu'alors de nouveaux états
arriveront en volant. La mesure dit quand, pas seulement combien.

```bash
TF_PACK=<pack> cargo test -p tf-app --test chantier mesurer_le_rechargement -- --nocapture
```

---

## 8. Rejouer

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo bench  -p tf-ops -p tf-mesh -p tf-bench

# avec le pack réel du serveur (rien n'est copié dans le dépôt)
TF_PACK=../titisite/public/codex cargo test -p tf-assets --test codex_reel -- --nocapture
cargo run --release -p tf-blocks --example deriver -- ../titisite/public/codex

# avec une VRAIE save — ni l'un ni l'autre n'écrit dans la save
cargo run --release -p tf-ops    --example verite_terrain  -- D:\monde minecraft:dirt
cargo run --release -p tf-assets --example recenser_monde  -- D:\monde %APPDATA%\.minefield_1_18
cargo run --release -p tf-render --example capture         -- %APPDATA%\.minefield_1_18 vue.png 1600 --monde D:\monde --zone "4,7,10,12"

# copier, tourner, coller — sans --ecrire, la save n'est pas touchée
cargo run --release -p tf-ops --example editer -- D:\monde --sel "0,60,0,31,90,31" --copier-vers "64,0,64" --tourner 90 --pack %APPDATA%\.minefield_1_18
cargo run --release -p tf-ops --example editer -- D:\monde --sel "0,60,0,31,90,31" --deplacer "64,0,0"
cargo run --release -p tf-ops --example editer -- D:\monde --sel "0,60,0,31,90,31" --empiler 5 est
```

Les outils acceptent trois formes d'assets, reconnues au CONTENU : un codex
extrait, un pack (dossier ou `.zip`), ou une **installation de launcher**
(`versions/` + `resourcepacks/` + `server-resource-packs/`). Le critère de
détection d'une installation est la présence de `versions/`, jamais le nom —
un serveur a son propre launcher.

**Une commande par ligne** : PowerShell ne connaît pas la continuation `\` d'un
shell Unix, et il faut les guillemets autour de `--zone` et `--sel`.
