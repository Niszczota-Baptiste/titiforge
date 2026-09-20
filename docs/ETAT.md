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
| Tests | **420**, zéro échec |
| `cargo clippy --all-targets` | propre |
| Crates finis | tf-nbt · tf-anvil · tf-world · tf-blocks · tf-ops · tf-mesh · tf-assets · tf-render |
| Crates non commencés | **tf-formats** (schematics) · **tf-app** (la coque) |
| Vraies saves vérifiées | 2 — Minefield 1.18, vanilla 1.20.1 |
| Blocs réellement réécrits puis annulés au bit près | **606 M** |
| Appels de dessin, quelle que soit la scène | **2** |

---

## 1. Les tests

```bash
cargo test --workspace
```

420 tests, répartis par ce qu'ils PROUVENT :

| Famille | Tests | Ce qu'elle tient |
|---|---:|---|
| `tf-nbt` reader/writer/robustesse | 42 | longueurs signées, profondeur, charges forgées |
| `tf-anvil` region/section/lossless/versions/external | 89 | round-trip octet pour octet, 1.13→1.21, `.mcc` |
| `tf-anvil` croisement | 2 | un `.mca` écrit par un **producteur tiers** (le moteur JS) |
| `tf-world` journal/staging/residency/coords/source/lecture | 112 | annuler ↔ refaire sur le CONTENU, division plancher, emprise bornée |
| `tf-blocks` regles | 21 | lois du groupe, et le contrôle de FORME indépendant |
| `tf-ops` etages/edition/tirage + 3 unitaires | 33 | les trois étages, la jonction, le hachage par plan |
| `tf-assets` pack/textures/rotation/jeu/codex_reel | 65 | parents, uv, atlas, `.jar`, détection d'installation |
| `tf-mesh` mailler/chantier | 27 | glouton contre naïf, case par case |
| `tf-render` rendu | 17 | **au pixel** : ombrage, teinte, dalle, alignement WGSL |
| `tf-bench` fixture/build | 12 | l'échantillon reste représentatif du pack |

Deux propriétés valent d'être nommées :

- **Les tests de rendu comparent des PIXELS.** Un ombrage inversé a vécu des
  années dans `we-engine` parce qu'il était invisible sur un build gris.
- **Certains tests ne tournent que si on leur donne la donnée** — le pack réel
  (`TF_PACK=…`) et le `.mca` du moteur tiers (`TF_MCA_TIERS=…`). Un test qu'on
  ne peut pas jouer sans une donnée privée ne doit pas casser la suite de
  quelqu'un qui ne l'a pas, mais il doit EXISTER.

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

## 7. Ce qui n'est pas fait, et ce qui n'est pas mesuré

Chiffré quand c'est possible — un trou nommé vaut mieux qu'un trou tu.

| Manque | Ce que ça coûte aujourd'hui |
|---|---|
| **Les fluides ne sont pas dessinés** | 4,5 % des blocs posés de Mosslorn, dont 6,2 M d'eau. Les fluides n'ont pas de modèle de bloc dans le format : il leur faut leur propre passe |
| **Les biomes ne sont pas décodés** | la couleur de teinte est un RÉGLAGE « plaines », pas une mesure. La vraie vit dans le chunk, par section |
| **`uvlock` non appliqué** | une dalle tournée montre la bonne portion de texture, pas forcément dans le bon sens |
| **Pas d'occlusion ambiante, pas de LOD** | le rendu est plat, et tout ce qui est résident est dessiné |
| **Pas de rendu indirect ni de HZB** | 2 appels de dessin suffisent aujourd'hui ; ils ne suffiront plus avec un remaillage partiel |
| **La fenêtre de résidence n'est pas branchée au rendu** | `tf-world::residency` existe et est testé ; rien ne le pilote encore depuis une caméra |
| **`tf-formats` n'existe pas** | ni `.schem`, ni `.litematic`, ni `.nbt` |
| **`tf-app` n'existe pas** | pas de fenêtre, pas d'interface. Tout passe par des exemples en ligne de commande |
| **Blocs hors pack** | 0,9 % sur Mosslorn (`reinforced_deepslate`, `mud`, `sculk`…) : un codex d'époque 1.18 ne connaît pas le 1.20. C'est pourquoi lire l'installation de l'utilisateur vaut mieux qu'un catalogue préparé |

Et une limite de méthode : **tous les temps de rendu sont mesurés sur un
rastériseur logiciel** et ne valent rien. Les comptes — quads, poses, appels de
dessin, octets — sont transposables ; les millisecondes, non.

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
```

Les outils acceptent trois formes d'assets, reconnues au CONTENU : un codex
extrait, un pack (dossier ou `.zip`), ou une **installation de launcher**
(`versions/` + `resourcepacks/` + `server-resource-packs/`). Le critère de
détection d'une installation est la présence de `versions/`, jamais le nom —
un serveur a son propre launcher.

**Une commande par ligne** : PowerShell ne connaît pas la continuation `\` d'un
shell Unix, et il faut les guillemets autour de `--zone` et `--sel`.
