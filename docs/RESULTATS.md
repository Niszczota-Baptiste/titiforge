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
