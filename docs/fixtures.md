# Les fixtures de mesure

Aucun fichier binaire dans le dépôt. Tout ce que les tests et les benchs
consomment se construit **à la volée depuis une graine** : identique d'une
exécution à l'autre, lisible en revue, et modifiable sans devoir régénérer un
`.mca` que personne ne peut relire.

Il y en a **deux**, et les confondre fausserait tout.

## `Terrain` — du sous-sol, pour mesurer Anvil

`crates/tf-bench/src/lib.rs`. Palettes d'une vingtaine d'entrées courtes,
beaucoup de pierre, des sections entièrement pleines et d'autres entièrement
vides. C'est le profil qui décide du coût de **lecture et d'écriture** d'un
fichier de région : décompression, balayage NBT, dépack, splice.

Vingt et une entrées de strates, et c'est un choix mesuré : ça demande
**5 bits** par indice, et 4 et 8 bits sont les seules largeurs où les deux
packings du format produisent les mêmes octets. Une fixture à 16 entrées ne
ferait jamais travailler le chemin intéressant.

## `Build` — un bâtiment, pour mesurer le rendu

`crates/tf-bench/src/build.rs`. Des salles, des murs, des piliers, des
planchers, un toit, et du décor semé le long des murs.

**`Terrain` ment pour le maillage.** Il donne 100 % de cubes pleins là où la
cible en a un tiers, et 80 % de sections homogènes là où un build n'en a
aucune. Mesurer le rendu dessus mesurerait le mauvais chemin et dimensionnerait
la phase 2 sur un cas qui n'existe pas.

Relevé sur le `.mca` produit, par le décodeur (`cargo run --release -p tf-bench
--example profil_build`) :

| | région pleine |
|---|---:|
| Blocs | 100 663 296 |
| `.mca` | 24 Mo |
| Sections homogènes | **0** |
| Palette médiane d'une section | 49 entrées (6 bits) |
| Blocs posés | 31 587 601 — **31,4 %** du volume |
| dont cubes pleins | 28 718 221 (90,9 %) |
| dont blocs-modèles | 2 869 380 (9,1 %) |
| Cuboïdes pour la passe de modèles | **9 458 866** |

Le chiffre qui dimensionne la phase 2 est le dernier. En nombre de blocs posés
le décor est minoritaire ; en **géométrie à émettre** il pèse près de la moitié,
parce qu'un bloc-modèle vaut 3,58 cuboïdes en moyenne.

### La densité de décor est un réglage, pas une mesure

La proportion de blocs-modèles dans un bâtiment dépend de qui l'a construit. La
faire passer pour un fait mesuré serait exactement l'erreur que ce dépôt
s'interdit.

`densite_decor` est donc explicite, et les benchs la **balaient** :

| densité | blocs-modèles | cuboïdes |
|---:|---:|---:|
| 20 % | 4,0 % | 485 190 |
| 55 % (défaut) | 9,1 % | 1 163 647 |
| 90 % | 13,7 % | 1 851 345 |

*(sur `Build::petit()`, un quart de région)*

## Ce qu'une vraie save en dit

Cinq régions d'un monde Minefield réel (1.18, 6,2 Mo, 1 481 chunks), relevées
par `cargo run --release -p tf-assets --example recenser_monde -- <monde>
<codex>`. Rien n'en est copié ici : l'outil lit la save de l'utilisateur et rend
des chiffres.

| | fixture `Build` | zone BÂTIE réelle | monde entier |
|---|---:|---:|---:|
| Blocs posés | 31,4 % du volume | 1,3 % | 0,8 % |
| Sections homogènes | 0 | 93,8 % | 96,6 % |
| Palette médiane d'une section | 49 (6 bits) | 1 (4 bits) | 1 (4 bits) |
| Blocs-modèles, part des posés | 9,1 % | **7,6 %** | 1,6 % |
| Cuboïdes par bloc-modèle | 3,58 | **1,54** | 2,16 |

Deux colonnes parce qu'une seule mentirait. Le monde est un PLOT créatif : un
sol plat de quatre couches (socle, deux de terre, une d'herbe — 1 024 blocs par
chunk, et c'est exactement la médiane), avec des constructions posées dessus sur
un chunk sur dix. Sa moyenne est un chiffre vrai qui ne décrit aucun chunk
existant, et surtout pas ceux qui coûtent quelque chose. La colonne « zone
bâtie » est le rectangle de chunks le plus chargé.

### Ce que ça confirme, et ce que ça corrige

**La densité de décor était une bonne estimation.** 9,1 % réglés contre 7,6 %
relevés sur ce qui est bâti : le réglage par défaut vise juste.

**Le nombre de cuboïdes par bloc-modèle ne l'était pas** — 3,58 contre 1,54,
soit un bench **2,3 × trop pessimiste** sur le seul chiffre qui dimensionne la
passe de modèles. Et la cause est précise : l'échantillon est stratifié sur le
catalogue, donc il reflète ce qu'un pack CONTIENT. Un constructeur, lui, pose
des dalles, des escaliers et des chemins de terre — les modèles bon marché — et
n'a jamais posé un `red_pumpkin_treat_bag` à 82 cuboïdes. **La distribution de
ce qui existe n'est pas celle de ce qu'on pose.** C'est la même erreur que
celle déjà notée un cran plus haut (« un premier tirage forçait les plus lourds
en tête »), qui avait survécu au niveau suivant.

Elle n'est pas corrigée dans la fixture, et c'est délibéré : remplacer une
estimation documentée par une mesure sous-échantillonnée — 42 chunks d'un seul
monde — échangerait une erreur connue contre une erreur cachée. Ce qui la
trancherait est un export de build DENSE, que `recenser_monde --zone` saura
recenser tel quel.

### Ce que ça a trouvé

Le relevé de ce qui pèse dans la passe de modèles mettait
`minecraft:mushroom_stem` en tête avec **38 %** du total. Un bloc plein du jeu,
en tête d'un classement de blocs-modèles : c'est ce qui a sorti une rotation de
variante jamais appliquée, et donc tous les escaliers d'un build dessinés dans
la même direction (`tf-assets/rotation.rs`).

C'est l'argument de cette page : une fixture ne peut pas trouver ça, parce
qu'elle est faite de ce qu'on savait déjà.

## Le catalogue

`crates/tf-bench/src/catalogue.rs` est **engendré**, pas écrit à la main :
220 blocs `minefield:*` réels, tirés des 1 678 du serveur, avec leur forme et
leur nombre de cuboïdes.

Ce ne sont que des **noms et des formes**. Ni textures ni modèles ne sont
recopiés — c'est ce qui permet de construire un build réaliste sans
redistribuer quoi que ce soit.

L'échantillon est stratifié sur le **nombre de cuboïdes**, proportionnellement.
Un premier tirage forçait les plus lourds en tête : moyenne 7,58 contre 3,58
dans le codex, soit un bench deux fois plus pessimiste que la cible. *Une
fixture fausse dans le sens prudent reste fausse* — elle ferait rejeter une
optimisation qui suffisait. Un test refuse un écart de plus de 15 %.

Le pire cas du serveur — `minefield:red_pumpkin_treat_bag`, **82 cuboïdes** —
est à part (`PIRE_CAS`) : un sur 1 121, l'inclure tirerait la moyenne à 4,12 et
ferait porter le pire cas par tous les chiffres. Il mérite sa propre mesure.

### Régénérer

Le script lit `titisite/public/codex/`, qui n'est pas dans ce dépôt. Il croise
`blockstates.json` avec `models/` et `render-models/` :

1. pour chaque `minefield:*`, résoudre le premier modèle cité (variantes **et**
   multipart — sauter les multipart perdait 353 blocs sur 1 678, et faussait la
   proportion de modèles de dix points) ;
2. classer : un cuboïde qui va de `[0,0,0]` à `[16,16,16]` ⇒ `Cube`, sinon
   `Modele`, aucun élément ⇒ `Vide` ;
3. échantillonner par strate de nombre de cuboïdes, proportionnellement ;
4. réécrire le fichier en entier, en-tête de documentation compris.

Le tableau de l'en-tête est recalculé à chaque fois : il dit ce que
l'échantillon préserve, et il n'a pas à être maintenu à la main.
