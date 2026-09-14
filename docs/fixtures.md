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

Personne ici n'a de vrai build Minefield à recenser : la proportion de
blocs-modèles dans un bâtiment dépend de qui l'a construit. La faire passer
pour un fait mesuré serait exactement l'erreur que ce dépôt s'interdit.

`densite_decor` est donc explicite, et les benchs la **balaient** :

| densité | blocs-modèles | cuboïdes |
|---:|---:|---:|
| 20 % | 4,0 % | 485 190 |
| 55 % (défaut) | 9,1 % | 1 163 647 |
| 90 % | 13,7 % | 1 851 345 |

*(sur `Build::petit()`, un quart de région)*

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
