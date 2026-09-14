# CLAUDE.md — contexte pour Claude Code

Lu automatiquement au démarrage. À garder court : ce qui change à chaque commit
appartient au code ou à `docs/`.

## Ce qu'est ce dépôt

**titiforge** — éditeur de mondes Minecraft Java pour des sélections de
plusieurs **milliards** de blocs. Cœur Rust, rendu wgpu, coque `winit + egui`.

Successeur d'`ExeWorldEdit` (Electron + JS), qui **reste en service** et n'est
pas modifié par ce dépôt. Il sert de référence d'écriture : sa table d'états de
blocs, son packing Litematica et ses 44 pièges sont du savoir payé cher.

Cible : **le serveur Minefield d'abord**, Minecraft vanilla ensuite. Ce n'est
pas « du vanilla plus quelques blocs custom » — c'est l'inverse, et les chiffres
le disent (mesurés sur le codex de `titisite`) :

| | |
|---|---|
| Blocs `minefield:*` | **1 678** — contre 882 vanilla, presque le double |
| Formes non-cubes | **56 %** (950 blocs) ; cubes pleins : 29 % seulement |
| Blocs portant un état | **54 %** (910), à transformer sous rotation |
| Variantes avec rotation | **22 627** déclarées dans `blockstates.json` |
| Le plus complexe | une tombe en bois : **82 cuboïdes** dans un seul bloc |

Trois conséquences qui ne sont pas négociables :

1. **Le greedy meshing ne couvre que 29 % d'un build Minefield.** La passe de
   modèles n'est pas un repli pour quelques escaliers : c'est le chemin
   PRINCIPAL sur la cible principale. La phase 2 doit se dimensionner là-dessus,
   pas sur du terrain vanilla.
2. **Une table de rotation écrite à la main est impossible.** 910 blocs à état,
   avec quatre propriétés que le vanilla n'a pas — `vertical` (16 920
   occurrences), `offset`, `model`, `position`. Les règles DÉRIVÉES des
   `blockstates` ne sont pas un confort, c'est la seule option praticable.
3. **Un `minefield:*` n'est JAMAIS remappé vanilla.** Sa géométrie et ses états
   se transforment ; son namespace, jamais. Invariant hérité d'`ExeWorldEdit`,
   et il devient central quand les deux tiers du catalogue sont custom.

Le codex extrait vit dans `titisite/public/codex/` : `codex.json` (2 034
entrées avec noms français et catégories), `blockstates.json` (2 560 entrées,
minefield ET vanilla), plus les modèles et textures. C'est la source de vérité
du catalogue — `ExeWorldEdit` le réextrayait par `npm run codex`.

## Où va ce projet

Pas « un MCEdit moderne » : les fondations d'un **écosystème de création de
contenu Minecraft**, dont la première mission — et le cœur permanent — est
d'être le meilleur éditeur de mondes possible. PNJ, quêtes, routes, outils
cinématiques, génération procédurale, rendu shaders viendront par **greffons**,
jamais dans le cœur.

Priorités, dans cet ordre et sans ambiguïté : **performances à grande échelle,
stabilité, maintenabilité, extensibilité.** Et la règle qui tranche tout le
reste : *aucune évolution future ne doit dégrader le cœur*.

Les six contraintes techniques que ça impose sont dans **`docs/VISION.md`**. Les
deux qui se cassent le plus facilement par inadvertance :

- **Le greffon DÉCRIT, le cœur exécute.** Une API qui laisserait un greffon
  appeler `set_block(x, y, z)` ferait retomber toute opération de greffon à
  l'étage bloc — × 95 perdus, mesuré. Masques, motifs et formes sont des
  DONNÉES que le cœur compile en plan d'exécution.
- **Le journal d'annulation n'est pas un journal de blocs.** Un PNJ posé par un
  greffon doit s'annuler avec le même Ctrl+Z qu'un `//set`. Le journal est une
  suite d'entrées réversibles TYPÉES ; l'instantané de section n'en est qu'une.

Ce qui se pose aujourd'hui, ce sont les **coutures** — registres plutôt
qu'`enum` figés, journal typé, format de projet versionné. Le runtime de
greffons ne se construit pas avant que le cœur soit fini.

### Portée de la 1.0, tranchée

- **Ni PNJ ni quêtes** : greffon de 2.0. Aucun magasin de documents à écrire
  aujourd'hui — seulement la couture pour qu'activer le greffon n'oblige pas à
  migrer les projets de 1.0.
- **Solo**, échange de fichiers exportés. Journal **linéaire**, staging par
  copie locale. Un log d'opérations synchronisé aurait changé le staging,
  l'annulation et le stockage : la question est tranchée, on ne la rouvre pas
  sans raison.
- **Annulation persistante avec points de reprise nommés.** Le journal vit sur
  disque, à format figé et versionné.

## L'idée centrale

Une section Anvil porte **une palette et 4 096 indices**. Presque toutes les
opérations WorldEdit s'expriment sur la palette seule. Le chemin rapide n'est
donc pas « itérer plus vite », c'est **ne pas itérer** :

| Étage | Quand | Coût |
|---|---|---|
| **Palette** | section entièrement couverte, correspondance valeur → valeur | O(palette) |
| **Section** | section entièrement couverte, résultat uniforme | O(1) |
| **Bloc** | bordure de sélection, ou opération qui lit vraiment chaque case | O(blocs), parallèle |

Mesuré sur 100 663 296 blocs : 0,26 ms par palette contre 26,8 ms par bloc
contre 11 718 ms pour le moteur JS. Voir `docs/RESULTATS.md`.

## Invariants à ne jamais casser

1. **On ne touche jamais au fichier source.** Toute opération travaille sur une
   copie de staging et commence par un instantané des sections qu'elle va
   toucher.
2. **Le round-trip Anvil est lossless.** Un chunk non modifié est réémis
   **octet pour octet** — on garde sa charge compressée brute, on ne la
   reconstruit pas. Vérifié en relisant le fichier produit avec un décodeur
   **indépendant**, à garder GELÉ : s'il partageait du code avec `tf-anvil`, le
   test ne prouverait plus rien.
3. **Un bloc n'est JAMAIS un objet.** C'est un `u32` interné dans une palette,
   et la section reste **packée**. C'est le mur qui a tué `we-engine` (154
   o/bloc) et MCEdit2 avant lui.
4. **L'étage palette ne dédoublonne pas** — et donc : *tout ce qui cherche un
   état dans une palette cherche TOUTES les occurrences, jamais la première.*
   Voir les pièges.
5. **Toute génération aléatoire prend une seed**, et un tirage PAR BLOC se
   hache sur la **position** (`hash3(x, y, z, seed)`), jamais sur un état. Un
   générateur à état n'est rejouable que si la boucle visite les cases dans le
   même ordre — contrainte invisible qu'une parallélisation casserait sans
   bruit. C'est ce qui rend `rayon` utilisable sans condition.
6. **Aucune écriture dans une save sans sauvegarde préalable**, et dans cet
   ordre : refuser si Minecraft tient le monde, sauvegarder en zip horodaté,
   puis écrire. Une sauvegarde prise après la première écriture ne sauvegarde
   plus rien.
7. **Il n'existe aucun état « le monde est chargé ».** Une région pleine, c'est
   déjà 100 millions de blocs ; un monde Minefield en fait 80 milliards. Il n'y
   a qu'une fenêtre de résidence plafonnée en **octets**, pilotée par la caméra
   et la sélection, avec éviction LRU.
8. **Une opération ne paie que sa PORTÉE.** Ses bornes décrivent ce qu'elle a
   vraiment écrit — l'instantané d'annulation et le remaillage incrémental s'y
   fient.

## Conventions

- Repère Minecraft partout : **+X = Est, +Z = Sud, +Y = Haut**. Index d'un bloc
  dans une section : `i = y*256 + z*16 + x` (ordre **YZX**).
- Toute traduction monde → région/chunk passe par une division **PLANCHER**.
  Le bloc −1 est dans la région −1, pas la région 0.
- Clé d'identité d'un bloc : le nom seul s'il n'a pas d'état, sinon
  `nom|k=v,k=v` **trié**. Une seule règle dans tout le dépôt — deux règles
  différentes dédoubleraient les palettes.
- **On n'écrit jamais un état par défaut.** `grass_block`, pas
  `grass_block[snowy=false]` : la palette se dédoublerait et un `//replace`
  visant un état exact en raterait la moitié.
- Chaque fonction arrive **avec ses tests**, déterministes.
- **Pas de fixture binaire dans le dépôt** : les tests et les benchs
  construisent leurs régions à la volée depuis une graine. Un `.mca` commité
  est opaque en revue et impossible à faire évoluer.
- Un écart de performance ne s'annonce qu'au-delà de **25 %**, sur une
  **médiane de N**. Mesuré dans `we-engine` : `mirror-rotate` a bougé de 18 % à
  code identique.
- Messages de commit en **français**.

## Commandes

```bash
cargo test            # tous les crates (240 tests aujourd'hui)
cargo clippy --all-targets
cargo fmt

# croisement avec un producteur TIERS (le moteur JS d'ExeWorldEdit) :
node proto/fixtures/gen.mjs
TF_MCA_TIERS=./r.0.1.mca cargo test -p tf-anvil --test croisement -- --nocapture

# mesure continue — fixture construite en Rust, aucune dépendance extérieure
cargo bench -p tf-bench
cargo bench -p tf-bench -- --save-baseline v0   # figer la référence
cargo bench -p tf-bench -- --baseline v0        # comparer

# le prototype de mesure (gelé, cité dans docs/RESULTATS.md)
cargo run --release -p tf-proto -- ./r.0.1.mca
```

Un écart ne compte **qu'au-delà de 25 %**, sur une médiane : mesuré dans
`we-engine`, un scénario a bougé de 18 % à code identique.

Le profil de test active `overflow-checks` : tout ce dépôt est de
l'arithmétique d'indices, et un débordement silencieux y produit une corruption
de save, pas un plantage.

## Structure visée

```
crates/
  tf-nbt/      lecteur zéro-copie CIBLÉ, écrivain  ✅ phase 0
  tf-anvil/    .mca lecture/écriture, splice lossless, 1.13→1.21, .mcc  ✅
  tf-blocks/   BlockState internés, palettes, règles de rotation dérivées
  tf-world/    adressage ✅ · résidence ✅ · source ✅ · staging ✅ · journal ✅
  tf-ops/      répartition 3 étages, masques, motifs, sélections-prédicat
  tf-formats/  .schem · .schematic · .litematic · .nbt
  tf-assets/   jars de mods, packs, blockstates→models→textures, atlas
  tf-mesh/     greedy + AO, cuisson des modèles, mips de LOD
  tf-render/   wgpu : arène, multi-draw indirect, HZB, transparence
  tf-app/      coque winit + egui, outils, commandes
  tf-bench/    criterion + générateurs de fixtures  ✅ phase 0
```

`proto/` est le prototype de performance. Il ne fait pas partie du produit :
il ne prouve que des chiffres, et disparaîtra quand `tf-anvil` et `tf-ops`
couvriront le même terrain.

## Pièges déjà rencontrés

Les pièges hérités d'`ExeWorldEdit` sont dans son propre `CLAUDE.md` — 44
entrées, à relire avant d'écrire l'équivalent Rust d'un module. Ceux-ci sont
propres à ce dépôt.

- **Dédoublonner une palette annule tout le gain de l'étage palette.** Sur du
  vrai terrain, une section qui contient de la pierre contient presque toujours
  de la terre : `//replace stone dirt` fusionne deux entrées, ce qui oblige à
  remapper les 4 096 indices. Mesuré : **9 216 sections sur 9 216** ont pris le
  chemin lent, et l'étage palette retombait au niveau de l'étage bloc (23,3 ms
  contre 26,8). Or Anvil **n'interdit pas** deux entrées identiques : le jeu lit
  `palette[indice]` et obtient un état valide dans les deux cas. En laissant le
  doublon, la longueur de la palette ne bouge pas, donc `bits` non plus, donc
  aucun indice n'est touché — **0,26 ms**. Le compactage se fait à l'écriture,
  jamais dans la boucle chaude.
  **Corollaire, et c'est la partie dangereuse :** chercher un état dans une
  palette avec un `position()` au lieu d'un filtre ferait rater la moitié d'un
  `//replace` suivant, sans la moindre erreur.
- **Un long NBT est BIG-endian.** Le lire en natif inverse silencieusement
  chaque indice de palette de la section : le build sort en bouillie et rien ne
  le signale.
- **Un parseur NBT généraliste est le goulot, pas la décompression.** Mesuré
  dans `we-engine` une fois le dépack de sections corrigé : NBT 35 %, dépack
  25 %, inflate 9 %. D'où un lecteur **ciblé** qui ne descend que dans
  `sections[].block_states` et enjambe le reste sans le matérialiser.
- **Un fil par chunk ne gagne rien — en JS.** La note d'`ExeWorldEdit` est
  exacte, mais sa cause est le `structuredClone` de l'arbre NBT, pas le calcul.
  En mémoire partagée la section redevient l'unité de travail naturelle.
- **Ré-encoder un chunk pour changer trois blocs détruit ce qu'on n'a pas
  compris.** `we-engine` garde la charge compressée d'un chunk NON modifié, donc
  son round-trip est lossless — mais dès qu'un chunk est modifié, il ré-émet
  tout son arbre NBT, et tout ce que `prismarine-nbt` a mal interprété est
  perdu. Ici un chunk modifié n'est pas ré-encodé : on note la PLAGE d'octets de
  chaque `block_states` (`tf_nbt::Span`) et on remplace uniquement celles qu'on
  a touchées. Heightmaps, structures, `neoforge:attachments` : le lecteur n'y
  touche pas, **donc il ne peut pas les abîmer**. Un test le prouve sur 256
  octets de données de mod et une chaîne en hangeul.
- **Une longueur NBT est un `i32` SIGNÉ.** La convertir en `usize` sans la
  borner transforme `-1` en 18 exaoctets, et la réservation tue le processus.
  Même chose dans l'autre sens : `i32::MAX` longs annoncés dans un fichier de
  quatre octets. On vérifie la place AVANT de réserver, jamais après.
- **Un débordement de pile n'est pas rattrapable en Rust.** Un `.mca` forgé
  avec 100 000 compounds imbriqués fait récurser le lecteur jusqu'à la mort du
  processus, sans message. D'où un plafond de profondeur explicite.
- **Le packing est une propriété du CHUNK, pas de la section.** Une section
  homogène n'a aucun tableau d'indices, donc rien à mesurer. Lui donner le
  packing moderne par défaut écrirait du 1.16+ dans un fichier 1.15 le jour où
  elle cesse d'être homogène — et c'est le seul cas que personne ne pense à
  tester. Les sections muettes héritent du packing qu'une section voisine a
  permis de mesurer ; quand aucune ne le permet, et seulement là, le
  `DataVersion` départage.
- **Le bit 0x80 de l'octet de compression n'est pas une compression.** Il
  signale une charge déportée en `c.X.Z.mcc`. Sans le masquer, `2 | 0x80` = 130
  se lit comme une compression inconnue et le chunk devient illisible.
- **Une palette ne peut pas dépasser 4096 entrées.** Une section n'a que 4096
  blocs, donc au plus 4096 états distincts y sont référencés — et le jeu ne lit
  pas au-delà de 12 bits par indice. Sans plafond, `bits_for(5000)` rendait
  13 bits et un fichier qu'aucun Minecraft ne relit ; au-delà de 16 bits, les
  indices sortaient tronqués à 65535 dans le `u16` du dépack. `repack` compacte
  quand il le faut, et **seulement** quand il le faut : compacter à chaque fois
  coûterait 103 ms par région pour rien.
- **`local_index` déborde sur l'axe voisin.** `local_index(16, 0, 0)` vaut 16,
  c'est-à-dire `(0, 0, 1)` : sans borne, `get` rendait le bloc de la case d'à
  côté. Un résultat parfaitement plausible, et faux. Toute lecture de bloc
  borne ses coordonnées.
- **Un tri sur une seule clé rend un résultat dépendant de l'ordre d'entrée.**
  `splice` triait sur le seul début de plage : une insertion en `p` et un
  remplacement commençant en `p` passaient ou rendaient `Overlap` selon lequel
  arrivait en premier dans le vecteur. Le tri porte sur `(début, longueur)`,
  donc les insertions passent avant — et deux appels au même ensemble
  d'éditions font la même chose.
- **Une recherche linéaire par BLOC est une boucle quadratique.** `count_of`
  faisait `hits.contains(v)` sur chaque bloc : O(palette × 4096), mesuré à
  340 µs pour une section à grosse palette. Une table indexée sur la palette,
  construite une fois, ramène ça à O(palette + 4096). Même famille que le
  `findIndex` de `we-engine`, sous une autre forme.
- **Une couture qu'on ne pose pas au départ devient une refonte.** Le journal
  d'annulation a failli être conçu comme « des instantanés de section
  compressés ». Correct pour les blocs, et impossible à étendre à ce qu'un
  greffon pose — donc une refonte de toute la pile d'undo, et de tout ce qui
  l'utilise. Coût de la version typée dès le départ : quelques heures.
- **Ce qui est ancré dans le monde doit suivre les blocs qui le portent.** Un
  PNJ posé à (120, 64, −300) bouge si l'utilisateur translate la région. C'est
  le même piège que `we-engine` a payé sur les block entities — « un build
  pivoté abandonne ses coffres » — et il reviendra à chaque nouveau type de
  donnée ancrée.
- **Une optimisation non vérifiée est une corruption silencieuse.** Ici elle
  tombe sur la sauvegarde d'un utilisateur. Toute stratégie rapide se compare
  au résultat de la stratégie lente sur le même monde, et le compte doit être
  exact au bloc près — pas « du même ordre ».
- **Une plage de réécriture trop large coûte autant que le journal.**
  `block_states` (1.18+) était relevé comme UN bloc : changer un nom de palette
  traînait les 4 096 indices derrière lui. Mesuré sur une région pleine,
  30 799 872 octets réécrits pour un `//replace` qui ne touche que des noms.
  Les plages sont séparées, et chaque édition est RESSERRÉE sur ce qui diffère
  vraiment (`trim_edit`) : **175 104 octets**, × 176. Le suffixe commun se
  compare depuis la FIN des deux tampons, donc il tient même quand le nouveau
  nom est plus court et décale tout ce qui suit. L'enjeu n'est pas le splice,
  qui recopie de toute façon : c'est l'annulation, qui stocke les octets des
  deux sens.
- **Un drapeau de propreté finit par mentir ; des octets, non.** `section_edits`
  compare ce qu'il va écrire à ce qui est DÉJÀ là. Une opération qui ne change
  rien ne salit donc aucun chunk — et ne remplit pas le journal d'entrées
  vides.
- **Une section 1.18+ qui redevient homogène doit PERDRE son `data`.** Le jeu
  n'en écrit pas pour une palette d'une entrée, et en laisser un de la mauvaise
  longueur casse le chargement du chunk. C'est pour ça que la plage relevée est
  celle du CHAMP entier et pas de sa seule charge.
- **Le sens « refaire » ne se déduit pas du sens « annuler ».** Le reconstruire
  après coup demanderait les octets d'après, qui ont justement disparu. Le
  journal stocke les deux — ce qui n'est bon marché que parce que les éditions
  sont resserrées.
- **Un journal réécrit à chaque action perd tout à la première coupure.** Le
  fichier est en AJOUT SEUL : annuler, refaire, abandonner une branche
  n'écrivent que quelques octets à la fin. Un enregistrement coupé en plein vol
  est repéré par sa longueur et son empreinte, et la lecture s'arrête là —
  perdre la dernière action vaut mieux que perdre l'historique.
- **Le discriminant d'un `enum` n'est pas un format de fichier.** Insérer une
  variante décalerait tout ce qui est déjà sur le disque d'un utilisateur, et
  ses annulations viseraient le mauvais dossier. `Folder::code()` est la
  correspondance stable, écrite à la main.
