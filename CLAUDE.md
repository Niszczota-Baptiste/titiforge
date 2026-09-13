# CLAUDE.md — contexte pour Claude Code

Lu automatiquement au démarrage. À garder court : ce qui change à chaque commit
appartient au code ou à `docs/`.

## Ce qu'est ce dépôt

**titiforge** — éditeur de mondes Minecraft Java pour des sélections de
plusieurs **milliards** de blocs. Cœur Rust, rendu wgpu, coque `winit + egui`.

Successeur d'`ExeWorldEdit` (Electron + JS), qui **reste en service** et n'est
pas modifié par ce dépôt. Il sert de référence d'écriture : sa table d'états de
blocs, son packing Litematica et ses 44 pièges sont du savoir payé cher.

Cible : Minecraft Java récent, Fabric / Forge / NeoForge, blocs moddés. Gros
builds pour le serveur Minefield.

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
cargo test            # tous les crates (113 tests aujourd'hui)
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
  tf-world/    résidence LRU par octets, streaming, staging, undo par section
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
- **Une optimisation non vérifiée est une corruption silencieuse.** Ici elle
  tombe sur la sauvegarde d'un utilisateur. Toute stratégie rapide se compare
  au résultat de la stratégie lente sur le même monde, et le compte doit être
  exact au bloc près — pas « du même ordre ».
