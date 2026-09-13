# titiforge

Éditeur de mondes Minecraft Java conçu pour des sélections de **plusieurs
milliards de blocs**. Cœur en Rust, rendu en wgpu, coque `winit + egui`.

Successeur de [`ExeWorldEdit`](https://github.com/Niszczota-Baptiste/ExeWorldEdit)
(Electron + JS), qui reste en service et sert de référence d'écriture pendant
toute la durée du MVP. Ce dépôt ne le modifie pas.

## Pourquoi une réécriture

Le moteur JS a été optimisé jusqu'à × 7 sur ses propres benchs, et il bute
malgré tout sur des murs **structurels**, pas sur un défaut d'optimisation :

| Mur | Mesuré dans `ExeWorldEdit` |
|---|---|
| Un bloc est un objet JS `{Name, Properties}` | 154 o/bloc → 15 Go pour pivoter 100 M blocs |
| L'aperçu est une liste de blocs matérialisée | plafond dur à 4 M blocs affichables |
| Moteur monofil, chaque case visitée | 11,3 M blocs/s |
| Arbre NBT générique par chunk | 35 % du temps de décodage |
| Un appel de dessin par chunk | 1 281 appels mesurés sur un build réel |

Détail complet et raisonnement : `docs/AUDIT.md`.

## Ce qui est déjà prouvé

Un prototype de ~650 lignes (`proto/`) mesure les trois cibles sur **le même
fichier et la même machine** que le moteur actuel — une région Anvil pleine
hauteur de **100 663 296 blocs**.

| Mesure | we-engine (JS) | prototype Rust | Gain |
|---|---:|---:|---:|
| Décodage d'une région pleine | 1 892 ms | **48 ms** | **× 39** |
| RSS du processus | 633 Mo | **54,6 Mo** | **× 11,6** |
| Empreinte par bloc | 6,59 o | **0,33 o** | **× 20** |
| `//replace` sur toute la région | 11 718 ms | **0,26 ms** | **× 45 000** |
| `//set` sur toute la région | non mesuré | **2,78 ms** | — |

Vérifié bloc à bloc : quatre implémentations indépendantes rapportent le même
résultat (`docs/RESULTATS.md`).

## Les trois étages d'une opération

C'est l'idée centrale, et ni WorldEdit ni `we-engine` ne l'exploitent. Une
section Anvil porte **une palette et 4 096 indices** ; presque toutes les
opérations WorldEdit s'expriment sur la palette seule.

| Étage | Quand | Coût |
|---|---|---|
| **Palette** | section entièrement couverte, correspondance valeur → valeur | O(palette) — ~40 entrées |
| **Section** | section entièrement couverte, résultat uniforme | O(1) |
| **Bloc** | bordure de sélection, ou opération qui lit vraiment chaque case | O(blocs), en parallèle |

Le chemin rapide n'est pas « itérer plus vite », c'est **ne pas itérer**.

## État — phase 0

`tf-nbt` et `tf-anvil` sont écrits et testés : **109 tests**, lecture et
écriture, round-trip lossless, formats 1.13 → 1.21 et chunks déportés `.mcc`.

Le round-trip ne repose pas sur une promesse mais sur une propriété
structurelle. Un chunk non modifié garde sa charge compressée brute, donc il
ressort octet pour octet. Un chunk **modifié** n'est pas ré-encodé non plus :
on note la plage d'octets de chaque `block_states` et on remplace uniquement
celles qu'on a touchées. Heightmaps, structures, `neoforge:attachments` — le
lecteur n'y touche pas, donc il ne peut pas les abîmer.

Les tests bouclent la chaîne entre deux implémentations **indépendantes** :
un producteur qui écrit ses octets à la main depuis la spec, et un décodeur
gelé qui reconstruit un arbre NBT complet. Ni l'un ni l'autre ne partage une
ligne avec `src/` — sinon le test dirait seulement « le code est d'accord avec
lui-même ». Un troisième croisement vise un `.mca` produit par le moteur JS
d'`ExeWorldEdit`, donc par un autre langage et une autre bibliothèque NBT :
sur la région de 100 millions de blocs, les deux moteurs comptent
**15 217 813** blocs remplacés, au bloc près.

Les formats anciens sont détectés sur la **structure**, jamais sur le
`DataVersion` : une save réelle mélange des chunks de plusieurs versions, parce
que le jeu ne réécrit un chunk que lorsqu'un joueur le visite. Le packing
1.13–1.15 (indices à cheval sur deux longs) se distingue du 1.16+ par la
**longueur** du tableau — les deux ne coïncident qu'aux largeurs où elles
produisent les mêmes octets.

Reste en phase 0 : le harnais `criterion`, pour que les chiffres viennent
d'une mesure continue et non du seul prototype.

Feuille de route complète : `docs/ROADMAP.md`.

## Rejouer les mesures

```bash
cargo test            # les 109 tests
cargo clippy --all-targets

# ExeWorldEdit doit être cloné à côté, avec npm install fait
node proto/fixtures/gen.mjs             # fabrique la région de référence
node proto/fixtures/bench-js-full.mjs   # la référence JS
cargo run --release -p tf-proto -- ./r.0.1.mca

# et le croisement avec ce producteur tiers
TF_MCA_TIERS=./r.0.1.mca cargo test -p tf-anvil --test croisement -- --nocapture
```

Aucune fixture binaire n'est commitée : la région se reconstruit depuis sa
graine.
