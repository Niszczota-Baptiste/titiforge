# Prototype de performance — résultats mesurés

Objectif de ce prototype : **ne prouver que des chiffres**. Aucune architecture
n'est écrite tant que les trois cibles ne sont pas tenues. C'est le piège n° 1
du `CLAUDE.md` de we-engine — « optimiser sans mesurer » — appliqué à la
réécriture elle-même.

## Protocole

Les deux moteurs lisent **le même fichier, octet pour octet**. Il est fabriqué
par `bench/lib/fixtures.js` de we-engine (terrain stratifié avec veines, pas du
bruit), donc reproductible à partir de sa graine et absent du dépôt — la règle
« pas de fixture binaire » tient.

```
r.0.1.mca   7,83 Mio   1024 chunks × 24 sections = 24 576 sections
                       100 663 296 blocs — soit une région Anvil pleine hauteur
```

Machine : Intel Xeon 2,10 GHz, **4 cœurs**, 15 Go, Linux. C'est la même classe
de machine que `bench/baseline.json` (le décodage y mesure 961 ms, ici 1010 ms
sur la demi-hauteur — l'écart est dans le bruit habituel).

Les mesures Rust sont des **médianes de 5**. Une mesure unique est du bruit :
`mirror-rotate` a bougé de 18 % à code identique dans votre propre bench.

## Résultats

| Mesure | we-engine (JS) | prototype Rust | Gain |
|---|---:|---:|---:|
| Décodage d'une région pleine | 1 892 ms | **48 ms** | **× 39** |
| RSS du processus | 633 Mo | **54,6 Mo** | **× 11,6** |
| Empreinte par bloc | 6,59 o | **0,33 o** | **× 20** |
| `//replace` — par bloc, coords monde, 1 fil | 11 718 ms | 352 ms | × 33 |
| `//replace` — par bloc, par section, rayon | — | 26,8 ms | × 437 |
| `//replace` — **par palette** | — | **0,26 ms** | **× 45 000** |
| `//set` sur la région entière | non mesuré | **2,78 ms** | — |

Les trois cibles fixées à l'audit sont franchies :

| Cible | Visé | Obtenu |
|---|---|---|
| Décodage d'une région pleine | < 150 ms | **48 ms** |
| Empreinte résidente | < 2 o/bloc | **0,33 o/bloc** |
| `//replace` sur 100 M blocs | < 20 ms | **0,26 ms** |

## Contrôle de correction

Une optimisation non vérifiée est une corruption silencieuse — et sur ce
produit, la corruption tombe sur la sauvegarde d'un utilisateur. Les quatre
stratégies sont donc comparées sur le même monde :

```
terre avant l'opération                 2 608 672
terre après (b) par bloc               17 826 485
terre après (c) palette dédoublonnée   17 826 485
terre après (f) palette sans dédoubl.  17 826 485
pierre restante après (f)                       0
```

`2 608 672 + 15 217 813 = 17 826 485`. Le moteur JS rapporte lui aussi
**15 217 813** blocs changés : quatre implémentations indépendantes, un seul
résultat.

## Ce que la mesure a CONTREDIT

L'audit annonçait que l'étage palette serait rapide parce qu'il ne toucherait
aucun indice de bloc. **C'est faux en l'état.**

```
(c) par palette — cible PRÉSENTE    23,3 ms    0 sections rapides, 9 216 remappées
(d) par palette — cible ABSENTE      0,5 ms    9 216 sections rapides, 0 remappée
```

Sur du vrai terrain, une section qui contient de la pierre contient presque
toujours de la terre. Remplacer l'une par l'autre **fusionne deux entrées de
palette**, ce qui oblige à remapper les 4 096 indices. Le chemin rapide ne s'est
déclenché sur **aucune** section : l'étage palette retombait au niveau de
l'étage bloc (23,3 contre 26,8 ms).

### Le correctif, mesuré à son tour

Le format Anvil **n'interdit pas** deux entrées de palette identiques : le jeu
lit `palette[indice]` et obtient un état valide dans les deux cas. En laissant
le doublon, la longueur de la palette ne bouge pas, donc `bits` non plus, donc
aucun indice n'est touché.

```
(f) par palette SANS dédoublonnage   0,26 ms   9 216 sections
```

**110 × plus rapide que l'étage bloc**, pour un résultat identique au bloc près
(vérifié ci-dessus). Deux contreparties, assumées et à écrire dans les
invariants :

1. La palette porte une entrée redondante jusqu'au prochain compactage — à
   l'écriture, ou jamais : le jeu recompacte à sa propre sauvegarde.
2. **Tout ce qui cherche un état dans une palette doit chercher TOUTES les
   occurrences, jamais la première.** Un `position()` au lieu d'un filtre
   ferait rater la moitié d'un `//replace` suivant, sans la moindre erreur.
   C'est exactement la forme du piège « écrire un état par défaut dédouble la
   palette » déjà payé sur `grass_block[snowy=false]`.

## Pourquoi c'est si rapide

Rien d'exotique — trois décisions, chacune visible dans le code :

- **`tf-proto/src/nbt.rs`** : lecteur NBT *ciblé*, qui saute tout ce qui n'est
  pas `sections[].block_states`. Le bench de we-engine montrait le parseur NBT
  générique à 35 % du décodage une fois le dépack corrigé ; ici il n'y a pas
  d'arbre d'objets du tout.
- **`tf-proto/src/section.rs`** : la section reste **packée** — palette de `u32`
  internés et indices de 4 à 6 bits. 0,33 o/bloc contre 154 o/bloc pour un
  `{Name, Properties}` par case.
- **`rayon`** : la section est l'unité de travail et elles sont indépendantes.
  Votre note « un fil par chunk ne gagne rien » était exacte *en JS* — le coût
  était le `structuredClone` de l'arbre NBT, pas le calcul.

## Rejouer

```bash
# la région de référence (nécessite ExeWorldEdit cloné à côté, npm install fait)
node proto/fixtures/gen.mjs

# la référence JS
node proto/fixtures/bench-js-full.mjs

# le prototype
cd proto && cargo run --release -- ./r.0.1.mca
```

---

# Mesure continue — `criterion`

Les chiffres ci-dessus viennent du prototype, mesuré une fois. À partir d'ici,
c'est `cargo bench -p tf-bench` qui mesure, sur une fixture construite **en
Rust** — plus besoin de Node ni d'une save réelle.

```bash
cargo bench -p tf-bench                        # mesurer
cargo bench -p tf-bench -- --save-baseline v0  # figer la référence
cargo bench -p tf-bench -- --baseline v0       # comparer
```

Deux règles héritées du bench de `we-engine` : un écart ne compte **qu'au-delà
de 25 %** (mesuré là-bas : 18 % de dérive à code identique), et ce qui est
exploitable ici, ce sont les **rapports entre scénarios**, pas les valeurs
absolues — c'est un conteneur partagé.

Première exécution, région pleine (24 576 sections, 100 663 296 blocs),
**monofil** :

## Chargement

| Scénario | Temps | Part |
|---|---:|---:|
| `1_inflate_seul` | **66,6 ms** | **75 %** |
| `2_scan_seul` | 5,5 ms | 6 % |
| `3_region_complete` | 88,8 ms | 100 % |

## Opérations

| Scénario | Temps | Rapport |
|---|---:|---:|
| `replace_par_palette` | **349 µs** | — |
| `replace_par_bloc` | 33,1 ms | **× 95** |
| `set_uniforme` | 501 µs | — |
| `compact_palette` | 103,8 ms | — |

## Packing

| Scénario | Temps (4 096 indices) |
|---|---:|
| `unpack_sans_chevauchement` | 2,45 µs |
| `unpack_avec_chevauchement` | 3,95 µs |
| `pack_sans_chevauchement` | 6,81 µs |
| `pack_avec_chevauchement` | 4,61 µs |

## Écriture

| Scénario | Temps |
|---|---:|
| `region_intacte` (rien n'a changé) | 1,66 ms |
| `splice_et_recompression` (un chunk) | 1,10 ms |

## Empreinte

```
fichier                     8,01 Mio
sections                   24 576
blocs                  100 663 296
états distincts                22
structure packée            26,0 Mio   = 0,27 o/bloc
RSS du processus            39,2 Mio
(we-engine, région équivalente : 633 Mio, 6,59 o/bloc)
```

## Ce que la mesure désigne

**Le goulot a changé de place, et ce n'est plus notre code.** `inflate` pèse
désormais **75 %** du chargement — 66,6 ms sur 88,8. Notre balayage NBT ciblé,
lui, fait 5,5 ms, soit 6 %. Pour mémoire, dans `we-engine` le parseur NBT
pesait 35 % et l'inflate 9 % : la proportion s'est inversée parce que le
parseur a été supprimé, pas parce que la décompression a ralenti.

Deux pistes, dans cet ordre :

1. **Le backend de décompression.** `flate2` utilise `miniz_oxide`, en Rust
   pur. `zlib-ng` est couramment 2 à 3 fois plus rapide sur ce profil. Ça ne
   demande qu'un drapeau de fonctionnalité — mais ça se **mesure** avant de
   s'annoncer.
2. **Le parallélisme.** Ces chiffres sont monofil. Le prototype, avec `rayon`
   sur 4 cœurs, décodait la même région en 48 ms contre 88,8 ici — cohérent, et
   la décompression est parfaitement parallélisable par chunk.

`compact_palette` à 103 ms est cher, et c'est assumé : il ne tourne qu'à
l'écriture, jamais dans une boucle chaude. Si un profil le montrait ailleurs,
ce serait un bug d'appelant.

---

# Première optimisation : le backend de compression

La mesure désignait `inflate`, à 75 % du chargement. L'hypothèse évidente était
le backend : `flate2` utilise `miniz_oxide` en Rust pur, et « zlib-ng est 2 à
3 fois plus rapide » est de notoriété publique.

**L'hypothèse était fausse**, et la façon dont elle s'est effondrée mérite
d'être notée.

## Ce qui n'a pas marché : comparer deux exécutions

Basculer sur `zlib-rs` puis relancer le bench annonçait **−16 %** sur
`inflate_seul`. Sauf que `scan_seul`, qui ne décompresse **rien**, annonçait
−14 % dans la même exécution. C'était le bruit du conteneur, pas un gain — et
il était du même ordre que ce qu'on cherchait.

C'est la règle des 25 % qui a fait son travail. Sans elle, `zlib-rs` aurait été
adopté sur la foi d'un chiffre qui ne mesurait que la charge de la machine.

## Ce qui a marché : les deux backends dans le même processus

`benches/inflate.rs` fait tourner les deux décompresseurs sur les **mêmes
octets**, dans la **même exécution**, en alternance. Le bruit machine s'annule,
et ce qui reste est réel.

| Décompression, région pleine | Temps | Débit |
|---|---:|---:|
| `flate2` / `zlib-rs` | 63,20 ms | 495 Mio/s |
| `miniz_oxide` | 63,55 ms | 492 Mio/s |

**0,6 % d'écart. Le backend ne change rien à la décompression.**

## Où le gain était vraiment

La réputation de `zlib-ng` porte sur la **compression**, pas sur l'inverse. Or
écrire une région modifiée recompresse chaque chunk touché — et c'est trois
fois plus cher que de le lire.

| Compression (niveau 6), 64 chunks | Temps | Taille produite |
|---|---:|---:|
| `flate2` / `zlib-rs` | **21,7 ms** | **15,4 %** du clair |
| `miniz_oxide` | 59,3 ms | 16,2 % du clair |

**× 2,7 plus rapide, et 5 % plus petit.** Les deux à la fois, ce qui est rare :
un compresseur plus rapide produit d'habitude des fichiers plus gros, et sur
une save de plusieurs gigaoctets ça ne serait pas un gain.

Vérifié sur le chemin réel :

| Scénario | Avant | Après | |
|---|---:|---:|---:|
| `splice_et_recompression` | 1,10 ms | **392 µs** | **−64 %** |
| `region_intacte` | 1,66 ms | 1,91 ms | bruit — ce chemin ne compresse pas |

`zlib-rs` est donc adopté. Il est en Rust pur, donc la compilation croisée vers
Windows reste triviale — un backend en C aurait imposé une chaîne de
compilation à tous ceux qui construisent le projet.

## Ce qui reste sur le chargement

`inflate` pèse toujours 75 %, et aucun backend n'y changera rien. Le levier
restant est le **parallélisme** : ces chiffres sont monofil, et le prototype
décodait la même région en 48 ms contre 88,8 avec `rayon` sur 4 cœurs. La
décompression est parfaitement parallélisable par chunk.

`benches/inflate.rs` reste dans le dépôt : c'est la preuve, et elle évite qu'on
repropose l'échange dans six mois sur la foi de la même réputation.

---

# Deuxième optimisation : le parallélisme

Le backend de compression épuisé, il restait les 75 % d'`inflate`. Chaque chunk
est un flux zlib indépendant : le découpage par chunk est donc le bon grain.

| Région pleine, 24 576 sections | Temps | Gain |
|---|---:|---:|
| Monofil | 107,45 ms | — |
| `rayon`, 4 cœurs | **28,25 ms** | **× 3,80** |
| `rayon`, sans la fusion des palettes | 27,19 ms | — |

**95 % d'efficacité sur 4 cœurs.** Et la fusion des palettes — le coût qu'on
pourrait craindre — ne pèse que **1,06 ms**, soit 3,9 % du total.

C'est l'exact inverse de ce qu'a vécu `we-engine`, dont le `CLAUDE.md` note
qu'« un fil par chunk ne gagne rien » : là-bas, transférer l'arbre NBT entre
fils coûtait plus cher que de le décoder (65 ms contre 60 sur 128 chunks). La
cause était le `structuredClone` de JavaScript, pas le problème. En mémoire
partagée, le découpage se paie largement.

## Le point de conception que la mesure met au jour

L'interner est de l'**état mutable partagé**. Le mettre derrière un verrou
sérialiserait exactement ce qu'on essaie de paralléliser. Chaque fil interne
donc dans une table LOCALE, et la fusion se fait après — c'est ce coût de
1,06 ms, et c'est ce que `tf-world` devra reprendre en phase 1.

Le bench vérifie en plus que les deux chemins produisent le **même monde** :
même nombre de sections, mêmes indices octet pour octet, et les mêmes
identifiants internés dans le même ordre. Un décodage rapide qui rend autre
chose n'est pas un décodage rapide.

## Où en est le chargement

| | Temps | vs `we-engine` |
|---|---:|---:|
| `we-engine` (JS, monofil) | 1 892 ms | — |
| `tf-anvil` monofil | 107 ms | × 17,7 |
| `tf-anvil` + `rayon` (4 cœurs) | **28,2 ms** | **× 67** |

Sur une machine de développement à 8 ou 16 cœurs, l'efficacité mesurée laisse
attendre 15 ms ou moins — mais ça se mesurera là-bas, pas ici.

# Les règles de transformation, dérivées du pack

Une table de rotation écrite à la main est impossible ici : **910 blocs
Minefield portent un état**, avec quatre propriétés que le vanilla ne connaît
pas (`vertical`, 16 920 occurrences, puis `offset`, `model`, `position`). Elles
sont donc DÉRIVÉES des `blockstates.json`, sans jamais regarder le nom d'une
propriété.

Mesuré sur le pack du serveur (2 560 blockstates, 6 233 modèles), **813 ms** :

| | rotations | miroirs |
|---|---:|---:|
| `minefield:*` — 910 blocs à état | **99,3 %** | **97,0 %** |
| `minecraft:*` — 345 blocs à état | **99,4 %** | 99,7 % / 100 % |

**Zéro faute aux lois du groupe sur 33 844 états** : quatre rotations de 90°
rendent l'état de départ, deux miroirs aussi, et `rot90 ∘ rot90 = rot180`.
Ces lois ne sont pas vérifiées après coup, elles sont **construites** —
`rot180`/`rot270` sont `rot90` composée, `miroirZ` est `miroirX` suivi du
demi-tour.

## Le contrôle qui compte, et que les lois du groupe ne savent pas faire

Les lois du groupe disent qu'une règle est cohérente **avec elle-même**. Elles
ne disent pas qu'elle est juste : une règle qui tournerait tout d'un quart de
trop les passerait toutes. Le contrôle qui décide compare donc, pour chaque
couple (état, transformation), le **solide** de l'état d'arrivée à celui de la
source transformée — la seule question qui compte pour l'utilisateur.

Il a trouvé **3 431 couples faux sur 132 380** là où les lois du groupe
n'annonçaient aucune faute : tous les escaliers en coin et toutes les portes
ouvertes du pack rendaient leur version TOURNÉE au lieu de la réfléchie. La
cause : l'arithmétique d'angles suppose que la transformation se ramène à un
décalage de `y`, ce qui est vrai d'une rotation mais d'un miroir seulement si
le modèle est lui-même symétrique.

Aujourd'hui, sur le pack du serveur :

| | |
|---|---:|
| couples vérifiés | 132 380 |
| formes qui ne suivent pas exactement | 1 351 |
| **dont annoncées par la dérivation elle-même** | **1 351** |

Le contrôle indépendant ne trouve donc **rien que la table ne dise déjà**, et
l'assertion de débogage — « une forme approchée n'arrive jamais quand le pack
déclarait la bonne » — ne saute sur aucun des 1 254 blocs. Les 1 351 restants
sont deux limites du PACK, pas de la dérivation : une bougie n'a aucun champ
qui oriente, un bloc chiral sans jumeau déclaré n'a pas de réfléchi à rendre.
Dans les deux cas l'état rendu est le plus proche possible, et c'est DIT.

## Ce qui reste, et pourquoi

Les 746 manques signalés se lisent en trois tas très différents, et les
confondre donnerait une image fausse dans les deux sens :

| | | |
|---|---:|---|
| `non décomposable` | 395 | **la règle exacte est juste** ; seule la forme compacte est appauvrie |
| `forme approchée` | 273 | le pack ne déclare pas la forme transformée ; l'état rendu est le plus proche |
| `non représentable` | 78 | un vrai trou, refusé plutôt que deviné |

Les vrais trous sont deux familles, toutes deux des asymétries du pack :

- **Les bougies** (6 blocs) déclarent 144 états sur 4 géométries, et les
  classes ont des tailles inégales (48 contre 32) : `position=middle` n'a
  aucun homologue tourné. Aucune bijection n'existe.
- **`minecraft:snow`** et sa famille n'ont de formes qu'au nord et au sud : la
  rotation de 90° mènerait à un angle que rien ne déclare.

## Le point de conception que la mesure met au jour

La règle est une **permutation d'ÉTATS**, pas de propriétés — et c'est la
mesure qui l'a imposé. Sur les escaliers du serveur, `shape=outer` (un ajout
Minefield, une seule écriture par coin) exige `facing: east → south` sous
miroir est-ouest, pendant que l'escalier DROIT exige `east → west`. **Les deux
sont géométriquement justes**, et aucune permutation de `facing` ne fait les
deux : 393 blocs, toute la famille des escaliers.

La forme par propriété reste, mais comme **repli** pour un état que le pack ne
déclare pas — un monde plus récent que le pack en contient. Elle coûte une
indexation de tableau en moins à l'exécution, et elle dit ce qu'elle ne peut
pas porter au lieu de se taire.

# La répartition à trois étages, mesurée dans `tf-ops`

Le prototype avait prouvé les chiffres ; `tf-ops` les tient dans du code de
production, avec une API que le cœur peut compiler. Mesuré sur une **région
pleine** (100 663 296 blocs, 24 576 sections), monofil :

| Opération | Étages déclenchés | Temps |
|---|---|---:|
| cible absente de la palette | 24 576 `rien` | 2,1 ms |
| `//set` | 24 576 `section` | 2,2 ms |
| `//replace` | 9 216 `palette`, 15 360 `rien` | **1,35 ms** |
| `//replace`, sélection bordée d'un bloc | 6 324 `palette` + 2 892 `bloc` | 28,9 ms |
| mélange pondéré (dépend de la position) | 24 576 `bloc` | 1,23 s |

Les **comptes d'étages sont imprimés avec les temps**, et ce n'est pas
décoratif : le prototype a annoncé une fois un chemin rapide que la mesure a
démenti — « 0 sections rapides sur 9 216 ». Un bench qui ne compte pas les
étages ne prouve pas que la répartition se déclenche.

## Trois chiffres qui changent la conception

**Compter coûte 31 × l'opération.** À l'étage palette, compter les blocs
modifiés est exactement le parcours qu'on vient d'éviter : 1,35 ms sans,
50,1 ms avec. Le comptage exact est donc une OPTION (`Plan::en_comptant`), pas
un service rendu d'office. Une interface qui affiche « 12 345 blocs » le paie
sciemment ; un script qui enchaîne vingt opérations ne le paie pas.

**Une sélection bordée d'un bloc coûte × 21.** Décaler la sélection d'un seul
bloc fait tomber 2 892 sections à l'étage bloc, et l'opération passe de 1,35 ms
à 28,9. Or une sélection d'utilisateur ne s'aligne presque jamais sur 16. C'est
le chiffre qui dit où ira la prochaine optimisation : une section partiellement
couverte dont la partie couverte est uniforme peut encore éviter le parcours.

**Le tirage par bloc était dominé par sa propre plomberie.** 18,6 ns par bloc au
départ, contre 2,4 ns pour la même boucle sans tirage. Deux causes, mesurées
séparément :

| | Temps sur la région |
|---|---:|
| version d'origine | 1,877 s |
| tirage compilé hors de la boucle (plus de somme ni de `%` par bloc) | 1,516 s |
| une seule avalanche au lieu de trois | **1,228 s** |

Soit **−35 %**. La somme des poids se recalculait à chaque bloc et le modulo
était une division 64 bits, qui ne se pipeline pas ; le `(h × total) >> 64` de
Lemire la remplace au prix d'un biais de 10⁻¹⁸. Et hacher chaque axe avec une
avalanche complète coûtait trois fois ce qu'il fallait.

**La formule de tirage est FIGÉE.** Un build fait avec une graine doit se
rejouer à l'identique : changer la façon de tirer changerait tous les mondes
déjà construits, sans que rien ne le signale.

## De l'opération au fichier

`tf-ops/src/edition.rs` est la jonction : lecture par le staging, application du
plan section par section, recollement par plages d'octets, correctif de journal,
écriture dans la copie de travail. C'est la première fois que la pile entière
travaille ensemble, et ce que ça vérifie ne se vérifie nulle part ailleurs :

- la **source** reste octet pour octet ce qu'elle était ;
- un chunk hors sélection n'est ni décompressé ni réécrit ;
- un chunk modifié n'est **pas ré-encodé** — seules les plages d'octets des
  sections touchées sont remplacées, donc heightmaps, structures et données de
  mods ne peuvent pas être abîmées ;
- **annuler rend le monde d'avant et refaire celui d'après**, vérifié sur le
  contenu et pas sur un compte ;
- une opération qui ne change rien ne salit pas la copie de travail et ne
  remplit pas le journal d'entrées vides.

Une opération qui déborde sur plusieurs régions fait **UNE** entrée de journal :
un `Ctrl+Z` qui ne défait qu'un tiers du travail serait pire qu'une annulation
absente.

## Où part vraiment le temps d'une opération

Le piège n° 1 du dépôt a failli se refermer une deuxième fois. J'allais
paralléliser le CALCUL — les trois étages, 1,35 ms sur une région pleine.
Découpée phase par phase, la chaîne complète dit autre chose :

| Phase | Temps | Part |
|---|---:|---:|
| **recompression** | **541 ms** | **68 %** |
| décompression | 89 ms | 11 % |
| application + encodage des sections | 65 ms | 8 % |
| balayage + décodage | 37 ms | 5 % |
| recollement | 5 ms | 0,6 % |
| **chaîne complète** | **800 ms** | |

Le calcul qu'on avait soigneusement optimisé pèse **0,17 %**.

### Ce que ça a donné

La chaîne par chunk est parallélisable de bout en bout — et ce n'est pas un
heureux hasard : le tirage se hache sur la POSITION précisément pour qu'aucune
opération ne dépende de l'ordre de parcours. Chaque fil part d'une copie de
l'interner, parce que le partager derrière un verrou sérialiserait exactement
ce qu'on parallélise.

| | Temps | |
|---|---:|---|
| départ | 800 ms | |
| parallèle, 4 cœurs | **219 ms** | × 3,65 — **91 % d'efficacité** |
| + niveau de compression 2 | **138 ms** | **× 5,8 au total** |

Le niveau de compression se choisit sur mesure, pas par défaut : 550 ms au
niveau 6 contre 230 au niveau 2, pour 11,8 % de taille en plus. Le niveau 9
coûte **huit fois** le niveau 6 pour 6 % de gain — jamais. Le raisonnement est
celui du cache d'aperçu d'`ExeWorldEdit` : la copie de travail se réécrit à
chaque opération pendant que l'utilisateur attend, la sauvegarde finale ne
s'écrit qu'une fois.

### Et la propriété qui rend tout ça acceptable

Les chemins parallèle et séquentiel produisent le **même octet** : même
fichier, même ordre de correctifs de journal, mêmes comptes. Vérifié en
croisant les deux compilations (`--example empreinte`, avec et sans la
fonctionnalité). Un résultat qui dépendrait du nombre de cœurs donnerait deux
mondes différents de la même opération, et un journal qui ne défait pas ce
qu'il croit défaire.

## Sur de vrais fichiers

`--example editer` fait tourner la chaîne complète sur une save sur disque.
Mesuré sur un monde d'essai de quatre régions (256 chunks peuplés par région),
sélection de 100 925 440 blocs :

| | |
|---|---:|
| `//replace` (essai à blanc, sans compter) | **40 ms** |
| le même, en comptant | 60 ms |
| une cible ABSENTE de la save | **12 ms** — 3 072 sections écartées sur leur seule palette |

Et l'aller-retour se vérifie sur le disque : après avoir remplacé la pierre par
de la terre, il ne reste **aucune** pierre, et la terre compte 4 981 854 blocs —
les 4 955 054 remplacés **plus les 26 800 qui existaient déjà**. La sauvegarde
préalable est octet pour octet le fichier d'origine.

## Une vraie save, et un vrai pack

Cinq régions d'un monde Minefield 1.18 (6,2 Mo, 1 481 chunks), et le codex du
serveur. Jusqu'ici tout venait de fixtures qu'on avait écrites soi-même : le
décodeur et l'encodeur ne s'étaient jamais mesurés qu'à eux-mêmes.

### Les deux invariants porteurs, sur des fichiers que Minecraft a écrits

`--example verite_terrain`. On n'exige pas la même empreinte, on exige les
mêmes **octets** — une empreinte qui collerait sur des octets différents serait
une collision.

| | |
|---|---:|
| `//replace` (étage palette) | **71 ms**, 1 141 chunks, 582 861 blocs |
| étages | rien 34 403 · palette 1 141 · **bloc 0** |
| chunks refaits puis annulés, octet pour octet | 1 141 |
| chunks NON modifiés, vérifiés au ZLIB près | 288 |
| un mélange (étage bloc) | **1 121 ms** pour **145 296 392 blocs** |
| chunks refaits puis annulés, octet pour octet | 1 481 |
| contrôles, fautes | 15 034 · **0** |

L'étage bloc réécrit chaque bloc du monde en 1,12 s — 130 millions de blocs par
seconde, recompression comprise — et l'aller-retour reste exact au bit près.

Corroboration croisée : le recensement compte 582 861 blocs de terre en
dépaquetant chaque indice ; l'opération en compte 582 861 par sa palette, sans
en dépaquetter un seul. Deux chemins sans code commun, le même nombre.

### Ce que le monde contient VRAIMENT

`--example recenser_monde`. Le détail et ce qu'il corrige aux fixtures sont
dans `docs/fixtures.md` ; l'essentiel :

| | fixture `Build` | zone BÂTIE réelle |
|---|---:|---:|
| Blocs-modèles, part des posés | 9,1 % | **7,6 %** |
| Cuboïdes par bloc-modèle | 3,58 | **1,54** |

La densité de décor était une bonne estimation ; le nombre de cuboïdes par
bloc-modèle était **2,3 × trop pessimiste**, parce que l'échantillon est
stratifié sur ce qu'un pack CONTIENT alors qu'un constructeur pose des dalles
et des escaliers, jamais le sac de friandises à 82 cuboïdes.

Et c'est ce relevé qui a sorti une rotation de variante jamais appliquée :
`minecraft:mushroom_stem`, un bloc PLEIN du jeu, arrivait en tête d'un
classement de blocs-MODÈLES avec 38 % du total — ses six parts se superposaient
en un seul plan.

## Ce que la passe de modèles coûte

`--example mesurer`, sur `Build::petit()` (un quart de région) avec la
géométrie RÉELLE du pack. La dernière colonne est la conception qu'on compare,
pas une optimisation qu'on espère : ce que les mêmes faces pèseraient si on les
émettait en quads.

| décor | quads | arène | poses | faces | modèles | en quads | gain |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 % | 516 893 | 16,54 Mo | 125 697 | 2 202 918 | 2,69 Mo | 70,49 Mo | **26,2 ×** |
| 55 % | 516 893 | 16,54 Mo | 300 942 | 5 276 449 | 5,50 Mo | 168,85 Mo | **30,7 ×** |
| 90 % | 516 893 | 16,54 Mo | 478 288 | 8 390 074 | 8,34 Mo | 268,48 Mo | **32,2 ×** |

Maillage 77 à 89 ms, construction des deux arènes 23 à 28 ms, et la table de
géométrie pèse **638 ko** — 570 états de modèle, quel que soit le nombre de
blocs qui les portent. C'est tout l'argument : deux dalles de chêne côte à côte
n'ont pas deux modèles, elles ont deux positions.

**Et le tout en DEUX appels de dessin**, quel que soit le nombre de faces par
bloc. Chaque pose a un nombre de faces différent — six pour une dalle, jusqu'à
492 pour le pire bloc du serveur — donc « n faces par instance » n'existe pas.
Aplatir côté processeur annulerait le gain ci-dessus ; un appel par nombre de
faces distinct en ferait une quinzaine. La pose porte le RANG de sa première
face dans le flot global, et le sommet retrouve la sienne par dichotomie :
vingt itérations sur un million de poses, zéro octet par face.

### À l'échelle d'une région PLEINE

Le même essai sur `Build::default()` — 1 024 chunks, 100 663 296 blocs — parce
qu'un quart de région ne prouve rien sur les limites d'un pilote.

| décor | quads | arène | poses | faces | modèles | en quads | gain |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 % | 4 141 777 | 132,54 Mo | 1 001 000 | 20 381 981 | 17,26 Mo | 652,22 Mo | **37,8 ×** |
| 55 % | 4 141 777 | 132,54 Mo | 2 385 953 | 48 520 638 | 39,42 Mo | **1 552,66 Mo** | **39,4 ×** |

Maillage 556 à 601 ms, arènes 242 à 270 ms, montée GPU 101 à 177 ms, et
**deux appels de dessin** pour 52 662 415 instances.

Le chiffre qui tranche est 1 552 Mo. En quads, une région bâtie ne tient tout
simplement pas : ce n'est pas « plus lent », c'est impossible. En poses elle
fait 39 Mo, et la table de géométrie 868 ko.

**Ce que la mesure désigne pour la suite**, et ce n'est pas ce qu'on aurait
parié : la passe gloutonne pèse maintenant **132 Mo**, trois fois la passe de
modèles. `InstanceQuad` fait 32 octets et porte une position en flottants
MONDE, alors qu'un quad tient dans sa section — le même raisonnement que la
pose. C'est là qu'ira la prochaine optimisation de mémoire, pas ailleurs.

### Une garde qui manquait

Un tableau de textures est plafonné à **2 048 couches**, et le pack du serveur
en cite 2 207. Monter tout le catalogue lève une erreur de VALIDATION wgpu qui
parle de `depth_or_array_layers` : exacte, et illisible pour qui vient
d'ouvrir une save. `AtlasGpu` vérifie le plafond et dit quoi faire — ne monter
que les textures des blocs PRÉSENTS.
