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
| Formes non-cubes | **73,7 %** (1 236 blocs) ; cubes pleins : 25,1 % (422) |
| Cuboïdes d'un bloc-modèle | **3,52** en moyenne, médiane 1, pire cas 82 |
| Textures citées par les blocs | **2 207**, dont 139 animées et 913 trouées |
| Blocs portant un état | **54 %** (910), à transformer sous rotation |
| Variantes avec rotation | **22 627** déclarées dans `blockstates.json` |
| Le plus complexe | `red_pumpkin_treat_bag` : **82 cuboïdes** dans une case |

Trois conséquences qui ne sont pas négociables :

1. **Le greedy meshing ne couvre qu'un tiers du CATALOGUE Minefield**, et la
   passe de modèles porte près de la moitié de la géométrie d'un build. Les deux
   chiffres sont différents et il faut les deux : 73,7 % des blocs du catalogue
   sont des modèles, mais dans un bâtiment ils ne sont que 9 % des blocs POSÉS —
   un mur est fait de cubes, le décor est semé. Sauf qu'un bloc-modèle vaut 3,58
   cuboïdes : sur une région bâtie, 2,87 M de blocs-modèles portent **9,46 M de
   cuboïdes** contre 28,7 M de cubes pleins qui se fondent en quads. La passe de
   modèles n'est donc pas un repli pour quelques escaliers, et la phase 2 se
   dimensionne sur ce chiffre-là — mesuré sur `Build` (`docs/fixtures.md`), pas
   déduit de la part du catalogue.
2. **Une table de rotation écrite à la main est impossible.** 910 blocs à état,
   avec quatre propriétés que le vanilla n'a pas — `vertical` (16 920
   occurrences), `offset`, `model`, `position`. Les règles DÉRIVÉES des
   `blockstates` ne sont pas un confort, c'est la seule option praticable.
   Mesuré (`tf-blocks`) : **99,3 % des rotations et 97,0 % des miroirs
   Minefield**, zéro faute aux lois du groupe sur 33 844 états, et ce qui
   manque est NOMMÉ. Détail et limites : `docs/RESULTATS.md`.
3. **Un `minefield:*` n'est JAMAIS remappé vanilla.** Sa géométrie et ses états
   se transforment ; son namespace, jamais. Invariant hérité d'`ExeWorldEdit`,
   et il devient central quand les deux tiers du catalogue sont custom.

Le codex extrait vit dans `titisite/public/codex/` : `codex.json` (2 034
entrées avec noms français et catégories), `blockstates.json` (2 560 entrées,
minefield ET vanilla), plus les modèles et textures. C'est la source de vérité
du catalogue — `ExeWorldEdit` le réextrayait par `npm run codex`.

## Où va ce projet

Pas « un MCEdit moderne » : **WorldEdit + MCEdit + SketchUp réunis**, et les
fondations d'un écosystème de création de contenu Minecraft. WorldEdit donne
les opérations de masse, MCEdit l'éditeur de monde, **SketchUp la
CONSTRUCTION** — pousser-tirer, inférence, et des composants qu'on modifie une
fois pour les mettre à jour partout. Les deux premiers transforment ce qui
existe ; le troisième permet de concevoir, et il n'existe nulle part sur
Minecraft.

**Conséquence directe sur tout ce qui s'écrit dès maintenant :** un composant
suppose qu'une opération soit REJOUABLE depuis ses paramètres, pas seulement
annulable depuis ses octets. La couture est POSÉE : `Genre::Operation` porte
un `params: Vec<u8>` opaque au journal, et `RapportRegion::journaliser` est la
seule jonction entre une opération et le journal. Une opération nouvelle qui
veut être rejouable y sérialise son `Plan` ; celle qui ne le veut pas passe
`Vec::new()` et s'annule comme avant. Détail dans `docs/ROADMAP.md`, « La
couture à poser avant la coque ». PNJ, quêtes, routes, outils
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

**Deux MODES, et un bouton.** Éditer un monde et concevoir un bâtiment ne se
pilotent pas pareil : en **Édition** on VOLE dans le monde et on sélectionne
un VOLUME ; en **Conception** on ORBITE autour de ce qu'on bâtit et on
sélectionne des ENTITÉS (face, arête, composant). Trois conséquences pour le
cœur : *le moteur n'a pas de mode* — une sélection de Conception se résout en
volumes avant d'atteindre `tf-ops` ; *le clic droit reste à la caméra dans les
deux* ; *une bascule ne bouge jamais l'image*. Le pilotage est posé et testé
sans écran (`tf-render/src/controles.rs`), le reste est de la phase 4.

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
  est opaque en revue et impossible à faire évoluer. Il y en a **deux**, et les
  confondre fausse tout : `Terrain` (du sous-sol) mesure Anvil, `Build` (un
  bâtiment) mesure le rendu. Voir `docs/fixtures.md`.
- **Un réglage de fixture qu'on n'a pas mesuré se NOMME comme un réglage.** La
  densité de décor d'un build dépend de qui l'a construit ; la présenter comme
  un fait mesuré serait l'erreur que tout ce dépôt s'interdit. Les benchs la
  balaient.
- Un écart de performance ne s'annonce qu'au-delà de **25 %**, sur une
  **médiane de N**. Mesuré dans `we-engine` : `mirror-rotate` a bougé de 18 % à
  code identique.
- Messages de commit en **français**.

## Commandes

```bash
cargo test            # tous les crates (590 tests aujourd'hui)
cargo clippy --all-targets
cargo fmt

# croisement avec un producteur TIERS (le moteur JS d'ExeWorldEdit) :
node proto/fixtures/gen.mjs
TF_MCA_TIERS=./r.0.1.mca cargo test -p tf-anvil --test croisement -- --nocapture

# mesure continue — fixtures construites en Rust, aucune dépendance extérieure
cargo bench -p tf-bench
cargo run --release -p tf-bench --example profil_build     # ce que la fixture de BUILD produit
cargo run --release -p tf-bench --example poids_editions   # ce qu'une opération fait réécrire
cargo bench -p tf-mesh                                     # le maillage, passe par passe
cargo bench -p tf-ops                                      # les trois étages, avec leurs COMPTES
cargo run --release -p tf-ops --example compression        # ce que coûte chaque niveau
# le parallèle et le séquentiel doivent rendre le MÊME octet :
cargo run --release -p tf-ops --example empreinte
cargo run --release -p tf-ops --example empreinte --no-default-features
cargo run --release -p tf-mesh --example mailler_build     # la chaîne complète, quads contre instances

# Les outils acceptent les TROIS formes d'assets, reconnues au contenu :
#   un codex extrait (un seul blockstates.json), un pack (assets/<ns>/...,
#   en dossier ou en .zip), ou une INSTALLATION de launcher (versions/ +
#   resourcepacks/ + server-resource-packs/ — c'est là que vivent les vraies
#   textures minefield:*). Exemple :
#   capture.exe %APPDATA%\.minefield_1_18 vue.png 1600 --monde D:\monde

# le pack RÉEL du serveur (rien n'est copié dans le dépôt)
cargo run --release -p tf-assets --example recenser     -- ../titisite/public/codex
# la COULEUR des biomes : il faut une INSTALLATION, un pack seul n'a pas `data/`
cargo run --release -p tf-assets --example biomes       -- %APPDATA%\.minefield_1_18
cargo run --release -p tf-assets --example mailler_reel -- ../titisite/public/codex
# la table de tf-bench est ENGENDRÉE par le même code que l'application :
cargo run --release -p tf-assets --example engendrer_catalogue -- ../titisite/public/codex \
    > crates/tf-bench/src/catalogue.rs
TF_PACK=../titisite/public/codex cargo test -p tf-assets --test codex_reel -- --nocapture

# ce que les règles de transformation couvrent, sur le pack RÉEL
cargo run --release -p tf-blocks --example deriver -- ../titisite/public/codex

# UN VRAI MONDE, croisé avec un VRAI pack — ce qu'aucune fixture ne peut dire.
# --zone restreint à un rectangle de CHUNKS (sur un monde plat, 99 % d'air
# dilue tout), --mailler maille pour de vrai ce qui a été relevé.
cargo run --release -p tf-assets --example recenser_monde -- D:\monde ../titisite/public/codex
cargo run --release -p tf-assets --example recenser_monde -- D:\monde ../titisite/public/codex --zone "4,7,10,12" --mailler
# les deux invariants porteurs, sur des fichiers que MINECRAFT a écrits
cargo run --release -p tf-ops --example verite_terrain -- D:\monde minecraft:dirt

# UNE OPÉRATION SUR UN VRAI MONDE — le premier bout qu'on peut lancer soi-même.
# Sans --ecrire, tout vit dans une copie de travail et la save n'est pas touchée.
# UNE COMMANDE PAR LIGNE : PowerShell ne connaît pas la continuation d'un shell Unix.
cargo run --release -p tf-ops --example semer  -- D:\monde-essai
cargo run --release -p tf-ops --example editer -- D:\monde-essai
cargo run --release -p tf-ops --example editer -- D:\monde-essai --remplacer minecraft:stone minecraft:dirt --sel "0,-64,0,511,320,511" --compter
# copier / tourner / coller. Sans --pack, les cases bougent mais les états ne
# sont PAS réécrits — l'outil le dit plutôt que de le laisser découvrir en jeu.
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,60,0,31,90,31" --copier-vers "64,0,64" --tourner 90 --pack %APPDATA%\.minefield_1_18
# //move et //stack : composées, mais UNE seule entrée de journal
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,60,0,31,90,31" --deplacer "64,0,0"
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,60,0,31,90,31" --empiler 5 est
# les formes : centrées sur la sélection, et l'opération ne paie QUE la forme
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-40,0,63,20,63" --poser minecraft:stone --sphere 20
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-40,0,63,20,63" --poser minecraft:stone --sphere 20 --creux 2
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-40,0,31,-21,31" --poser minecraft:stone --murs 1
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-64,0,255,60,255" --naturaliser
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-64,0,63,63,63" --biome minecraft:desert
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-60,0,63,-20,63" --lisser 4 --passes 3
# //hollow : TOPOLOGIQUE. Ce qu'aucun chemin de vide ne relie au dehors s'en va.
# C'est la seule opération qui matérialise toute la sélection — elle l'annonce.
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-40,0,20,-20,20" --creuser
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "0,-40,0,20,-20,20" --creuser --epaisseur 3

# le .exe WINDOWS, depuis Linux — pour donner l'outil à quelqu'un qui n'a pas Rust
# (apt install mingw-w64 ; rustup target add x86_64-pc-windows-gnu)
cargo build --release --target x86_64-pc-windows-gnu -p tf-ops --example editer --example semer

# une image, SANS écran (lavapipe suffit : apt install mesa-vulkan-drivers)
cargo run --release -p tf-render --example adaptateur
cargo run --release -p tf-render --example capture -- ../titisite/public/codex vue.png 1000
# le même, sur une VRAIE save — --zone borne au chunk près, parce qu'il
# n'existe aucun état « le monde est chargé »
cargo run --release -p tf-render --example capture -- ../titisite/public/codex vue.png 1400 --monde D:\monde --zone "4,7,10,12" 
TF_PRES=1 cargo run --release -p tf-render --example capture -- ../titisite/public/codex pres.png
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

`docs/ETAT.md` est la page d'état : tests, mesures, trous, et **la commande qui
rejoue chaque chiffre**. C'est là qu'on regarde avant de dire « c'est rapide ».

## Structure visée

```
crates/
  tf-nbt/      lecteur zéro-copie CIBLÉ, écrivain  ✅ phase 0
  tf-anvil/    .mca lecture/écriture, splice lossless, 1.13→1.21, .mcc ✅
               block entities ✅ · biomes 1.18+ ✅
  tf-blocks/   règles de transformation DÉRIVÉES du pack ✅ · internement à venir
  tf-world/    adressage ✅ · résidence ✅ · source ✅ · staging ✅ · journal ✅
               (typé, ajout seul, avec paramètres de REJEU)
  tf-ops/      3 étages ✅ · masques ✅ · motifs ✅ · staging+journal ✅
               jonction rapport → entrée de journal ✅
               presse-papiers ✅ · rotation/miroir ✅ · block entities ✅
               //move ✅ · //stack ✅ · formes ✅ · portée `Colonne` ✅
               //naturalize ✅ · //setbiome ✅ · //smooth ✅ · //hollow ✅
  tf-formats/  .schem · .schematic · .litematic · .nbt
  tf-assets/   packs ✅ · modèles ✅ · textures ✅ · atlas ✅ · .jar + launcher ✅
               couleurs de biome DÉRIVÉES du jeu ✅
  tf-mesh/     glouton ✅ · modèles ✅ · instances ✅ · teinte par BIOME ✅
               (gloutonne ET modèles) · AO, LOD à venir
  tf-render/   wgpu : arène ✅ · hors écran ✅ · modèles ✅ · teinte ✅ · indirect, HZB
               pilotage : vol / orbite et la bascule ✅ (pur, sans écran)
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
- **Une fixture fausse dans le sens prudent reste fausse.** Le premier
  échantillon du catalogue forçait les blocs les plus lourds en tête : 7,58
  cuboïdes en moyenne contre 3,58 dans le codex. Un bench deux fois plus
  pessimiste que la cible ne protège de rien — il fait rejeter une optimisation
  qui suffisait. Et le pire cas (82 cuboïdes, un sur 1 121) mérite sa propre
  mesure au lieu de peser sur toutes les autres.
- **Sauter les `multipart` fausse le recensement d'un pack.** Un tiers des
  blocs Minefield déclarent leurs modèles en `multipart`, et le nom du modèle
  n'est pas celui du bloc (`ladder_dark_oak` pour `dark_oak_ladder`). En
  n'ouvrant que `models/` sur les seules `variants`, 353 blocs sur 1 678
  restaient non résolus — et la part de blocs-modèles sortait à 50 % au lieu de
  66,8 %. Un recensement qui laisse 21 % de trous ne dit rien.
- **Un mailleur qui paie le VOLUME au lieu de la SORTIE.** La passe gloutonne
  examinait ses 24 576 cases de masque par section quel que soit le contenu :
  mesuré au banc, une section **entièrement pleine** — six quads en sortie —
  coûtait 94 µs, et sur une salle décorée la gloutonne prenait 68 % du temps en
  produisant 10 % des quads. Même famille que le `warmup(extent)`
  d'`ExeWorldEdit`. L'opacité tient par RANGÉES de dix-huit cases dans un
  `u32`, « opaque et voisin transparent » est `rangee & !(rangee << 1)`, et on
  ne visite que les bits posés. × 1,8 sur la chaîne complète.
- **Une rangée qui TRAVERSE les tranches ne se recalcule pas par tranche.** Sur
  l'axe X, une rangée court le long de la profondeur : la recalculer dans la
  boucle des tranches la refaisait seize fois, et annulait tout le gain sur
  deux faces sur six.
- **Un masque reblanchi par tranche coûte ce qu'on vient d'économiser.** La
  fusion remet à zéro chaque case qu'elle consomme, et elle les consomme
  toutes : le masque ressort propre. Le nettoyer quand même, c'était 24 576
  écritures par section. Un `debug_assert` fige la propriété — une marque
  oubliée produirait un quad fantôme à la mauvaise profondeur.
- **La passe de modèles ne doit pas produire de géométrie.** 349 k
  blocs-modèles produisaient 5,8 M de quads, neuf dixièmes du maillage — alors
  que ces quads sont la MÊME géométrie répétée : deux dalles de chêne côte à
  côte n'ont pas deux modèles, elles ont deux positions. Une POSE fait huit
  octets, le modèle vit une fois dans un tampon indexé par l'état. Mesuré :
  93 Mo de quads remplacés par 2,8 Mo de poses. Contrepartie : le masquage des
  faces se déplace dans le shader, puisqu'il n'y a plus de faces à supprimer
  ici.
- **Un cuboïde en seizièmes ENTIERS re-quantifie le pack.** Mesuré sur les
  125 382 coordonnées de modèle du serveur : **17,2 % ne sont pas entières** —
  surtout des demis, mais aussi des dixièmes (`.6`, `.2`, `.4`) qu'aucune
  fraction binaire ne représente. Les arrondir déplacerait un cinquième de la
  géométrie, et une chaise sortirait de travers. Les cuboïdes et les quads sont
  en flottants ; ceux de la passe gloutonne tombent sur des multiples de 16,
  donc exacts et comparables sans tolérance.
- **`uv` absent n'est pas `[0,0,0,0]`, et ce n'est pas un cas limite.** Mesuré :
  **26 à 32 % des faces** du pack n'en déclarent pas. Les uv se DÉDUISENT des
  bornes du cuboïde — c'est ce qui fait qu'une dalle montre la moitié basse de
  sa texture au lieu de la texture entière écrasée sur huit seizièmes.
- **`cull` ne porte que les faces qui déclarent `cullface`**, pas toutes celles
  qui sont à ras. Une dalle a son dessus à ras et sans `cullface` : un bloc
  posé dessus ne doit pas le faire disparaître.
- **Une variable de texture renvoie souvent à une autre.** `#top` → `#all` →
  `block/stone`. Sans suivre la chaîne, la face porte le nom d'une variable et
  aucune texture n'est trouvée. Et une variable qu'on ne résout pas RESTE
  nommée : la remplacer par du vide ferait dire « texture absente : » sans
  nommer la fautive.
- **Un modèle nomme son parent, et un pack vient du disque d'un utilisateur.**
  `../../../etc/passwd` est un nom de parent bien formé, et `a` parent de `b`
  parent de `a` boucle jusqu'à la mort du processus. Les deux sont refusés.
- **Un atlas est un TABLEAU de textures, pas une planche.** Le maillage est
  glouton : un quad couvre plusieurs blocs et sa texture doit se répéter. Sur
  une planche, `fract` sort de la tuile et mord sur la voisine — la frange
  classique de la mauvaise texture au bord des faces. Une couche par tuile et
  la répétition ne peut pas en sortir. Rien à border, rien à rogner.
- **Un pack n'est pas uniformément en 16 × 16.** Mesuré : 92,7 % le sont, et
  il y a du 32 × 32 dans le même pack. Un tableau n'a qu'une taille de couche :
  on prend la PLUS GRANDE et on agrandit les petites au plus proche voisin —
  réduire perdrait la moitié des pixels, et une interpolation ferait baver
  chaque bord de bloc. Avec un plafond : un pack HD en 512 donnerait 3,7 Go.
- **242 textures du pack sont des bandes d'animation**, jusqu'à 32 images
  empilées. Les prendre pour une image unique écraserait trente-deux vues du
  feu sur une face. Le codex ne contient aucun `.mcmeta` : le nombre d'images
  se déduit de la géométrie (`hauteur / largeur`), ce que fait aussi le jeu
  quand le mcmeta ne dit rien.
- **Cinq types de PNG cohabitent dans un pack** — RGBA, palette, RGB, gris, et
  du 4 bits. Ne lire que le RGBA laisserait 1 010 textures sur 3 556 au bord de
  la route.
- **« Une texture transparente quelque part » ne rend pas un bloc translucide.**
  Ça classait 864 blocs sur 2 560, dont `grass_block` — sa couche d'herbe est
  transparente sur les côtés, et tout le terrain serait devenu non opaque.
  C'est le piège de `grass_block` repris sous une autre forme. Seules comptent
  les faces du cuboïde qui REMPLIT la case, et seulement celles à ras du bord :
  116 blocs, ce qui est le bon ordre (feuilles, verre teinté, cactus, glace).
- **Deux implémentations d'une même règle divergent — troisième fois.** La
  table de `tf-bench` était produite par un script Python qui rejouait le
  classement de son côté : **16 désaccords sur 220 blocs**. Elle est maintenant
  ENGENDRÉE par `tf-assets`, le même code que l'application.
- **Un pixel totalement transparent n'a pas de couleur.** L'inclure dans la
  moyenne d'une tuile la tirerait vers le noir d'un fond qu'on ne voit jamais,
  et le facteur de teinte serait faux pour toutes les plantes.
- **Sans mipmaps, tout build vu de loin est du BRUIT.** Une texture de 16 × 16
  écrasée dans dix pixels d'écran échantillonne un pixel sur deux : à la
  première capture, un mur de pierre ressemblait à de la neige. Et la moyenne
  d'un mip se pondère par l'ALPHA — une feuille a des pixels transparents de
  couleur arbitraire, en général noire, et les moyenner à poids égal borde
  chaque feuille de noir à mesure qu'on s'éloigne.
- **Un tableau de textures est plafonné à 2 048 couches**, sur la carte comme
  sur le pilote logiciel. Le pack du serveur en cite 2 207 : charger tout le
  catalogue dépasse la limite ET paie des textures qu'aucun bloc de la scène
  n'emploie. On ne monte que celles des blocs PRÉSENTS.
- **Ne pas trier les faces par orientation.** Le mailleur n'émet que des faces
  visibles et ne garantit pas leur sens de rotation : activer le culling par
  orientation en ferait disparaître la moitié, et c'est le genre de défaut
  qu'on met des heures à voir.
- **Une copie GPU exige des lignes alignées sur 256 octets.** L'ignorer rend
  une image décalée d'un peu plus à chaque ligne — une image en escalier,
  parfaitement plausible et fausse.
- **La profondeur de wgpu va de 0 à 1, pas de −1 à 1.** Se tromper de
  convention met TOUT derrière le plan proche : image noire, aucune erreur.
- **Une section d'air coûte 17 µs au mailleur, et il ne peut rien y faire.**
  De l'air et des plantes sont indiscernables du point de vue de l'opacité.
  C'est l'APPELANT qui le sait gratuitement — sa palette a une entrée, et c'est
  de l'air. Sur un monde plein de ciel, c'est le poste principal.
- **Le discriminant d'un `enum` n'est pas un format de fichier.** Insérer une
  variante décalerait tout ce qui est déjà sur le disque d'un utilisateur, et
  ses annulations viseraient le mauvais dossier. `Folder::code()` est la
  correspondance stable, écrite à la main.
- **Une règle de rotation est une permutation d'ÉTATS, pas de propriétés.**
  C'est la mesure qui l'a imposé, et le contre-exemple n'est pas exotique :
  sur les escaliers du serveur, `shape=outer` (un ajout Minefield, une seule
  écriture par coin) exige `facing: east → south` sous miroir est-ouest,
  pendant que l'escalier DROIT exige `east → west`. Les deux sont
  géométriquement JUSTES ; aucune permutation de `facing` ne fait les deux.
  393 blocs concernés, toute la famille des escaliers. La forme par propriété
  reste — comme REPLI pour un état que le pack ne déclare pas — et elle DIT ce
  qu'elle ne peut pas porter (`Manque::NonDecomposable`) au lieu de se taire.
- **Deux modèles qui décrivent le même solide ne le découpent pas pareil.**
  `oak_stairs_inner` réfléchi est exactement `oak_stairs_inner` tourné — même
  solide, boîtes différentes. Comparer les listes de cuboïdes répondait
  « formes différentes », la route géométrique ne trouvait RIEN pour les
  escaliers en coin, et le miroir retombait sur la route des angles, qui rend
  l'escalier TOURNÉ. On découpe sur la grille induite, on refusionne dans un
  ordre fixe, et la comparaison porte sur le SOLIDE. Corollaire : les boîtes
  PLATES (une fleur en croix) n'occupent aucune cellule — sans mise à part,
  elles disparaîtraient.
- **Une preuve qui envoie un état sur lui-même ne prouve rien.** Une forme
  symétrique réfléchie se ressemble, et n'a donc rien à dire sur un `facing`
  qui veut dire plus que le dessin : `minecraft:bell` en
  `attachment=double_wall` est symétrique est-ouest, concluait « east → east »,
  et contredisait le « east → west » des trois autres attaches. L'immobilité
  n'est pas jetée, elle est RELÉGUÉE — et ne se juge qu'après les
  déplacements, sinon une trappe fermée (immobile sous toute rotation)
  contredit le `facing` que la trappe ouverte vient de donner.
- **Partager un angle ne rend pas l'angle faux.** J'avais gardé « la source
  doit être seule sur sa géométrie », pour écarter la trappe fermée. Mais sur
  le pack du serveur une trappe OUVERTE partage son (modèle, angle) entre
  `half=bottom` et `half=top`, un escalier entre trois états : la garde coupait
  donc la route des angles — la seule exacte — sur tous les escaliers, trappes,
  portes et portillons. 35 blocs Minefield retombaient sur une règle IDENTITÉ
  **silencieuse**, comptée comme une réussite. Partager un angle veut seulement
  dire que plusieurs états ont cette orientation ; c'est au départage de
  choisir lequel.
- **L'identité n'est pas toujours un aveu d'ignorance.** Une bougie a `candles`
  et `lit`, pas un champ qui oriente : aucune rotation de sa forme n'est
  déclarée nulle part, et pourtant refuser rendrait `//rotate` inutilisable sur
  tout un mur décoré. L'identité est la seule fonction TOTALE sur cet espace
  d'états, et c'est ce que fait le jeu. **Mais** dès qu'un modèle est déclaré à
  DEUX angles, `y` porte du sens et l'angle manquant est un vrai trou —
  `minecraft:snow` n'a de formes qu'au nord et au sud. La différence entre les
  deux est le seul contrôle qui sache dire « cette rotation n'existe pas » :
  les angles déclarés doivent être CLOS par la transformation.
- **L'injectivité se construit, elle ne se vérifie pas.** Tous les candidats
  d'un état rendent le MÊME solide — ils sont sa classe d'arrivée, et le pack
  ne dit rien de plus. N'importe quelle bijection entre les deux classes est
  donc visuellement juste : on honore les preuves, puis on complète sur ce qui
  reste LIBRE. Vérifier après coup laissait deux escaliers en coin tomber sur
  le même état, et la moitié du mur disparaissait à l'annulation.
- **Les deux miroirs ne sont pas indépendants.** `z → −z` est `x → −x` suivi
  d'un demi-tour. Dérivés séparément, ils divergeaient là où le pack est
  asymétrique : mesuré, 97,0 % contre 84,5 % sur exactement les mêmes blocs. On
  en dérive UN et on compose l'autre — même principe que `rot180`/`rot270`,
  construites à partir de `rot90`.
- **Un manque réparé par composition n'est pas un manque.** Les manques
  s'accumulent par bloc et ne sont publiés qu'à la fin, quand on sait ce que la
  composition a rattrapé. Annoncer un trou qui n'existe plus est aussi trompeur
  que d'en taire un vrai — et noie les 78 vrais dans 471 lignes.
- **Les lois du groupe ne savent pas voir une règle FAUSSE.** Elles disent
  qu'une règle est cohérente avec elle-même : une règle qui tournerait tout
  d'un quart de trop les passerait toutes, et zéro faute sur 33 844 états ne
  prouvait donc rien. Le contrôle qui décide compare le SOLIDE de l'état
  d'arrivée à celui de la source transformée — il a trouvé **3 431 couples
  faux sur 132 380** pendant que les lois du groupe étaient au vert. Toute
  règle dérivée a besoin d'un contrôle qui ne relit pas le raisonnement qui
  l'a produite.
- **La géométrie ARBITRE, les angles départagent.** L'arithmétique d'angles
  suppose que la transformation du monde se ramène à un décalage de `y` :
  vrai pour une rotation, vrai d'un miroir seulement si le modèle est
  lui-même symétrique. Sur un escalier en coin elle désigne l'escalier TOURNÉ
  au lieu du réfléchi — et comme la géométrie d'un coin est ambiguë (deux
  écritures pour le même dessin), le repli sur les angles avait le dernier
  mot. C'étaient tous les escaliers en coin et toutes les portes ouvertes du
  pack. On ne garde des angles que ce qui a la bonne forme ; quand le pack ne
  déclare la forme transformée NULLE PART, les angles redeviennent seuls
  juges — et le résultat est annoncé comme approché.
- **« Approché » et « faux » ne sont pas la même chose, et doivent se
  distinguer.** Une bougie n'a aucun champ qui oriente : elle ne tournera pas,
  et c'est une limite du bloc. Un escalier qui rend sa version tournée au lieu
  de la réfléchie, c'est un bug. Les deux se lisent pareil sur une capture
  d'écran. `Manque::FormeApprochee` nomme le premier, un `debug_assert` refuse
  le second — et un test exige que le compte annoncé soit EXACTEMENT celui que
  le contrôle indépendant trouve, ni plus ni moins.
- **Dépacker APRÈS avoir pris la palette écrase la section entière.** `unpack`
  consulte la palette pour savoir si la section est homogène. Lui retirer la
  palette d'abord la faisait répondre « homogène », donc rendre 4 096 zéros,
  donc réécrire toute la section avec sa première entrée. Un mur entier changé
  de bloc, sans la moindre erreur — attrapé dès la première exécution par le
  croisement avec le chemin lent, et par rien d'autre. C'est très exactement
  l'invariant « toute stratégie rapide se compare au résultat de la stratégie
  lente », et il a payé son écriture le jour même.
- **Compter les blocs modifiés coûte 31 × l'opération.** À l'étage palette,
  c'est le parcours qu'on vient d'éviter : 1,35 ms sans, 50,1 ms avec, sur une
  région pleine. Le comptage exact est donc une OPTION, jamais un service rendu
  d'office — et un rapport qui rend `None` est plus honnête qu'un chiffre qui a
  coûté trente fois le travail.
- **Une sélection bordée d'un bloc coûte × 21.** Décalée d'un seul bloc, elle
  fait tomber 2 892 sections sur 24 576 à l'étage bloc : 1,35 ms deviennent
  28,9. Et une sélection d'utilisateur ne s'aligne presque jamais sur 16. C'est
  là qu'ira la prochaine optimisation, pas ailleurs.
- **Un tirage par bloc se fait dévorer par sa plomberie.** 18,6 ns par bloc
  contre 2,4 pour la même boucle sans tirage. La somme des poids se recalculait
  à CHAQUE bloc et le modulo était une division 64 bits, qui ne se pipeline
  pas ; et hacher chaque axe avec une avalanche complète coûtait trois fois ce
  qu'il fallait. Tirage compilé hors boucle, `(h × total) >> 64` au lieu du
  modulo, une seule avalanche : **−35 %**.
- **Un hachage bon marché se vérifie par PLAN, pas par moyenne.** Une
  proportion globale juste à 50 % ne prouve rien : le défaut qui se voit à
  l'écran est une tranche entière à 0 % ou 100 %, et une corrélation entre
  cases voisines. On vérifie donc chaque tranche de chaque axe, les quatre
  décalages de voisinage, et les bits BAS autant que les hauts — le tirage
  prend les hauts, un test naïf prendrait les bas, et l'un passerait pendant
  que l'autre montre un damier.
- **Un `StateId` n'a de sens que relativement à SON interner.** Deux lectures
  indépendantes du même monde numérotent dans l'ordre où elles rencontrent les
  états : si un seul chunk diffère, TOUTE la numérotation des chunks suivants
  se décale. Comparer les identifiants de deux lectures, c'est comparer deux
  systèmes de coordonnées — mesuré, toutes les valeurs décalées de 1, et une
  heure perdue à chercher un bug qui n'existait pas. Ce qui traverse deux
  lectures se compare par NOM, ou passe par une table partagée.
- **La jonction est l'endroit le plus dangereux, et elle se teste à part.**
  Le plan, le splice, le journal et la copie de travail sont justes chacun de
  son côté ; ça ne dit rien de leur raccord. `tf-ops/tests/edition.rs` vérifie
  ce que personne d'autre ne peut : la source reste intacte, un chunk hors
  sélection n'est pas touché, annuler rend le monde d'avant et refaire celui
  d'après — sur le CONTENU, pas sur un compte.
- **Filtrer APRÈS avoir itéré rend quadratique.** `appliquer_region` parcourait
  tous les chunks de la SÉLECTION puis jetait ceux des autres régions : une
  sélection de dix régions sur dix en contient 102 400, filtrés cent fois, soit
  dix millions d'itérations pour cent mille chunks utiles. Sur un monde
  Minefield une sélection peut faire des milliers de régions. Ça ne se voit sur
  aucune fixture — toutes tiennent dans une région — et ça rend l'outil
  inutilisable chez l'utilisateur. On COUPE la plage avant d'itérer.
- **Un indice hors palette tue le processus.** `bits` se DÉDUIT de la longueur
  de palette : deux entrées se lisent sur quatre bits, donc seize valeurs sont
  représentables pour deux valides. Un `.mca` corrompu, tronqué ou forgé en
  porte, et `palette[indice]` panique — sur la sauvegarde de quelqu'un.
  Trouvé à TROIS endroits : l'étage bloc des opérations, le compactage de
  palette, et le mailleur (celui-là tuait l'application à l'AFFICHAGE, avant
  même que l'utilisateur ait touché à quoi que ce soit). La règle : on ne
  touche pas à ce qu'on ne comprend pas — la case reste telle quelle et
  ressort à l'identique, ou vaut de l'air pour le mailleur. Coût mesuré de la
  vérification : +3,4 % sur le seul chemin qui la fait par bloc.
- **Le piège n° 1 s'est presque refermé une deuxième fois.** J'allais
  paralléliser le CALCUL d'une opération — les trois étages, 1,35 ms sur une
  région pleine. Découpée phase par phase, la chaîne complète dit : 541 ms de
  RECOMPRESSION sur 800, 89 de décompression, 65 d'application, 37 de décodage,
  5 de recollement. Le calcul qu'on avait soigneusement optimisé pèse **0,17 %**.
  Découper une chaîne AVANT de choisir quoi accélérer n'est pas une précaution,
  c'est la seule façon de ne pas travailler pour rien.
- **Le niveau de compression est un réglage, pas une constante par défaut.**
  Mesuré sur une région pleine : niveau 6 (le défaut de `flate2`) 550 ms pour
  5,0 Mo, niveau 2 **230 ms pour 5,6 Mo**, niveau 9 **4 371 ms** pour 4,7 Mo —
  huit fois plus lent pour 6 % de gain. La copie de travail se réécrit à chaque
  opération pendant que l'utilisateur attend ; elle s'optimise pour le TEMPS,
  comme le cache d'aperçu d'`ExeWorldEdit`. La contrepartie (+11,8 % de taille)
  est réelle et tient dans une constante, `NIVEAU_STAGING`.
- **Un résultat qui dépend du nombre de cœurs est un bug silencieux.** Deux
  utilisateurs obtiendraient deux mondes différents de la même opération, et un
  journal qui ne défait pas ce qu'il croit défaire. Ce qui rend la propriété
  vraie n'est pas la chance : le tirage se hache sur la POSITION, et le
  `collect()` de rayon garde l'ordre d'entrée. Les deux chemins sont croisés
  sur l'EMPREINTE du fichier produit — un test seul ne peut pas le faire, le
  choix se fait à la compilation.
- **Un pack ne décrit pas seize escaliers : il en décrit UN et le TOURNE.** La
  rotation vit sur la VARIANTE (`"x": 90, "y": 270`), pas sur le modèle, et
  elle n'était appliquée nulle part au rendu : tous les escaliers d'un build
  sortaient tournés vers l'est, toutes les échelles plaquées au nord. Aucune
  erreur à l'écran — ça se lit « le rendu est bizarre » et ça ne désigne pas la
  cause. Pire, la règle vivait en DOUBLE (ici pour le rendu, dans `tf-blocks`
  pour la dérivation) et les deux copies avaient divergé sur le sens de X :
  quatrième fois que ce piège se referme. Corollaire : **le sens d'une rotation
  s'ANCRE, il ne se lit pas dans un commentaire.** `mushroom_stem` déclare six
  rotations et NOMME la face visée par chacune — six rotations, six faces
  distinctes, le sens est forcé.
- **Un seul monde réel ne tranche rien non plus.** Le premier relevé donnait
  1,54 cuboïde par bloc-modèle contre 3,58 attendus, sur 42 chunks — trop peu
  pour corriger quoi que ce soit, et le dire était la seule réponse honnête. Il
  a fallu un SECOND monde, sans rien de commun avec le premier (vanilla contre
  Minefield, une ville dense contre un plot plat, 5 120 chunks contre 42) pour
  que **1,75** confirme **1,54**. Deux mesures qui se ressemblent en disent plus
  que l'une des deux, et une seule mesure n'est qu'une anecdote mieux habillée.
  Corollaire : la fixture n'a PAS été recalibrée pour autant — lui inventer une
  distribution de placement qu'on n'a mesurée que sur du vanilla la rendrait
  fausse sans qu'on sache dans quel sens. Le dimensionnement absolu vient des
  vrais mondes ; le bench reste un instrument de COMPARAISON, où un pessimisme
  uniforme ne trompe sur rien.
- **Une fixture ne peut trouver que ce qu'on savait déjà.** Tout ce que le dépôt
  affirmait venait de fixtures qu'on avait écrites soi-même : le décodeur et
  l'encodeur ne s'étaient jamais mesurés qu'à eux-mêmes. Le premier relevé d'une
  VRAIE save a sorti la rotation manquante en une ligne — `mushroom_stem`, un
  bloc plein du jeu, en tête d'un classement de blocs-MODÈLES. Et il a corrigé
  le chiffre qui dimensionne la passe de modèles : **1,54 cuboïde par
  bloc-modèle posé contre 3,58 attendus**, parce que l'échantillon est stratifié
  sur ce qu'un pack CONTIENT alors qu'un constructeur pose des dalles et des
  escaliers, jamais le sac de friandises à 82 cuboïdes. *La distribution de ce
  qui existe n'est pas celle de ce qu'on pose* — même erreur que le premier
  tirage du catalogue, survivant au niveau suivant (`docs/fixtures.md`).
- **Un `vec3<f32>` s'aligne sur SEIZE octets en WGSL, pas sur quatre.** Une
  structure Rust en `[f32; 3]` mise en face décale tout ce qui suit d'un champ
  sur deux, et le shader lit des bornes prises au hasard dans la table voisine.
  Mesuré : des traînées qui filent à l'infini depuis le build, et **aucune
  erreur de validation** — les deux côtés sont valides séparément, c'est leur
  RACCORD qui est faux. Tout ce qui traverse la frontière est en `vec4`, ce qui
  rend la correspondance LISIBLE au lieu de la faire reposer sur des règles de
  bourrage ; un test fige les tailles et les décalages.
- **Une teinte de bloc se rate de deux façons, et il faut éviter les deux.**
  Les textures teintées du jeu sont GRISES (`grass_block_top.png` vaut 147) :
  c'est le jeu qui les multiplie par une couleur de biome, ce que la face
  signale avec `tintindex`. En l'ignorant, tout le sol sort blanchâtre.
  Ensuite : **(a)** compenser le gris de la tuile pour retrouver la couleur de
  biome pleine fait monter le canal vert à 1,286, donc écrêter à 1 — une teinte
  ne peut qu'assombrir — pendant que le rouge passe à 0,987 sans être touché :
  le vert perd son avance et le sol sort OLIVE, (145, 147, 89). Un écrêtage par
  canal ne conserve pas une teinte, il la déplace. **(b)** une couleur de biome
  est en sRGB et le mélange se fait en LINÉAIRE (`Rgba8UnormSrgb` des deux
  bouts) : passée telle quelle elle délave, (113, 128, 90). Les deux ont du
  vert en tête ; seul le rapport au ROUGE les sépare. La règle est celle du
  jeu, `texel × teinte`, la teinte convertie en linéaire — mesuré (82, 109, 48)
  contre (84, 109, 51) dans le jeu.
- **Une absence ne se voit pas.** Les blocs-modèles étaient maillés, comptés,
  affichés dans le rapport — et jamais dessinés. Sur la première capture d'une
  vraie save, 3 957 blocs manquaient sans que rien ne le dise : une image
  incomplète reste une image plausible. C'est le pendant visuel de
  « déclaré, branché, testé — et inatteignable », et ça demande le même
  remède : un test qui exige la présence, pas un coup d'œil.
- **Optimiser la passe qu'on vient d'optimiser.** La pose a ramené les
  blocs-modèles de 1 552 Mo à 39 sur une région bâtie — et la mesure suivante a
  désigné la passe GLOUTONNE, restée à 132 Mo, soit trois fois plus. Un quad
  tombe toujours sur un bord de bloc et tient dans sa section : cinq bits par
  axe, quatre par dimension, trois pour la face — 26 bits, et l'instance passe
  de 32 octets à 16. On ne devine pas où est le poids, même juste après l'avoir
  déplacé soi-même. Corollaire : un empaquetage se vérifie par l'IMAGE — la
  capture d'un vrai build doit être identique octet pour octet, parce qu'une
  troncature d'un bloc ne plante rien, elle déplace un mur.
- **Une fonction de chemin qui ignore ce qu'on lui demande.** `Catalogue`
  passait au résolveur de modèles une fonction rendant TOUJOURS le chemin de la
  racine, quel que soit le maillon de la chaîne. Chercher le parent à l'adresse
  de l'enfant relit le même fichier : la chaîne se referme sur elle-même et
  tout se solde en `Boucle`. Résultat, **zéro modèle résolu sur un vrai
  `.jar`** — où chaque bloc vanilla descend de `block/cube_all` puis
  `block/cube`. Invisible sur le codex, dont les modèles sont APLATIS, donc
  invisible sur TOUT ce que le dépôt mesurait : il a fallu monter une fausse
  installation de launcher pour le voir. Deuxième fois de la séance qu'un
  format réel sort un défaut qu'aucune fixture ne pouvait montrer.
- **Un lecteur de ZIP se fie au répertoire CENTRAL, pas aux en-têtes locaux.**
  Un en-tête local peut annoncer des tailles nulles et renvoyer à un
  descripteur placé APRÈS les données, qu'on ne saurait pas trouver. Mais il
  porte ses PROPRES longueurs de nom et de champ supplémentaire, qui peuvent
  différer de celles du répertoire : prendre celles du répertoire décale la
  lecture de quelques octets, ce qui donne une charge illisible et aucune
  explication. Les deux tables servent, chacune à ce qu'elle décrit.
- **Toucher une section qu'on n'a pas changée la fait RÉÉCRIRE.** Annoncer
  l'étage bloc la fait passer par `section_edits`, qui la ré-encode pour la
  comparer — et le ré-encodage ne reproduit pas toujours les octets d'origine :
  une propriété d'état que le jeu écrit dans un autre ordre ressort
  normalisée. Mesuré sur un vrai monde 1.20, reposer un extrait à sa propre
  place produisait **six correctifs de journal pour zéro changement**, et le
  collage prenait 19 ms au lieu de 5. Une opération qui n'écrit rien doit
  rendre `Etage::Rien`, pas « bloc, zéro case ». Aucune fixture ne pouvait le
  montrer : elle écrit ses propriétés dans l'ordre où le décodeur les relit.
- **Le contenu d'un coffre n'est pas dans la grille de blocs.** C'est une liste
  à part du chunk, dont chaque entrée porte ses propres coordonnées MONDE. Rien
  ne la fait suivre les blocs toute seule et le format ne signale aucune
  incohérence : un build pivoté sort vide, et on l'apprend en ouvrant un
  coffre. L'entrée voyage donc par ses OCTETS, et on ne réécrit que les douze
  qui portent `x`, `y` et `z` — à une position que le balayage a relevée. Rien
  n'est ré-encodé, donc les données d'un mod dans un coffre ne peuvent pas être
  abîmées : c'est la propriété du splice, appliquée un cran plus bas.
- **Le RETRAIT d'une entité se déduit de sa CASE, jamais de la boîte de
  l'opération.** `Rapport::bornes` est une borne SUPÉRIEURE honnête : s'en
  servir pour décider quel coffre effacer détruirait celui qu'un `//replace` a
  seulement survolé. Le critère exact est « la case a-t-elle changé d'état ? »,
  et il ne coûte rien — on relève l'état des cases habitées avant l'opération,
  on le recompare après, et un chunk sans coffre ne paie pas un octet. Il vaut
  aussi pour toute opération future, ce qu'une règle écrite dans chaque
  opération ne ferait pas : c'est la JONCTION qui le fait, une fois.
- **Une entité posée doit prendre la place EXACTE de celle qu'elle remplace.**
  Ajoutée à la fin, elle réordonne la liste : mêmes entrées, autres octets,
  donc un correctif de journal pour zéro changement. Troisième forme du même
  piège, après le `position()` sur une palette dédoublonnée et l'étage bloc
  annoncé pour rien.
- **Une fixture dont deux ordres coïncident ne prouve pas qu'ils sont
  distincts.** Les coffres de la fixture avaient des `y` CROISSANTS : l'ordre
  du fichier était donc déjà l'ordre YZX, et la faute ci-dessus ne faisait
  rougir aucun test. Trouvée par MUTATION — casser la pièce et exiger qu'un
  test tombe — pas par relecture. Un test vert ne dit rien tant qu'on n'a pas
  vu ce qui le fait rougir. Les `y` décroissent maintenant, et c'est écrit à
  côté de la constante.
- **Une forme n'est pas un masque, et le confondre coûterait tout.** Un masque
  est un prédicat sur l'ÉTAT, donc il s'évalue une fois par ENTRÉE de palette ;
  une forme est un prédicat sur la POSITION, donc elle s'évaluerait 4 096 fois
  par section. Mettre une sphère dans `Masque` ferait retomber `//replace` à
  l'étage bloc. Une forme a son propre chemin rapide, sur l'autre axe : elle
  répond par SECTION (`Dedans` / `Dehors` / `Partielle`) avant de répondre par
  case, et le cœur d'une sphère garde donc l'étage palette.
- **Un verdict par section se croise aux 4 096 cases, jamais relu.** Un
  « Dedans » faux écrit hors de la forme, un « Dehors » faux y laisse un trou,
  et ni l'un ni l'autre ne se voit sur une capture d'écran. Mesuré par
  mutation : juger l'appartenance d'un ellipsoïde sur le point le plus PROCHE
  au lieu du plus LOIN passe tous les tests de `contient` et corrompt le
  résultat.
- **« La section est dans la forme pleine » n'implique pas « elle est dans le
  trou ».** Première écriture du test de coque : j'affirmais que toute section
  entièrement dans la sphère extérieure devait être écartée par la coque.
  Faux — une telle section peut chevaucher la paroi, et `Partielle` y est la
  bonne réponse. La propriété juste se dit avec le TROU, reconstruit
  indépendamment par les constructeurs publics : si la règle de
  rétrécissement change d'un côté seulement, le test le dit.
- **Un rayon nul divise par zéro, et `NaN` est faux dans les DEUX sens.**
  `0 / 0` propagé dans `<= 1.0` rend `false`, et dans `> 1.0` aussi : la forme
  serait à la fois vide et pleine selon le chemin. Un rayon nul n'accepte que
  le centre exact, et c'est écrit explicitement.
- **Une clé de fusion porte la VALEUR, jamais l'index qui la désigne.** Pour
  que la teinte de biome soit exacte, le biome entre dans la clé du mailleur
  glouton : deux cases de biomes différents ne fusionnent plus. J'y ai d'abord
  mis la CELLULE (le rang 0..63 qui porte le biome) — elle change à la même
  frontière, donc elle paraît équivalente. Elle est fausse : deux cellules
  VOISINES du même biome ont deux rangs différents, donc deux clés, donc plus
  aucune fusion au-delà de quatre blocs. Mesuré en écrivant la faute : une
  face de section pleine sortait en **64 quads de 4 × 4 au lieu d'un seul**,
  sur un terrain d'un seul biome. Le rendu en était juste ; le maillage,
  seize fois trop cher — et rien à l'écran ne l'aurait dit.
- **Le biome ne rejoint la clé que pour les états TEINTÉS.** L'y mettre
  partout couperait les quads d'une muraille de pierre à chaque frontière,
  pour une couleur que la pierre ne prend pas. C'est `Formes::teinte_biome`,
  et c'est faux par défaut.
- **La couleur d'un biome vit dans DEUX moitiés du jeu, pas une.** La table
  `assets/minecraft/textures/colormap/grass.png` donne une couleur par couple
  (température, humidité) ; les températures, elles, sont dans
  `data/<ns>/worldgen/biome/*.json`. Ressources d'un côté, DONNÉES de l'autre :
  un resource pack seul ne suffit pas, il faut une installation ou un
  datapack. C'est la seconde fois que lire l'installation de l'utilisateur
  paie — la première étant les textures `minefield:*`.
- **La formule de couleur du jeu se copie à la LETTRE, troncature comprise.**
  `(int)((1 − t) × 255)` sur `t = 0,8f` rend **50**, pas 51 : `0,8` vaut
  0,800000011920929 en flottant, le produit fait 50,99999…, et Java tronque.
  Arrondir « proprement » décalerait la table d'un pixel sur la moitié des
  biomes. Et l'humidité se multiplie par la température AVANT d'être inversée.
  Trois mutations, trois tests rouges — dont l'interversion des deux axes, qui
  donne un désert vert et une jungle jaune, deux images parfaitement
  plausibles.
- **Un biome n'est pas un bloc, et copier le code des blocs le casse de trois
  façons.** Sa palette est une liste de CHAÎNES (pas de compounds `{Name,
  Properties}`) ; il n'y a que 64 cellules par section, une pour 4 × 4 × 4
  blocs ; et surtout **il n'y a pas de plancher à quatre bits**. Cinq biomes
  se lisent sur trois bits : reprendre `bits_for` des blocs écrirait un
  tableau quatre fois trop long, valide pour personne, et le chunk ne se
  charge plus — sans que rien ne le signale à l'écriture.
- **La grille d'un biome se voit, et il faut le DIRE.** Une sélection d'un
  seul bloc en peint quatre par axe, parce que c'est l'unité du format. Taire
  le débordement ferait passer une propriété d'Anvil pour un bug de l'outil ;
  le rapport l'annonce à chaque `//setbiome`.
- **Ce qui ne tient dans aucune portée se fait en DEUX PASSES.** Lisser
  moyenne la hauteur d'une colonne avec celle de ses voisines, qui sont
  parfois dans un autre chunk. Élargir la vue coûterait cher à tenir juste —
  c'est le piège `cold_read`. La sortie est une passe de LECTURE, qui n'écrit
  rien et n'a donc aucune portée à respecter, un calcul PUR sur la carte de
  hauteurs, puis une écriture à portée `Colonne` qui applique une carte déjà
  calculée. Une carte de mille blocs de côté fait quatre mégaoctets : elle
  tient, et chaque chunk la lit en entier sans rien lui coûter.
  **La lecture déborde de la sélection du rayon du noyau** — sans cette marge,
  le bord se moyennerait contre des colonnes qu'on n'a pas lues, donc contre
  du vide, et s'effondrerait.
- **Une hauteur négative se moyenne en division PLANCHER.** Le monde descend à
  −64 depuis 1.18. Une division qui tronque vers zéro remonte le relief d'un
  bloc sous y = 0 et pas au-dessus : une marche d'un bloc à l'altitude zéro,
  exactement là où personne ne la cherche. Et `SANS_SOL` n'est pas une hauteur
  basse : le compter comme zéro creuse une fosse au bord de chaque sélection.
- **Une opération qui lit HORS de sa section ne peut pas être une opération de
  section.** « Où est la surface » est une propriété de la COLONNE : décidée
  section par section, la naturalisation poserait une bande d'herbe tous les
  seize blocs, au milieu de chaque falaise — régulièrement, donc visiblement,
  et sans que rien ne dise pourquoi. D'où `Portee` : l'opération DÉCLARE ce
  qu'elle lit, `edition.rs` lui donne la vue correspondante. Trop petite, elle
  lit de l'air ; trop grande, elle décode un chunk entier pour trois blocs —
  c'est `PORTEE` d'`ExeWorldEdit`, avec la même raison d'être.
- **« Pas de section ici » n'est pas « c'est de l'air », troisième fois.**
  `Colonnes::get` rend `None` là où le chunk ne porte rien. Rendre l'air
  ferait écrire de la pierre dans le vide, et le piège `cold_read` a déjà été
  payé deux fois sous d'autres formes.
- **Un `[usize; 3]` invite à échanger des unités.** Le témoin de coffre du
  chemin par colonne porte x et z LOCAUX et y MONDE ; écrits dans un tableau
  de trois nombres du même type, ils se sont échangés à la première écriture.
  Le compilateur l'a attrapé cette fois — un `struct` aux champs nommés fait
  qu'il n'y avait rien à attraper.
- **La documentation et le code peuvent se contredire sans qu'un test s'en
  aperçoive.** Le commentaire de `Naturaliser` disait « tout sauf l'air » et
  le constructeur posait `Masque::Tout`, ce qui aurait rempli le CIEL de
  pierre. Aucun test ne les départageait : ils comptaient tous des cases sans
  regarder LESQUELLES. Le test qui manquait tient en une ligne — le ciel doit
  rester du ciel.
- **L'idempotence ne prouve pas l'invariant n° 4.** Reposer une opération deux
  fois ne montre rien sur une palette dédoublonnée : à la seconde passe,
  toutes les cases pointent déjà sur la première occurrence. C'est la PREMIÈRE
  qu'il faut regarder — et comme elle écrit légitimement par ailleurs, la
  propriété se dit sur le COMPTE : ce que l'opération annonce doit être la
  différence mesurée case par case. Mesuré par mutation : 16 640 annoncés
  contre 1 848 réels.
- **Un branchement par bloc coûte, même parfaitement prédit.** Le test de
  forme dans la boucle de l'étage bloc était un booléen INVARIANT — le
  processeur le prédit à coup sûr — et il coûtait quand même **5,9 %** sur
  cent millions de cases. Passé en paramètre de COMPILATION
  (`etage_bloc::<BORDE>`), la boucle sans forme ne porte plus une instruction
  de plus qu'avant que les formes existent : **−2,2 %**, c'est-à-dire la
  parité au bruit près. Corollaire : une fonction appelée une fois par SECTION
  se mesure aussi — `Forme::couverture` laissée hors ligne coûtait 11,9 % à
  `//set`, dont l'étage entier est en O(1).
- **Un A/B qui `git stash` entre deux exécutions mesure aussi le
  rebuild.** Et il laisse l'arbre à moitié rangé si on l'interrompt. Deux
  binaires construits UNE fois — l'un depuis un `git worktree` sur le commit
  d'avant — puis alternés, c'est le même protocole en plus sûr et en plus
  rapide. C'est comme ça que le +11,9 % a été vu, puis corrigé, puis vérifié.
- **Les correctifs d'une entrée de journal s'annulent À L'ENVERS.** Chacun est
  gardé par l'empreinte de l'état qu'il attend. Tant qu'une opération ne
  touchait chaque chunk qu'une fois, l'ordre n'avait aucune importance et le
  test de jonction les rejouait dans le sens d'enregistrement — juste par
  accident. `//move` repasse sur les chunks que source et destination ont en
  commun : le second correctif échoue alors sur `Divergence`. Le sens vit
  maintenant dans `Entree::a_annuler` / `a_refaire` et nulle part ailleurs,
  parce qu'un appelant qui écrirait `corrections.iter()` à la main aurait
  raison jusqu'au jour où il aurait tort, sans prévenir. Une opération
  composée rend ses correctifs bout à bout (`RapportRegion::absorber`) : un
  seul `Ctrl+Z` doit défaire le déplacement entier, pas son dernier tiers.
- **`--quick` de criterion n'est pas une mesure.** L'A/B du suivi des block
  entities annonçait **−25 %** sur le balayage, un gain qu'aucune ligne du
  changement ne pouvait expliquer. Repris en runs complets, alternés, médiane
  de trois : 6,04 ms contre 6,07, soit rien. Un chiffre qu'on ne sait pas
  expliquer est un chiffre à re-mesurer, pas à publier.
- **Une ligne de commande écrite pour bash ne marche pas chez la cible.** Deux
  fois de suite, sur la même séance : la continuation `\` en fin de ligne, que
  PowerShell ne connaît pas, et les virgules de `--sel 0,-64,0,511` que
  PowerShell lit comme un TABLEAU et recolle avec des espaces avant de passer
  l'argument. Dans les deux cas l'erreur ne parle pas du programme, et
  l'utilisateur a tapé exactement ce que la documentation disait. Le
  `CLAUDE.md` le dit depuis le début — **Windows est la cible** — et ça vaut
  pour les EXEMPLES autant que pour les scripts. Corollaire : un outil accepte
  le rendu naturel de son propre shell (la virgule ET l'espace), sinon un
  oubli de guillemets devient un bug à déboguer.

- **Une entité POSÉE gagne sur le verdict d'orphelinage, et c'est voulu.** La
  jonction retire les block entities dont la case a changé d'état ; mais
  `liste_voulue` consulte d'ABORD les entités que l'opération pose, sinon
  reposer un extrait à sa propre place perdrait ses coffres. Conséquence pour
  `//hollow`, qui repose un extrait dont il vient de vider le cœur : un coffre
  resté dans cet extrait serait reposé DANS LE VIDE, par-dessus le verdict qui
  venait justement de le condamner. C'est `extrait_creuse` qui le retire, au
  moment où il vide la case — pas la jonction, qui a raison de faire l'inverse.
  Mesuré par mutation : sans ce retrait, `entites_posees` vaut 1 là où il doit
  valoir 0, et on l'apprendrait en ouvrant un coffre qui n'existe plus.
- **Creuser est TOPOLOGIQUE, pas géométrique — et les deux se ressemblent sur
  une capture d'écran.** « Enlever l'intérieur de la boîte » vide une salle
  déjà ouverte par une porte, et laisse pleine une sphère qui n'est pas une
  boîte. Le critère est « aucun chemin de VIDE ne le relie au dehors », donc
  une diffusion. Elle ne se découpe ni par section ni par colonne (un couloir
  traverse la sélection de part en part), donc `//hollow` est la seule
  opération qui matérialise tout le volume : c'est assumé, c'est borné, et
  c'est ANNONCÉ avant de commencer. Et la pile est explicite — une récursion
  sur une sélection de cent blocs de côté déborde, et un débordement de pile
  n'est pas rattrapable en Rust.
- **Le bord de la sélection EST le dehors.** Première écriture : seules les
  cases non solides du bord étaient semées, ce qui est juste, mais le bord
  solide n'était pas gardé — un cube entièrement plein n'avait donc aucune
  graine, aucun dehors, et partait EN ENTIER. La sélection s'arrête là ; ce
  qu'il y a au-delà ne nous regarde pas, donc sa peau touche l'extérieur par
  définition.

- **Une jonction que personne n'écrit est une jonction que chaque hôte
  réécrira.** Le journal savait annuler, les opérations savaient produire des
  correctifs, et RIEN ne reliait les deux : les tests recomposaient l'entrée à
  la main sous le commentaire « comme l'application le tiendra ». Trois
  occasions de se tromper par hôte — les correctifs, le nom, les bornes — dont
  une que ce dépôt a déjà payée : l'ordre des correctifs ne se voit que sur une
  opération qui repasse deux fois sur un chunk. Même famille que « déclaré,
  branché, testé — et inatteignable », et c'est la forme la plus coûteuse :
  celle où chaque appelant a l'air d'avoir raison.
- **Un champ ajouté au MILIEU d'un format décale tout ce qui est déjà sur le
  disque.** Les paramètres de rejeu s'écrivent en dernier dans le corps d'une
  entrée, et le lecteur traite « plus d'octets » comme « pas de paramètres ».
  Glissés entre `op` et `bounds`, ils auraient rendu illisible la première
  entrée d'un journal existant — et la lecture s'arrête à la première entrée
  incomprise, donc c'est tout l'historique qui serait parti, pas une ligne.
- **Un outil de CONTRÔLE qui accuse à tort coûte plus cher qu'un silence.**
  `verite_terrain` remplace la cible par `minecraft:stone` ; appelé avec
  `minecraft:stone`, il n'écrivait rien — ce qui est JUSTE — et annonçait une
  FAUTE du moteur. On part alors chercher un bug qui n'existe pas. Un argument
  dégénéré se refuse à l'entrée, en disant lequel.
- **La teinte d'un bloc-MODÈLE ne tient pas dans sa pose, et n'a pas à y
  tenir.** J'avais listé trois pistes, toutes coûteuses : des bits libres dans
  `Pose::local`, une table par section, un second tampon. Les trois passaient à
  côté du fait que la table de géométrie est déjà MÉMOÏSÉE — il suffit de la
  mémoïser sur `(état, biome)`. La pose reste à seize octets, le shader ne
  bouge pas d'une ligne, et il n'y a ni nouvel empaquetage ni nouveau tampon à
  tenir juste : aucun des trois endroits où ça se serait cassé en silence. Ce
  qui borne le coût est le ZÉRO que le mailleur écrit pour un état non teinté,
  la même règle que la clé de fusion gloutonne — sans lui, la géométrie d'un
  escalier se copierait autant de fois qu'il y a de biomes dans la scène.

- **`vec![]` n'échoue pas gentiment : une allocation refusée ABANDONNE le
  processus.** Presque tout le moteur travaille sur des sections packées et ne
  paie que sa portée ; trois opérations font exception et demandent une case
  par bloc en mémoire — `//copy`, `//paste`, `//hollow`. `copier` n'avait
  aucun plafond : « tout sélectionner » sur un monde Minefield demandait des
  dizaines de gigaoctets et faisait disparaître l'éditeur avec le travail en
  cours, sans un mot. C'est le piège des longueurs NBT (« on vérifie la place
  AVANT de réserver ») avec un nombre qui vient de la SOURIS au lieu d'un
  fichier — la conséquence est identique. Le plafond est en OCTETS et non en
  cases, comme la fenêtre de résidence : `//copy` coûte 4 octets par case,
  `//hollow` en coûte 12, et un plafond en cases mentirait à l'un des deux.
- **Une sélection est une BOÎTE, un monde est un SEMIS.** `appliquer`
  parcourait `sel.regions()`, c'est-à-dire la boîte englobante : sur une
  sélection de tout le monde, 13,7 MILLIARDS de régions pour la poignée qui
  existe. Ça ne plante pas, ça ne dit rien, ça ne finit pas — le pire des
  trois, et attrapé par un test que j'écrivais pour autre chose. Au-delà de
  1 024 régions dans la boîte, on demande à la source lesquelles existent.
  L'équivalence est EXACTE — une région absente rend `RapportRegion::default()`,
  et `absorber` d'un rapport par défaut n'ajoute rien — et elle est croisée
  contre le chemin lent sur les OCTETS produits, pas sur un compte. Corollaire :
  la couche de staging compte autant que la source, sinon une opération
  sauterait la région qu'une opération précédente vient de créer.

- **Un bouton de mode qui BOUGE l'image est un bouton qu'on n'ose plus
  toucher.** `Camera` porte un œil et une cible, donc la bascule vol ↔ orbite
  a l'air gratuite. Elle ne l'est pas : en vol, la cible est un point
  arbitraire posé à un mètre devant le nez, et orbiter autour d'elle ferait
  pivoter l'utilisateur autour de son propre nez. Le pivot se DÉCIDE — ce
  qu'on vise — et il se PROJETTE sur le rayon du regard : posé tel quel à
  quarante blocs de l'axe, il faisait sauter l'œil de quarante blocs. Un
  aller-retour doit rendre la caméra de départ, sinon le bouton coûte un
  recadrage à chaque appui — une dérive qu'on attribue à sa souris pendant
  des semaines. Corollaire : recentrer délibérément sur une sélection hors
  champ est un AUTRE geste, avec un autre nom, parce qu'il bouge la caméra.
- **`cible` est un POINT SUR le rayon, pas le rayon.** Un vol pose sa cible à
  une unité devant le nez, une orbite la pose sur son pivot à soixante blocs :
  deux points différents, la même direction, donc la même image — la matrice
  de vue ne garde que la direction normalisée. Mon test comparait les POINTS
  et rougissait sur du code juste, ce qui envoie chercher un bug là où il n'y
  en a pas. « La même image » se dit avec l'œil et la DIRECTION.
- **Une pente de caméra se borne, elle ne s'enroule pas.** À la verticale
  pile, le regard est colinéaire au haut du monde, leur produit vectoriel
  s'annule et la base de la vue devient dégénérée : l'image bascule d'un
  quart de tour sans prévenir. Un demi-degré de marge coûte ce qu'on ne voit
  pas et évite ce qu'on ne comprend pas. Même famille : un rayon d'orbite qui
  atteint zéro donne une direction nulle, donc des `NaN` — et `NaN` ne plante
  pas, il affiche du NOIR.
- **Un zoom à pas FIXE est inutilisable aux deux bouts.** Trop lent pour
  traverser un build, et il traverse l'objet d'un cran quand on est contre.
  Un facteur rend le geste identique à toutes les échelles — ce qu'exige un
  outil qui sert du bloc à la ville.
