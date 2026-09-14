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

## Ce qui reste, et pourquoi

Les 471 manques signalés se lisent en deux tas très différents :

| | | |
|---|---:|---|
| `non décomposable` | 393 | **la règle exacte est juste** ; seule la forme compacte est appauvrie |
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
