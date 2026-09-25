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
pilotent pas pareil : en **Édition** gauche et droit posent les deux coins
d'un VOLUME, comme WorldEdit ; en **Conception** ils désignent des ENTITÉS —
face, arête, composant.

**Le point fixe de la caméra est le JOUEUR, jamais le build**, et dans les
deux modes : tourner fait pivoter le regard autour de l'œil, qui ne bouge pas.
C'est ce que fait Minecraft, donc ce que la main de quiconque construit sait
déjà faire. **La caméra ne dépend donc pas du mode** — seuls les boutons
changent de sens.

La répartition, et elle est FIXE : **molette ENFONCÉE** = tourner, molette +
Maj = panoramique, molette roulée = avancer ; **gauche et droit restent aux
outils**. C'est la caméra de SketchUp et la sélection de WorldEdit réunies
sans se marcher dessus — donner la caméra au clic droit coûterait l'un des
deux coins, et c'est le geste que tout utilisateur de WorldEdit connaît par
cœur. Le piège d'`ExeWorldEdit` — *un outil qui coupe la caméra entière
enferme l'utilisateur* — ne peut pas se produire : la caméra a un bouton à
elle, qu'aucun outil ne prend.

Conséquence pour le cœur : *le moteur n'a pas de mode* — une sélection de
Conception se résout en volumes avant d'atteindre `tf-ops`. Le pilotage est
posé et testé sans écran (`tf-render/src/controles.rs`), le reste est de la
phase 4.

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
cargo test            # tous les crates (906 tests aujourd’hui)
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
# ce qu'une IMAGE DE VOL coûte, cadencée à 60 i/s, par le chemin de la coque —
# arènes seules, puis avec la scène GPU refaite, puis synchronisée
cargo run --release -p tf-app --example vol
cargo run --release -p tf-app --example vol -- --gpu synchro
# ce qu'une RÉGION coûte à rendre résidente — le budget de la phase 5
cargo run --release -p tf-app --example residence
# ce que DÉCIDER coûte : le croisement demande × résidence, à l'échelle
cargo run --release -p tf-world --example demande

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
# les ENTITÉS d'un vrai monde, et ce que le moteur en ferait (rotation + miroir)
cargo run --release -p tf-ops --example recenser_entites -- D:\monde
cargo run --release -p tf-ops --example recenser_entites -- D:\monde --zone "4,7,10,12"
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
# Les GESTES de sélection, dans l'ordre tapé, avant l'opération.
# //expand pousse UNE face ; //chunk et --sections sont les DEUX moitiés de ce
# qui donne l'étage palette — l'outil COMPTE les sections entièrement couvertes
# plutôt que d'annoncer « alignée », qui est vrai et ne prouve rien.
cargo run --release -p tf-ops --example editer -- D:\monde-essai --sel "3,-40,5,20,-20,40" --pousser est 5 --chunk --sections --poser minecraft:stone

# le .exe WINDOWS, depuis Linux — pour donner l'outil à quelqu'un qui n'a pas Rust
# (apt install mingw-w64 ; rustup target add x86_64-pc-windows-gnu)
cargo build --release --target x86_64-pc-windows-gnu -p tf-ops --example editer --example semer

# une image, SANS écran (lavapipe suffit : apt install mesa-vulkan-drivers)
cargo run --release -p tf-render --example adaptateur
cargo run --release -p tf-render --example capture -- ../titisite/public/codex vue.png 1000
# le même, sur une VRAIE save — --zone borne au chunk près, parce qu'il
# n'existe aucun état « le monde est chargé »
cargo run --release -p tf-render --example capture -- ../titisite/public/codex vue.png 1400 --monde D:\monde --zone "4,7,10,12" 
# LE DÉCOUPAGE, vu : les chunks comme F3+G, et les .mca que le jeu ne montre
# PAS. Les chunks se teintent par la parité de LEUR .mca : la couleur change à
# la frontière de fichier, donc on voit à quel .mca appartient ce qu'on regarde.
# Le rayon est en CELLULES — 0 = la sienne seulement.
cargo run --release -p tf-render --example capture -- ../titisite/public/codex vue.png 900 --chunks 3 --mca 1
TF_PRES=1 cargo run --release -p tf-render --example capture -- ../titisite/public/codex pres.png

# LA COQUE. Une fenêtre, un vol à la Minecraft (molette ENFONCÉE pour tourner,
# +Maj panoramique, molette roulée pour avancer ; gauche et droit restent aux
# outils), la visée au réticule, la sélection à deux coins, l'accrochage.
cargo run --release -p tf-app -- ../titisite/public/codex
cargo run --release -p tf-app -- ../titisite/public/codex --monde D:\monde --zone "4,7,10,12"
# Elle se dessine aussi dans une TEXTURE — interface comprise — donc elle se
# regarde depuis une machine sans écran, et elle se vérifiera au pixel.
cargo run --release -p tf-app -- ../titisite/public/codex --capture ecran.png --taille 1400x900
cargo run --release -p tf-app -- ../titisite/public/codex --capture concep.png --mode conception
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
               block entities ✅ · entités (`entities/`) ✅ · biomes 1.18+ ✅
  tf-blocks/   règles de transformation DÉRIVÉES du pack ✅ · internement à venir
  tf-world/    adressage ✅ · résidence ✅ · source ✅ · staging ✅ · journal ✅
               (typé, ajout seul, avec paramètres de REJEU) · découpage ✅
               (cellules de chunk et de .mca, `//chunk` et les sections)
               sélection ✅ : deux coins, `//expand`, la FACE qu'on attrape,
               et le POUSSER-TIRER (combien de blocs, et quelle TRANCHE)
               inférence ✅ : accrocher à ce qui est BÂTI, et DIRE à quoi
  tf-ops/      3 étages ✅ · masques ✅ · motifs ✅ · staging+journal ✅
               CATALOGUE ✅ : ce que les opérations disent d'elles-mêmes —
               noms, paramètres, bornes, coût. Un hôte ne réécrit plus rien
               EXÉCUTEUR ✅ : un seul aiguillage, et le REJEU d'une entrée de
               journal sur la copie de travail (annuler / refaire)
               jonction rapport → entrée de journal ✅
               presse-papiers ✅ · rotation/miroir ✅ · block entities ✅
               ENTITÉS ✅ : cadres, tableaux, bêtes suivent copie, rotation,
               déplacement ; nouveaux UUID pour une copie, les mêmes pour un
               déplacement ; ce qui ne se transforme pas est NOMMÉ
               //move ✅ · //stack ✅ · formes ✅ · portée `Colonne` ✅
               //naturalize ✅ · //setbiome ✅ · //smooth ✅ · //hollow ✅
  tf-formats/  .schem · .schematic · .litematic · .nbt
  tf-assets/   packs ✅ · modèles ✅ · textures ✅ · atlas ✅ · .jar + launcher ✅
               couleurs de biome DÉRIVÉES du jeu ✅
  tf-mesh/     glouton ✅ · modèles ✅ · instances ✅ · teinte par BIOME ✅
               (gloutonne ET modèles) · AO, LOD à venir
  tf-render/   wgpu : arène ✅ · hors écran ✅ · modèles ✅ · teinte ✅ · indirect, HZB
               pilotage ✅ : le JOUEUR est le point fixe (pur, sans écran)
               viser ✅ : quel bloc, quelle FACE sous le curseur
               quadrillage ✅ : un calque de lignes, chunks et .mca
  tf-app/      coque winit + egui ✅ (fenêtre, vol, visée, sélection,
               accrochage, quadrillage) · formulaires ENGENDRÉS depuis les
               descripteurs ✅ · FIL MOTEUR ✅ (l'interface ne bloque jamais)
               · SÉLECTION À LA SOURIS ✅ (gauche = coin 1, droit = coin 2)
               · POUSSER-TIRER ✅ en mode Conception, qui S'ACCROCHE
                 à ce qui est bâti et DIT à quoi il tient
               · APPLIQUER / ANNULER / REFAIRE ✅ sur une copie de travail
               · ÉCRIRE DANS LA SAVE ✅ (refus, sauvegarde, écriture)
               · OUTILS de Conception ✅ (tirer / poser / casser)
               · FORMES, comptage et graine réglables ✅
               · REMAILLAGE INCRÉMENTAL ✅ (× 7 sur 64 chunks), et aucune
                 édition ne recharge la zone — un COMPTEUR le tient
               · FIL DE CHARGEMENT ✅ (`chargeur.rs`) : une lecture par
                 RÉGION, une réponse par CELLULE, la plus urgente d'abord
               · INTÉGRATION ✅ (`Ouvert::integrer`) : par LOT, tables d'états
                 fusionnées, et la scène streamée est identique à celle qu'un
                 chargement d'un bloc donne
               · MÉMOIRE BORNÉE ✅ (`Ouvert::peser`) : la résidence sert de
                 COMPTABLE, pas de magasin — une cellule pèse ses sections,
                 son `Vec<Quad>` et sa copie packée, et ce que la fenêtre
                 compte est ce que la scène porte, à l'octet près
               · CAMÉRA QUI PILOTE ✅ (`pilote.rs`, `--rayon`) : demande,
                 charge, pose et lâche à chaque image. Dans la bibliothèque
                 et non dans la fenêtre, pour qu'un test fasse VOLER une
                 caméra sans serveur graphique
               · composants et saisie chiffrée à écrire.
               Elle sait se dessiner dans une TEXTURE (`--capture`) : ce
               n'est pas un mode dégradé, c'est ce qui la rend vérifiable
  tf-bench/    criterion + générateurs de fixtures  ✅ phase 0
```

`proto/` est le prototype de performance. Il ne fait pas partie du produit :
il ne prouve que des chiffres, et disparaîtra quand `tf-anvil` et `tf-ops`
couvriront le même terrain.

## Où ajouter quoi

| Ajouter… | …dans |
|---|---|
| **Une opération** | son entrée dans `OPS` (`tf-ops/src/catalogue.rs`) — nom, noms WorldEdit, paramètres, bornes, coût — son bras dans `construire`, son bras dans `executer` (`executer.rs`), puis le calcul. **Aucune ligne d'interface** : le formulaire se génère depuis le descripteur, et trois tests l'exigent (chaque descripteur se construit ; chaque paramètre se dessine ; chaque opération s'EXÉCUTE vraiment sur un monde) |
| Un PARAMÈTRE à une opération | le `Param` dans son descripteur, et sa lecture dans le bras de `construire`. Le normaliseur n'a rien à savoir : il recopie ce que le descripteur déclare. C'est le piège `seed` d'`ExeWorldEdit`, fermé par construction |
| Un GENRE de paramètre | une variante de `Saisie`, son rang dans `Saisie::rang` (exhaustif, donc le compilateur l'exige), son entrée dans `TOUTES`, et son bras dans `interface::champ`. Un test exige un champ pour chaque variante — sans quoi le paramètre retombe muet, comme `blocklist` dans `ExeWorldEdit` |
| Un état de bloc à transformer | rien : les règles sont DÉRIVÉES du pack (`tf-blocks`). Ce qu'elles ne savent pas faire est NOMMÉ, jamais deviné |
| Un plafond réglable | les bornes du `Param`, jamais une constante dans l'interface. Le champ se génère depuis elles et le serrage s'y réfère |
| Le nom d'une direction | `Direction::nom` (`tf-world/src/selection.rs`) — une seule table, et c'est sur Z que la seconde se tromperait |
| Une capacité de la coque | `etat.rs` si ça DÉCIDE (pur, testable sans écran), `interface.rs` si ça dessine. L'interface ne décide rien |
| Une FORME (sphère, cylindre…) | `Volume` (`tf-ops/src/forme.rs`) — sa variante, son rang dans `rang` (exhaustif), son entrée dans `TOUS`, son cas dans `forme()`, puis son bras dans `interface::volume`. Les deux hôtes la voient aussitôt |
| Un OUTIL de Conception | `Outil` (`tf-app/src/etat.rs`) : sa variante, son `nom`, sa `legende`, et son cas dans le `match` du clic (`coque.rs`). Un test exige que chaque outil dise ce que font les DEUX boutons |
| Un tampon que la scène GPU TIENT | un `Tampon` dans `Scene` (`tf-render/src/scene.rs`), rempli par `Scene::envoyer` depuis les plages sales de l'arène — jamais une reconstruction. Un tampon lu par `arrayLength` se lie à sa taille EXACTE, et sa liaison se refait quand son nombre change, pas seulement quand il grandit |
| Une chose que la scène TIENT en mémoire | son terme dans `Ouvert::peser` (`tf-app/src/scene.rs`) ET dans `octets_residents`, jamais dans l'un seul : c'est leur ÉGALITÉ qu'un test vérifie à l'octet près, et une comptabilité qui ne se compare à rien est une comptabilité qu'on peut tenir en se trompant |
| Une case dont une ENTITÉ se souvient (lit, ruche, laisse…) | les tables de `tf-anvil/src/mobiles.rs` (`TRIPLETS`, `COMPOUNDS_CASE`, `TABLEAUX_CASE`) — par son NOM et sa forme exacte, jamais devinée sur l'allure d'un `{X, Y, Z}` — et un cas dans `tf-ops/tests/mobiles.rs`. Elle ne suivra que si le bloc qu'elle désigne suit |
| Un type d'entité ORIENTÉ (`Facing` qu'on sait lire) | `genre` dans `tf-ops/src/mobiles.rs`. Sans ça, son orientation reste telle quelle et elle est annoncée APPROCHÉE — jamais devinée |
| Un piège rencontré | ici, en disant ce qu'il a COÛTÉ et comment on l'a mesuré |

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

- **Le point fixe d'une caméra d'éditeur Minecraft est le JOUEUR, pas le
  build.** J'avais écrit l'orbite de la CAO : un pivot posé sur ce qu'on
  construit, l'œil qui tourne autour. C'est juste pour SketchUp et faux ici —
  la main de quiconque construit vient de Minecraft, et une habitude de jeu ne
  se rééduque pas, elle se sert. Tout le morceau difficile est parti avec
  l'orbite : il fallait DÉCIDER d'un pivot à chaque bascule de mode, le
  projeter sur le rayon du regard (posé tel quel à quarante blocs de l'axe, il
  faisait sauter l'œil d'autant), et garantir qu'un aller-retour ne dérive pas.
  Avec le joueur comme point fixe, il n'y a plus rien à convertir. **Une
  correction qui SUPPRIME la partie qu'on avait eu le plus de mal à rendre
  juste est le signe qu'on résolvait le mauvais problème.**
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

- **Poser et casser ne visent pas la même case — et le rendre à l'appelant
  serait le faire refaire à chacun.** Un rayon touche une FACE, donc un plan
  ENTRE deux cases : `case` est celle qu'on casse, `avant` celle où l'on
  pose, et `avant` est la voisine par la face traversée. Laisser chaque outil
  refaire le pas de son côté donne un outil qui pose un bloc DANS le mur une
  fois sur deux. Piège hérité d'`ExeWorldEdit`, fermé d'un cran plus bas :
  `viser` rend les deux. Corollaire : la face traversée est celle du côté
  d'où l'on VIENT — en avançant vers +X on entre par −X. L'inverser fait
  poser de l'autre côté du mur, ce qui se lit « l'outil vise à côté ».
- **Un test qui ne mesure que le RÉSULTAT laisse passer une garde entière.**
  La mutation « retirer la garde contre une direction dégénérée » passait
  tous mes tests : sans elle, `INFINITY > portee` finit par couper, ou le
  garde-fou de pas. Le résultat était `None` des deux côtés. Ce que la garde
  achète n'est pas le résultat, c'est le COÛT — `arrete` est une lecture du
  monde, qui décode des chunks, et une origine `NaN` la fait appeler seize
  mille fois PAR IMAGE sans qu'aucune comparaison ne soit jamais vraie. Le
  test qui manquait COMPTE les appels. Quand une mutation survit, c'est
  souvent que le test mesure la mauvaise chose, pas qu'il en manque un.
- **Une marche de voxels a besoin d'un garde-fou en NOMBRE DE PAS, pas
  seulement d'une portée.** Une direction minuscule fait des pas
  infinitésimaux : la portée seule n'arrête la boucle qu'après des millions
  d'itérations. Et une boucle qui ne finit pas fige la fenêtre sans message —
  personne ne sait dire pourquoi, et il n'y a rien à lire dans un journal.

- **L'espace de la CAMÉRA est en blocs, les tables de géométrie en
  seizièmes.** Les deux shaders divisent par seize juste avant la matrice de
  vue. En branchant le quadrillage j'ai converti par symétrie avec les tables
  — il sortait SEIZE FOIS trop grand, et l'image restait parfaitement
  plausible : un quadrillage large est exactement ce à quoi ressemble un
  quadrillage. Chaque moitié était juste, c'est leur jonction qui ne l'était
  pas — le piège `toHeights` / `applyHeightmap`, sous une autre forme. Le
  test qui tranche TRAVERSE : le contour d'une section doit encadrer les
  pixels de cette section, aux mêmes pixels près.
- **Trouvé par une mutation qui visait autre chose.** Je cassais le test de
  profondeur du calque ; il ne changeait rien, parce que le calque n'était pas
  là où je croyais. Une mutation qui survit sans raison est un indice sur le
  CODE, pas seulement sur le test.
- **Un seuil choisi à la main ne prouve pas une occultation.** « Plus de vingt
  pixels verts » était vert dans les deux sens : mesuré, un test de profondeur
  actif laisse quand même passer 49 pixels sur 111 — les arêtes que la
  silhouette ne couvre pas. La propriété juste est une ÉGALITÉ contre un
  TÉMOIN : le même calque sans rien devant. Un nombre seuil est une opinion ;
  un témoin est une mesure.
- **Le fond d'une image se MESURE, il ne se recopie pas.** La couleur
  d'effacement est donnée en linéaire et la cible est en sRGB : les octets
  relus ne sont pas ceux de la constante, ils sont son encodage. Recopiée à la
  main, elle donne un prédicat « pas le fond » qui accepte tout, donc une
  emprise qui couvre l'image — et un test qui échoue en accusant la mauvaise
  moitié.
- **L'épaisseur d'un trait n'est pas réglable, la couleur si.** wgpu ne
  garantit pas de ligne plus large qu'un pixel, sur aucune plateforme. Ce qui
  sépare le quadrillage de chunk de celui des `.mca` est donc la couleur —
  s'appuyer sur l'épaisseur donnerait un rendu juste sur une carte et plat sur
  une autre, sans erreur nulle part. Corollaire utile : teinter les chunks
  par la parité de LEUR région fait voir la frontière de fichier sans
  dessiner un second quadrillage par-dessus le premier.

- **« Alignée sur les chunks » est vrai et ne prouve rien.** `//chunk`
  n'aligne que x et z — c'est la convention de WorldEdit, et étendre la
  hauteur sans le dire remplirait de la pierre du sol au ciel. Mais une
  sélection de y = −40 à −20 ne couvre AUCUNE section entière : les sections
  vont de −48 à −33 puis de −32 à −17. Mesuré sur un vrai monde, douze
  sections passaient encore par l'étage bloc pendant que l'outil annonçait
  « alignée ». Les deux moitiés sont nécessaires et ce sont deux gestes
  distincts (`--chunk` et `--sections`). Corollaire, et c'est la vraie leçon :
  **ce qui décide l'étage se COMPTE** (`sections_entieres`), il ne se déduit
  pas d'une propriété qui a l'air suffisante. L'outil annonce « 12 / 12 »,
  pas « alignée ».
- **Deux `enum` de directions dans deux crates qui ne peuvent pas se voir.**
  `tf_mesh::forme::Face` sert au mailleur, `tf_world::Direction` à la
  sélection ; ni l'un ni l'autre crate ne dépend de son voisin, et les faire
  dépendre mettrait le mailleur sous l'éditeur ou l'inverse. Ce dépôt a payé
  QUATRE fois le piège des tables qui divergent. La parade n'est pas
  d'espérer : un test de `tf-render` — le seul crate qui voie les deux — les
  croise par ce qu'elles VEULENT DIRE (`pas()`, `axe()`, `opposee()`), jamais
  par leur rang, qui serait une coïncidence d'écriture. `viser` rend une
  `Face` et `agrandir` prend une `Direction` : un désaccord ferait tirer la
  paroi opposée à celle qu'on a visée.
- **Les bornes d'une `BBox` sont INCLUSES, le solide qu'elle décrit ne l'est
  pas.** Un bloc occupe une case d'une unité : le solide va de `min` à
  `max + 1`. Confondre les deux rend la dernière rangée de blocs impossible à
  attraper — le rayon passe juste derrière la paroi — ce qui se lit « le bord
  de la sélection ne répond pas » et ne désigne pas la cause. La conversion se
  fait une fois, dans `BBox::coins` ; recopiée sur place, elle est oubliée une
  fois sur deux.
- **Une face de sélection ne traverse jamais la face opposée.** `//contract`
  au-delà laisserait une boîte retournée que `BBox::new` normaliserait SANS
  RIEN DIRE : la sélection couvrirait brusquement l'autre côté, et
  l'opération suivante écrirait là-bas. On s'arrête à un bloc d'épaisseur,
  comme WorldEdit. Même famille : un `//expand` près de `i32::MAX` ramène la
  face à l'autre bout du monde si l'addition n'est pas faite en `i64`.

- **Sur une grille de blocs, accrocher à la GRILLE ne vaut rien.** C'est la
  moitié du travail en CAO ; ici tout y est déjà. Ce qui manque est
  d'accrocher à ce qui est BÂTI — le nu d'un mur, le coin d'une tour, la
  hauteur de la fenêtre d'à côté. Et les trois accroches de SketchUp sortent
  d'un seul mécanisme si l'on accroche AXE PAR AXE : un axe donne un plan,
  deux une droite, trois le point. Trois implémentations séparées auraient
  divergé, et la « ligne droite depuis le dernier point » n'est qu'un cas de
  la règle générale.
- **Une inférence qui accroche en SILENCE est une inférence qu'on combat.**
  Ce qui rend celle de SketchUp utilisable n'est pas sa précision, c'est
  qu'elle DIT ce qu'elle a attrapé. Sans ça la position saute et on ne sait
  pas si l'on a mal visé ou si l'outil a décidé. `Accroche` rend une raison
  PAR AXE, avec sa référence entière — c'est le trait pointillé, et sans lui
  on ne voit pas à quoi on tient.
- **Un départage à deux critères fait CLIGNOTER l'accrochage.** Deux coins
  symétriques d'une même boîte sont à égale distance et de même genre : sans
  troisième critère, celui qui gagne dépend de l'ordre de la liste, donc
  change d'une image à l'autre. Un clignotement se lit « l'outil est
  instable » et ne désigne rien. L'ordre est (écart, genre, coordonnée), et
  le dernier est TOTAL.
- **L'axe de la face contre laquelle on POSE ne doit pas accrocher.** On vise
  la face Est d'une paroi, on pose donc à `x + 1` — mais la paroi est à un
  bloc, donc dans la tolérance, et l'accrochage ramène le bloc neuf DANS le
  mur qu'on visait. L'accroche était juste, la pose aussi : c'est leur
  COMPOSITION qui ne l'était pas, et seule la jonction pouvait le montrer.
  `Direction::verrou` dit la règle une fois — l'axe de pose est décidé par la
  pose, les deux autres restent libres, et ce sont eux qui portent tout
  l'intérêt puisque c'est dans le PLAN de la face qu'on s'aligne.
- **Le milieu d'une arête porte sur le SOLIDE, pas sur les indices.** Une
  section va de 0 à 15 inclus, donc de 0 à 16 en géométrie : son milieu est
  8, pas 7. Et le milieu d'une arête PAIRE n'existe pas — on prend toujours
  le plus bas, par division plancher. Alterner selon la parité ferait sauter
  l'accroche d'un bloc quand on redimensionne, ce qui se lit « le milieu
  bouge tout seul ».
- **Une face ne s'accroche pas à son PROPRE point de départ.** La position de
  départ est toujours dans la tolérance au premier bloc tiré : sans filtre, la
  face revient se coller là d'où elle part, et un petit déplacement devient
  impossible. Ce n'est pas un alignement, c'est un non-mouvement. Troisième
  forme du même piège après le verrou d'axe de la pose : l'accroche est juste,
  le geste est juste, c'est leur COMPOSITION qui décide.
- **Du code défensif qui ne peut pas se tromper ne dit rien.** J'avais
  verrouillé les deux axes perpendiculaires à la face tirée, « parce qu'une
  face ne bouge que le long de sa normale ». Vrai, et sans effet : on ne relit
  que l'axe de la face, et `accrocher` choisit sa référence axe par axe. La
  mutation qui retirait les verrous n'a fait rougir aucun test — et elle avait
  raison. Quand une mutation survit, la première question est « qu'est-ce que
  cette ligne achète ? », pas « quel test manque ? ».
- **Un pousser-tirer ne se mesure pas en pixels.** Multiplier un glissement
  d'écran par une sensibilité donne un geste qui dérive selon la distance et
  l'angle — personne ne sait corriger ça à l'œil. On projette le rayon de
  souris sur la DROITE de l'axe, et on compte des blocs entiers. Corollaire :
  quand le rayon devient colinéaire à l'axe, la projection part à l'infini et
  un pixel vaut des dizaines de blocs. On refuse BIEN avant zéro — à un degré
  près, c'est déjà ingouvernable — et refuser veut dire ne pas bouger, pas
  bouger n'importe comment.
- **Un geste qui cumule s'emballe.** Le tirage repart de la sélection de
  DÉPART à chaque image, jamais de la courante : additionner les tirages fait
  accélérer la face à mesure qu'on la tire, ce qui se lit « la poignée
  s'emballe » et ne désigne rien.
- **Un tirage n'écrit que sa TRANCHE.** Tirer une face de trois blocs sur un
  bâtiment de cent mille ne doit pas réécrire le bâtiment — c'est l'invariant
  n° 8 appliqué au geste. Et la tranche commence à `ancien_max + 1`, parce que
  les bornes d'une `BBox` sont INCLUSES : un bloc d'écart et la dernière rangée
  de l'ancien volume est écrasée.
- **Une mesure sert aussi à NE PAS écrire du code.** Un état que l'atlas ne
  connaît pas force un rechargement complet de la zone, et j'allais rendre
  l'atlas extensible — une vraie pièce, avec une vraie surface de bug. Mesuré
  d'abord : 31 ms pour une zone de 64 chunks, une fois par type de bloc et par
  séance. Ça ne vaut pas la peine aujourd'hui ; ça la vaudra quand la résidence
  sera pilotée par la caméra. Le chiffre ne dit pas seulement combien, il dit
  QUAND.
- **J'ai optimisé le maillage pendant que la relecture coûtait cent fois
  plus.** Le remaillage incrémental écrit, mesuré : **× 1,1**. La découpe par
  phases a dit pourquoi — 18 ms sur 18,3 dans la RELECTURE, 0,2 dans le
  maillage. `sections_de` inflatait chaque chunk AVANT de filtrer (256 chunks
  décompressés pour en lire quatre) et ignorait la HAUTEUR de la sélection
  (vingt-quatre sections décodées pour une). Corrigé : × 87 sur la lecture
  d'une section, × 7 sur le remaillage complet — et le chargement en profite
  autant. C'est le piège n° 1 du dépôt, payé une troisième fois : **découper
  une chaîne AVANT de choisir quoi accélérer n'est pas une précaution.**
- **Un témoin qui ne voit pas le même monde accuse le mauvais coupable.** Pour
  croiser le remaillage incrémental et le rechargement complet, j'avais ouvert
  un SECOND `Ouvert` sur la même save. Il a sa propre copie de travail, donc il
  lisait le monde d'AVANT l'opération : le test annonçait 4 722 quads contre
  4 840 et accusait le chemin rapide d'une différence qui venait du témoin. Un
  témoin se construit en changeant UNE chose, et rien d'autre.
- **« Déclaré, branché, testé — et inatteignable » ne se referme pas une fois
  pour toutes.** Un audit de fin de phase a trouvé QUATRE trous du même genre,
  tous creusés par le même hôte neuf : la coque câblait `Forme::Boite`,
  `compter: true` et `seed: 0` en dur dans un bouton — donc cinq formes
  perdues, un invariant violé (le comptage coûte × 31, il ne se rend jamais
  d'office) et tous les mélanges identiques d'un projet à l'autre — et
  `point_de_pose`, l'accrochage à la pose, n'était appelé que par ses propres
  tests. Le piège se rouvre à chaque hôte qui n'expose pas ce que le moteur
  offre. Remède : construire la commande dans l'ÉTAT (testable), jamais au
  milieu d'un bouton que personne ne teste.
- **`cargo build -p A -p B --example X` ne construit que l'exemple.** Le
  `--example` s'applique à l'invocation entière : le binaire de `A` reste
  celui d'avant, cargo dit « Finished » en cinq secondes, et la capture montre
  l'interface de la version précédente. J'ai cherché pourquoi un panneau
  n'apparaissait pas dans du code qui le dessinait bien. Une commande qui
  réussit en ne faisant pas ce qu'on croit est pire qu'une qui échoue.
- **Un coin de sélection ne s'accroche PAS.** L'inférence sert à poser un bloc
  au nu d'un mur ; appliquée à un coin, elle sélectionnerait autre chose que ce
  qu'on a visé — et le bord d'une paroi, qui est justement ce qu'on vise le
  plus souvent, deviendrait inattrapable. Même famille que le verrou d'axe :
  l'accroche est juste, la pose est juste, c'est leur COMPOSITION qui décide.
  Corollaire : un coin se pose sur la case VISÉE, jamais sur celle d'avant —
  confondre les deux décale toute sélection d'un bloc vers l'observateur.
- **Remailler depuis la SOURCE au lieu de la copie de travail.** C'est la faute
  qui se lit « le bouton ne fait rien » : l'opération réussit, le journal la
  garde, et l'écran montre le monde d'avant. La coque relit donc le `Staging`,
  jamais ce qu'il recouvre. Vérifié par mutation — remplacer la copie de
  travail par sa source fait rougir le test de jonction, et rien d'autre.
- **Deux mondes ouverts partageaient la même copie de travail.** Le dossier
  temporaire portait le seul identifiant de PROCESSUS : deux `Ouvert` dans le
  même programme écrivaient donc dans la même couche, et le second passait par
  -dessus les régions du premier. Trouvé par deux tests qui tournaient en
  parallèle ; un utilisateur qui ouvre deux fenêtres l'aurait trouvé autrement,
  et beaucoup plus tard. Un nom « unique » qui ne l'est qu'à l'échelle du
  processus n'est pas unique.
- **Un fil qui écrit et un fil qui lit n'ont pas besoin d'un verrou** quand un
  seul écrit. `Staging` prend `&self` partout : un `Arc` suffit, et donner la
  possession exclusive au moteur aurait obligé à faire revenir les octets par
  le canal — c'est-à-dire à recopier une région entière à chaque coup de
  pinceau.
- **Un test qui PEND ne dit pas ce qui ne va pas.** En cassant `recevoir` pour
  qu'il bloque — la faute même que le fil moteur existe pour empêcher — la
  suite ne rendait plus la main du tout : ni rouge, ni vert, rien à lire. Une
  propriété qui se formule « ça ne doit pas attendre » se vérifie donc dans un
  fil TÉMOIN, avec une attente bornée, sinon le seul symptôme est l'absence de
  symptôme.
- **Un texte d'aide faux se lit exactement comme un texte d'aide juste.**
  `editer` listait ses dix opérations à la main ; la liste avait déjà vieilli,
  et rien ne pouvait le dire — ce n'est pas du code, donc aucun compilateur, et
  ce n'est pas une sortie vérifiée, donc aucun test. Elle vient maintenant du
  catalogue, bornes et défauts compris : c'est la même donnée que le formulaire
  de la coque, donc les deux ne peuvent plus se contredire.
- **Un `{:?}` dans un texte destiné à un humain montre du Rust.** L'aide
  engendrée affichait `Texte("minecraft:stone")` et `Direction(PlusX)` à
  quelqu'un qui tape une commande. `Display` sur `Valeur` est la seule table, et
  un test refuse tout défaut du catalogue qui s'afficherait avec une parenthèse
  ou un guillemet.
- **Une jonction que personne n'écrit, deuxième fois — et je l'ai vue écrite
  DANS UN TEST.** Rejouer une entrée de journal sur la copie de travail
  n'existait nulle part : le test d'édition le refaisait à la main, sous le
  commentaire « c'est exactement ce que l'application fera ». C'est la
  définition du piège, et cette fois avec un aveu écrit à côté. Chaque hôte
  l'aurait réinventé, avec la faute que ce dépôt a déjà payée — les correctifs
  s'annulent À L'ENVERS. `edition::rejouer` le fait une fois.
- **Une mutation peut survivre parce que la FIXTURE ne sépare pas les deux
  cas.** Rejouer une annulation dans le sens d'enregistrement passait tous les
  tests : tant qu'une opération ne touche chaque chunk qu'une fois, `a_annuler`
  et `a_refaire` rendent le MÊME ensemble et seul l'ordre change. Il a fallu un
  `//move` dont la source et la destination se chevauchent. Même leçon que les
  coffres aux `y` croissants, un cran plus haut : quand une mutation survit,
  c'est souvent que le motif de test ne distingue pas ce qu'on croit tester.
- **Un identifiant DÉDUIT de la forme de l'objet n'est pas un identifiant.**
  `Travail::id()` devait se lire dans le plan — masque `Tout` et motif
  `Melange` pour un mélange, et ainsi de suite. Ça paraissait plus sûr que de
  recopier une chaîne, puisque rien ne pouvait diverger. C'est faux :
  `Motif::melange` d'une liste VIDE rend `Garder` et d'une seule entrée rend
  `Bloc` — deux simplifications justes et voulues — donc un « mélange » se
  relisait « remplir » selon ce que l'utilisateur avait tapé dans le
  formulaire. Un identifiant qui change avec le contenu d'un champ ne peut pas
  servir de clé de journal. Il voyage donc AVEC le travail, et ce qu'un test
  regarde pour les opérations directes, c'est le masque et le motif — là où la
  faute serait vraiment.
- **Un nom WorldEdit n'est pas une clé.** `//set` désigne aussi bien « remplir »
  que « mélange » : dans le jeu c'est une seule commande à un motif près. Une
  recherche en UNE passe (`d.id == nom || d.we.contains(nom)`) ferait dépendre
  la réponse de l'ordre du tableau, et un identifiant finirait par tomber sur
  le nom WorldEdit d'une autre opération. L'identifiant d'abord, TOUJOURS ; les
  noms WorldEdit servent à CHERCHER, et la palette rend tous les candidats
  plutôt que d'en choisir un à la place de l'utilisateur.
- **Le coût d'une sélection n'appartient pas à la sélection.** Le compte de
  sections entières était affiché dans le panneau SÉLECTION *et* dans celui de
  l'opération : le même chiffre deux fois, donc deux chiffres différents le
  jour où ils divergent. Et ils divergent déjà — une opération à portée
  `Colonne` ne verra jamais l'étage palette, quelle que soit la sélection.
  L'alignement est une propriété de la sélection ; ce qu'il COÛTERA est une
  propriété de l'opération.
- **Un test qui cherche un mot dans une phrase trouve aussi sa négation.**
  J'exigeais que le verdict d'une opération par colonne ne contienne pas
  « palette » — or il dit « ni étage palette », qui le contient. Le test
  rougissait sur du code juste, ce qui envoie chercher un bug là où il n'y en a
  pas. Une assertion sur un texte porte sur ce que la phrase AFFIRME, pas sur
  ses lettres : ici « le compte de sections ne doit pas apparaître ».
- **Une assertion toujours vraie par le TYPE ne prouve rien**, et clippy le
  dit. `r.point.x >= i32::MIN` ne peut pas échouer ; ce qui pouvait déborder
  était le MILIEU du domaine entier, où `MIN + MAX + 1` s'enroule en `i32`.
  Un test qui borne ce que le type borne déjà donne un vert gratuit.

- **Le format d'une surface se NÉGOCIE, il ne se choisit pas.** Le rendu hors
  écran fabrique sa cible, donc il impose son format ; une fenêtre reçoit le
  sien du pilote. En forçant `Rgba8UnormSrgb` — celui de toutes les captures du
  dépôt — la première ouverture est morte sur
  « *not in list of supported formats: [Bgra8UnormSrgb, Bgra8Unorm]* ». Le
  format traverse donc la `Scene` jusqu'aux pipelines, et on prend le premier
  qui soit **sRGB** : la conversion que les shaders attendent est une propriété
  de la cible, pas une préférence. Corollaire : la passe de rendu est écrite
  UNE fois et sert les deux chemins (`rendre` et `dessiner_sur`), sinon la
  fenêtre et la capture divergeraient — et seule la capture est testée.
- **Les couleurs SATURÉES sont des points fixes de la conversion sRGB.** Le
  calque de lignes envoyait ses couleurs telles quelles à une cible sRGB, qui
  les réencode : elles passaient deux fois dans la courbe et sortaient
  délavées. Le test de couleur existant ne l'a pas vu parce qu'il n'employait
  que du rouge et du vert PURS — 0 et 255 se transforment en eux-mêmes, dans
  les deux sens et dans les deux espaces. Mesuré sur une demi-teinte :
  (120, 220, 140) ressortait en (184, 240, 196), qui se lit « blanc ». Un test
  de couleur qui n'emploie que des primaires ne teste pas la couleur. Même
  famille que les couleurs de sommet d'`ExeWorldEdit`, et la troisième fois que
  l'espace de couleur se venge dans ce dépôt.
- **egui met sa fonte dans le PREMIER `textures_delta`, et nulle part
  ailleurs.** La capture fait tourner l'interface deux fois — une pour que les
  panneaux se mesurent, une pour dessiner — et je n'appliquais que les deltas
  de la seconde. Résultat : une interface complète, correctement disposée, et
  **pas un seul caractère**. Aucune erreur, aucune validation en défaut : la
  texture manquante est simplement vide. Les deltas des deux passes
  s'enchaînent.
- **Le même défaut avait un ÉTAGE de plus.** L'atlas réparé, j'ai remesuré : une
  édition de trois blocs coûtait encore **80 ms** sur une région bâtie de 256
  chunks, dont **52 dans l'arène des quads** et 19 dans la passe de modèles —
  toutes deux rebâties en entier à chaque coup de pinceau. Du O(scène) pour un
  geste en O(édition), la troisième forme du `warmup(extent)` dans la même
  séance. `Arene::remplacer` refait les tranches visées et RECOPIE les autres :
  80 → 39 ms. Puis la passe de MODÈLES est devenue le poste dominant à son
  tour (23 à 29 ms des 35), et elle a un champ de plus à tenir juste :
  `debut_face` est une somme PRÉFIXE sur tout le flot, que le shader
  dichotomise, donc une tranche qui change de longueur décale toutes les
  suivantes. Retrouver le nombre de faces d'une pose recopiée demande la mémo
  `(état, biome) → géométrie`, qui vit maintenant DANS l'arène plutôt que dans
  une locale — sans elle, refaire une tranche rappellerait `modele` et ferait
  grossir la table de faces d'une copie à chaque édition, sans fin.
  **80 → 27 ms en tout.** Ce qui reste est de la bande passante : recopier
  276 k instances et 178 k poses. Pour descendre sous les 8 ms il faudra des
  tampons GPU par SECTION au lieu d'un tableau à plat — c'est la marche
  suivante, et c'est celle dont la résidence a besoin de toute façon.
  **Corollaire : réparer le poste dominant ne clôt pas le sujet, il DÉSIGNE le
  suivant.** Quatre fois de suite dans ce dépôt.
- **Découper une fonction en deux et appeler quand même l'ancienne.** Premier
  jet du remplacement d'arène : gain **zéro**, parce que le chemin incrémental
  appelait toujours `arenes()` pour en tirer la passe de modèles — donc
  rebâtissait l'arène des quads pour la jeter aussitôt. Le chiffre l'a dit
  avant que je l'annonce, et c'est tout l'intérêt de mesurer APRÈS comme
  avant : une optimisation qui ne gagne rien se lit exactement comme une
  optimisation qui gagne, tant qu'on n'a pas relu le chronomètre.
- **Un numéro de couche d'atlas est un système de coordonnées.** Le croisement
  « l'arène remplacée est-elle celle qu'un rechargement donnerait ? » a échoué
  sur 276 328 instances sur 276 328, et uniquement sur le champ `couche`.
  Aucun bug : un atlas ÉTENDU ajoute ses couches à la fin, un atlas REBÂTI les
  range par nom. Les deux sont justes, chacun avec le sien. C'est le piège du
  `StateId` relatif à SON interner, sous une troisième forme — et le remède est
  le même : *ce qui traverse les deux se compare par NOM*. Un test qui compare
  des octets entre deux mondes indexés différemment accuse toujours le mauvais
  coupable. Le piège s'est refermé QUATRE fois sur le même test avant qu'il
  soit juste : `InstanceQuad::couche`, puis `FaceModele::couche`, puis
  `Pose::debut_modele` — qui indexe une table de faces dont l'ordre de
  remplissage diffère — et enfin le nombre de faces d'une pose, qui ne se
  devine pas mais se DÉDUIT de la somme préfixe. Chaque fois, le test
  rougissait sur du code juste.
- **Une prémisse de test qui ne tient pas rend le test muet.** Deux mutations
  du remplacement d'arène survivaient — l'indice de lot non corrigé à la copie,
  les origines non refaites. J'ai écrit le test qui manquait côté application :
  sa prémisse (« un lot doit apparaître ») était FAUSSE, parce que le monde
  porte déjà toutes ses sections et que notre écrivain n'en supprime jamais.
  Le cas ne peut donc pas se produire dans la coque aujourd'hui — il se
  produira à chaque pas de caméra dès que la résidence sera pilotée — et il se
  teste au niveau où il existe (`tf-render/tests/arene.rs`), pas au niveau où
  on aimerait qu'il existe.
- **Une assertion de performance écrite en TEMPS tombe quand la suite tourne.**
  « Une édition ne paie pas la zone » était un rapport de durées entre deux
  tailles de zone, seuil 3. Seul, le test passait (rapport 2,5) ; dans la suite
  complète, en parallèle, il tombait — les temps absolus dérivent avec la
  charge, ce que ce dépôt a mesuré à un facteur **2,4 à code identique**. Un
  test rouge une fois sur trois fait douter du CODE au lieu du test, ce qui est
  pire que pas de test du tout. La propriété se COMPTE : `sections_remaillees`
  vaut **1 sur une zone de 16 chunks comme sur une de 256**, contre 384 et 6 144
  au rechargement. Exact, machine-indépendant, et ça dit la chose même qu'on
  veut. Troisième compteur de la séance après `rechargements` et les lectures
  du chargeur — à ce stade c'est une règle : *ce qui doit rester vrai se
  compte.*
- **Le grain d'intégration est décidé par le COÛT FIXE, pas par l'API.**
  `Ouvert::integrer` prend un LOT de cellules, pas une. Le remplacement de
  tranches recopie les deux arènes, donc son coût est en O(scène) QUEL QUE SOIT
  le nombre de cellules intégrées : une par une, on paie cette recopie N fois.
  Mesuré en streamant 197 cellules sur du bâti (805 k quads) : **4 577 ms une
  par une contre 826 par lot, × 5,5** — et pire, le coût unitaire EMPIRE à
  mesure que la scène grandit (médiane 23 ms, pire 84). C'est la même leçon que
  l'unité de LECTURE du chargeur, à l'autre bout de la chaîne. Ce qui reste
  sous-jacent est toujours la recopie O(scène), que seuls des tampons GPU par
  SECTION élimineront.
- **Une source qui COMPTE ses lectures dit ce qu'un chronomètre ne sait pas.**
  « Un `.mca` n'est lu qu'une fois par lot » est la conclusion qui a décidé
  l'unité de lecture du chargeur (× 10 entre un chunk seul et un chunk amorti,
  soit 4,4 s gaspillées par région). Mesurée au temps, elle dépendrait de la
  charge de la machine ; comptée, elle est exacte et la mutation « une lecture
  par cellule » meurt en une ligne. Un compteur est une preuve, un seuil de
  temps une opinion — même leçon que `Ouvert::rechargements`.
- **L'emprise d'un lot est un RECTANGLE, la demande un DISQUE.** Le chargeur
  lit chaque `.mca` une fois, sur la boîte qui couvre les cellules voulues de
  cette région — mais un lot au bord de l'horizon n'occupe qu'un coin de son
  rectangle. Mesuré (`tf-world --example demande`) : **31 % de chunks décodés
  en trop à rayon 8, 12 % à rayon 64** — la part baisse quand le disque
  grandit, parce que les régions centrales y sont entièrement dedans. C'est
  réel, c'est borné, et ce n'est pas le prochain combat : ces chunks-là sont à
  une cellule de l'horizon, donc le premier pas de caméra les réclame. Écrit
  ici pour que personne n'ait à le redécouvrir en le soupçonnant.
- **Un rechargement de ZONE derrière le geste le plus banal de l'éditeur.**
  Le remaillage est incrémental, mais il gardait une porte vers le chemin
  complet : un état de bloc que l'atlas ne connaît pas. Or prendre un bloc dans
  la palette et le poser est exactement ce qu'un éditeur sert à faire, donc
  cette porte s'ouvrait en permanence. Mesuré sur une zone de 64 chunks : poser
  un bloc connu coûte 0,7 ms, poser un bloc NEUF en coûtait **22,3** — × 33. Et
  le coût est en O(ZONE) : 6,6 ms sur 16 chunks contre **85,7 sur 256**, donc
  867 ms sur une région bâtie. C'est le `warmup(extent)` d'`ExeWorldEdit` sous
  un autre nom — 5,2 s pour une sphère de 62 blocs, dont 47 % à décoder des
  chunks jamais lus. L'atlas s'ÉTEND maintenant (`Atlas::etendre`), et la table
  comme l'habillage se prolongent : les indices déjà attribués ne bougent pas,
  donc rien de ce qui est maillé ne change de sens. 22,3 ms → 0,6. La défense
  durable n'est pas le correctif mais le COMPTEUR : `Ouvert::rechargements` se
  lit dans un test, là où un seuil de temps n'aurait été qu'une opinion.
- **Un test qui ne tourne pas ne dit rien.** Neuf tests de la coque — ceux qui
  tiennent la JONCTION entre la coque, le moteur et le rendu — étaient gated
  sur un vrai pack (`TF_PACK`), donc ne tournaient jamais ailleurs que sur la
  machine qui a le serveur. Le codex du site se reconnaît à UN fichier
  (`blockstates.json` à la racine) : on en écrit donc un à la volée, modèles et
  PNG compris, comme les régions et pour la même raison — pas de fixture
  binaire dans le dépôt. Corollaire qui a mordu tout de suite : une fixture qui
  ne couvre pas ce que l'AUTRE fixture écrit est un test vert qui ne mesure
  rien. Le premier codex ne déclarait que des `minecraft:*` alors que `Build`
  pose 221 `minefield:*` : une région bâtie de 256 chunks se chargeait en
  **six quads**, et la mesure prise dessus annonçait 0,1 ms là où la vraie
  valeur est 85.
- **Une fonction publique ne doit pas dépendre en silence de l'ordre de son
  entrée.** `par_region` groupe une demande en lectures de région ; son tri des
  lots ne décidait rien, parce que `voulues` rend déjà une liste triée et
  qu'aucun test ne lui donnait autre chose. La mutation qui le retirait
  survivait donc. En écrivant le test qui manquait — une demande à l'envers —
  il a échoué sur le code INTACT : `Lot::urgence` lisait la PREMIÈRE cellule du
  lot, ce qui n'est la plus urgente que si l'entrée était triée, donc les lots
  se triaient sur une clé fausse. Deux leçons, et la seconde vaut la première :
  une mutation qui survit dit parfois qu'il manque un test, et ce test-là
  trouve alors un vrai bug ailleurs. La fonction est maintenant TOTALE — les
  cellules d'un lot sont triées, donc `first` est exact par construction — et
  trier une tranche déjà triée ne coûte presque rien, le tri détectant les
  suites ordonnées.
- **L'APPARTENANCE et l'URGENCE ne se mesurent pas de la même façon.** La
  demande de la caméra découpe un disque de cellules autour de l'œil et les
  classe par urgence. J'avais fait les deux avec la même mesure — la distance
  de l'œil au centre de la cellule — et le premier test l'a démolie : avec un
  rayon de 0 et un œil près d'un coin, la cellule où l'on SE TIENT est à 0,707
  de son centre, donc jetée de sa propre demande. Et la conséquence générale
  est pire que le cas limite : découper sur la position EXACTE fait glisser
  l'ensemble demandé pendant qu'on marche à l'intérieur d'une cellule, donc on
  charge et on jette en continu — exactement le va-et-vient que le disque
  existe pour empêcher. L'appartenance se décide sur la grille de cellules
  (elle ne change qu'en franchissant une frontière), l'urgence sur la position
  réelle. Corollaire mesuré : un disque au lieu d'un carré, c'est 30 % de
  cellules en moins À HORIZON ÉGAL, et surtout un ensemble invariant par
  rotation — tourner sur place ne fait plus rien entrer ni sortir.
- **Un tri STABLE rend un départage inopérant, donc invérifiable.** Le
  comparateur de la demande départage les ex æquo sur `(z, x)`. La mutation qui
  retirait cette ligne survivait : sur une trentaine d'éléments le tri retombe
  sur une insertion, qui est stable, et les ex æquo gardaient l'ordre
  d'émission de `cellules_autour` — lequel est justement `(z, x)`. La ligne ne
  décidait donc rien, et l'ordre « garanti » reposait en fait sur un détail
  d'un AUTRE module, du genre qui se casse le jour où ce module change sans que
  rien ne le dise. Deux corrections, et il fallait les deux : tri INSTABLE pour
  que le comparateur décide seul, et le test porté à quelques milliers de
  cellules pour sortir du tri par insertion. Avant ça, « l'ordre est total »
  était une opinion.
- **Filtrer APRÈS avoir itéré rend quadratique — troisième fois.** `planifier`
  croisait la demande et la résidence avec un `contains` sur deux tranches.
  Mesuré : 0,04 ms à rayon 8, et **11,4 ms à rayon 40**, c'est-à-dire tout le
  budget d'image dépassé pour décider quatre-vingts chargements — et 40 chunks
  est une distance d'affichage ordinaire, pas un cas limite. Par ensembles :
  0,60 ms, × 19. Les ensembles ne servent QUE à l'appartenance ; l'ordre des
  deux listes vient des tranches d'entrée, sinon le hasard d'un `HashSet`
  donnerait deux chargements différents de la même scène.
- **La date d'un DOSSIER ne bouge pas quand on réécrit ce qu'il contient.**
  Une copie de travail qu'un `kill` laisse derrière ne se voit pas — elle vit
  dans le dossier temporaire — et elle pèse ce que pèsent les régions éditées :
  mesuré après une séance, dix-sept mégaoctets en dix-sept dossiers. Le
  balayage se fie donc à l'ÂGE, faute de savoir détecter un processus vivant de
  façon portable (la limite de `session.lock`, déjà payée). Sauf que j'avais
  écrit, dans le commentaire qui justifiait le seuil de 24 h, qu'« une édition
  réécrit des régions dans la couche, donc rafraîchit sa date ». Mesuré, c'est
  FAUX dans les deux sens : réécrire `region/r.0.0.mca` ne change ni la date de
  la racine, ni celle de `region/` — un dossier ne voit passer que les
  créations et les suppressions d'ENTRÉES. Une séance ouverte depuis plus d'un
  jour et toujours en train d'éditer se serait donc fait effacer sa copie de
  travail par n'importe quelle autre instance, c'est-à-dire toutes ses
  opérations non écrites, le seul endroit du programme où elles existent.
  L'âge se mesure sur le fichier le plus RÉCENT de l'arborescence. Corollaire
  sur les tests : la fixture ne datait que le dossier, donc elle verrouillait
  le critère cassé — deux mutations (la date de la racine, la date la plus
  ancienne) passaient au vert. **Un raisonnement écrit dans un commentaire n'a
  pas été mesuré** : celui-ci était faux le jour où je l'ai écrit.
- **Un disque plein sort par le compilateur, pas par le programme.**
  `LLVM ERROR: IO failure on output stream: No space left on device` en plein
  `cargo build`. La cause n'était ni le code ni la machine : `target/debug`
  pesait 26 Go, dont **8,4 pour le seul `incremental`**, qui est un cache pur et
  se régénère à la demande. Un `target/` qu'on ne vide jamais finit par coûter
  la séance ; l'effacer est gratuit, et la première chose à faire avant de
  chercher plus loin.
- **Des coordonnées de démonstration écrites en dur mentent poliment.** Le
  contour de sélection de la capture était cadré à y = 64..97, une hauteur de
  surface vanilla ; la fixture de BUILD vit autour de y = −10. Le contour était
  donc HORS CHAMP, et l'image restait une image parfaitement plausible — c'est
  « une absence ne se voit pas », appliqué à ce qui sert à montrer que ça
  marche. Ce qu'une capture met en scène se DÉRIVE des bornes de ce qu'elle
  vient de charger.
- **Un budget qu'on estime AVANT de mesurer invente un facteur.** Le poids
  d'une cellule résidente, c'est ses sections plus son maillage — et le
  rapport entre les deux vaut 0,022 sur du terrain contre 1,18 sur du bâti,
  soit un facteur **54** entre les deux fixtures (`--example residence`).
  L'estimer à la pose, avant le maillage, aurait donc voulu dire choisir une
  des deux et se tromper d'un ordre de grandeur sur l'autre — c'est-à-dire sur
  Minefield, qui est du bâti. La pesée a lieu APRÈS le maillage, et l'éviction
  qu'elle déclenche est reportée à l'appel suivant : la payer tout de suite
  demanderait un second remaillage, donc une seconde recopie d'arène en
  O(scène), à chaque image d'un vol.
- **Une cellule MAIGRIT quand sa voisine arrive.** Ses faces de bord, jusque-là
  exposées à du vide, se retrouvent masquées. Ne repeser que les arrivées
  laisse donc chaque cellule inscrite au poids qu'elle avait SEULE — un
  surcompte durable, et une fenêtre qui tient une fraction de ce qu'elle croit
  tenir. Rien ne le dit de soi-même : la scène reste juste, la mémoire reste
  bornée, seul le chiffre ment. C'est l'égalité « ce que la fenêtre compte est
  ce que la scène porte, à l'octet près » qui l'attrape, et c'est pour ça
  qu'elle est écrite comme une égalité et pas comme un ordre de grandeur.
- **Corriger un poids n'est pas un accès.** La correction ci-dessus passe par
  `Residency::update`, qui ne touche ni à la récence ni à l'état. `insert`
  ferait remonter en tête une cellule que personne n'a regardée : dans un vol
  en ligne droite, chaque cellule est réchauffée par l'arrivée de la suivante,
  donc la traînée n'est jamais rendue et le LRU garde exactement ce qu'il
  faudrait lâcher. `edit` la marquerait modifiée, donc inévinçable pour
  toujours. Mesuré par mutation : remplacer `update` par `insert` passait les
  cinq autres tests — la comptabilité reste exacte, la scène reste juste,
  seul CE QUI est lâché change.
- **La zone d'OUVERTURE doit s'inscrire comme le reste.** Sinon elle n'est
  jamais évinçable : on ouvre un monde, on vole cinq mille blocs plus loin, et
  les chunks du départ restent en mémoire pour toujours — la fuite même que la
  fenêtre existe pour empêcher. Invisible tant qu'on ne regarde que ce qui
  ARRIVE, et c'est le défaut qu'on ne trouve qu'en se demandant ce qui entre
  dans la scène SANS passer par la porte qu'on vient d'écrire.
- **Un filtre juste à un niveau est faux d'un facteur mille à l'autre.** Pour
  retrouver ce qu'une cellule portait, la scène balayait la grille sur
  `adresse.0 == cellule.x`. Vrai au niveau CHUNK ; une cellule de RÉGION en
  couvre 32 × 32, donc 1 023 colonnes sur 1 024 seraient restées dans la
  grille pour toujours — et sans rien qui se voie, puisque l'affichage, lui,
  aurait été juste. Les adresses se déduisent de la GÉOMÉTRIE de la cellule,
  ce qui supprime au passage un balayage en O(scène × cellules) : la cellule
  sait ce qu'elle couvre, la grille n'a pas à le chercher.
- **Une boucle écrite dans un gestionnaire d'image n'est jamais vérifiée.** La
  jonction caméra → demande → fil → scène aurait naturellement vécu dans le
  `RedrawRequested` de la fenêtre : derrière un serveur graphique, donc jamais
  en intégration continue. Mise dans la bibliothèque (`tf-app/src/pilote.rs`),
  elle prend un œil et un regard — pas une caméra — et un test peut donc faire
  VOLER une caméra. Elle a trouvé trois défauts le jour même, tous silencieux,
  et qu'aucune des trois pièces ne pouvait voir seule (ci-dessous).
- **Redemander à chaque image annule le chargement en cours.** Le fil remplace
  sa file à chaque demande — c'est voulu, une demande neuve chasse la périmée —
  donc soixante demandes par seconde annulent soixante fois la région qu'il
  lit. Le monde ne se charge jamais, et le symptôme est le PIRE possible : la
  fenêtre répond et la machine a l'air occupée. La règle est dans `Suivi`
  (pur) : on ne redemande qu'en FRANCHISSANT une frontière de cellule, ou
  quand le fil est au repos. Mesuré par un COMPTEUR de lectures de `.mca` —
  une seule pour 326 images — et pas par un chronomètre.
- **Un budget plus petit que le champ de vision fait tourner la machine à
  vide.** Le LRU évince une cellule que la caméra REGARDE encore, la demande
  la redemande aussitôt, elle arrive, elle en évince une autre du champ.
  Mesuré : 180 évictions en 100 images sur une scène qui n'avance pas d'un
  bloc, sans fin. Ce que la caméra regarde est donc épinglé — le LRU ne prend
  que dans la traînée — et si le champ seul ne tient pas, on DÉPASSE le
  budget en le disant (`Ouvert::deborde`) plutôt que de tourner en rond : « le
  plafond est une CIBLE, pas une limite dure ». Corollaire attrapé par
  mutation : le champ épinglé se recalcule à CHAQUE image et pas seulement
  quand une demande part, sinon voler au-dessus d'un terrain déjà chargé ne
  l'actualise jamais et la mémoire ne redescend plus — un symptôme qui se lit
  « elle ne rend rien quand je ne charge pas », c'est-à-dire l'inverse de ce à
  quoi on pense.
- **Un canal de travail sans borne est une fuite de mémoire.** Le fil lit une
  région d'un coup et émet ses mille cellules ; l'hôte n'en intègre que deux
  par image, parce que la recopie d'arène est en O(scène). Mesuré sur un vol
  de soixante chunks : l'hôte recevait encore des cellules 300 images après
  s'être ARRÊTÉ, et les sections décodées s'empilaient dans le canal — hors de
  tout budget, puisque la fenêtre de résidence ne compte que ce qui est POSÉ.
  Le canal est borné (`sync_channel`) : le fil attend quand l'hôte est en
  retard, ce qui est exactement ce qu'on veut — ce qu'il aurait décodé en
  avance serait périmé au premier mouvement de caméra. Corollaire : un canal
  borné rend l'arrêt délicat, le fil pouvant être bloqué dans un `send`. On
  LÂCHE le récepteur avant de joindre, ce qui fait échouer son envoi ; sonder
  avec un délai aurait marché « presque toujours ».
- **Une mesure qui ne mesure rien a l'air excellente.** Le premier essai de
  `--example vol` rendait 0,01 ms par image : quatre cents images enchaînées
  sans attendre durent 4 ms en tout, et le fil n'avait pas eu le temps de
  lire une seule région. ZÉRO cellule posée, et le meilleur chiffre de tout le
  dépôt. Une vraie fenêtre attend la synchronisation verticale entre deux
  images — c'est pendant ce temps-là que le fil travaille. La mesure est
  cadencée à 60 images par seconde, et elle REFUSE de rendre un chiffre si
  moins de cent cellules sont arrivées : une prémisse se vérifie, elle ne
  s'espère pas. Une fois juste, elle a dit ce que personne n'avait mesuré :
  35 ms par image sur du bâti, 396 images sur 400 au-delà du budget.
- **Une recopie « seulement de ce qui n'a pas changé » reste une recopie de
  la scène.** Les arènes remplaçaient leurs tranches en recopiant les autres :
  juste, et en O(scène) — 35 ms par image de vol sur du bâti, qui
  grandissaient avec elle. La cause était un indice : le champ `section`
  d'une instance était le RANG de son lot, donc une section qui apparaissait
  décalait toutes les suivantes. Un emplacement STABLE par section a supprimé
  la raison de recopier ; les places libérées deviennent des trous que le
  shader rend dégénérés, et l'arène garde un seul appel de dessin. 35 → 2 ms.
- **Une somme préfixe dichotomisée tient avec des trous, à trois conditions.**
  La passe de modèles attribue chaque face à la DERNIÈRE pose dont le début la
  précède. Il faut donc (1) des débuts croissants sur tout le tableau, (2) des
  trous faits de poses vides, (3) un TERMINAL vide au bout de chaque place —
  sans lui, les faces en trop d'une fenêtre tomberaient sur la dernière vraie
  pose et dessineraient le modèle suivant de la table, un bout de chaise collé
  à un escalier. Avec les trois, recoller deux trous ne réécrit rien, et en
  couper un ne réécrit que le préfixe où la croissance casserait.
- **Recoller un trou à son voisin d'AVANT laisse l'ancienne entrée derrière.**
  Une place libérée était encore rangée sous sa première pose ; recollée au
  trou qui la précède, le résultat se rangeait sous la clé du trou, et la place
  morte survivait par-dessus. Tous les cas simples passaient — il fallait que
  le voisin d'avant soit libre. Attrapé par un VÉRIFICATEUR de pavage appelé à
  chaque pas d'une suite tirée d'une graine, qui dit quelle règle casse et où,
  là où la comparaison d'images ne disait que « index hors bornes ».
- **Un emplacement jamais rendu dessine juste et fuit.** Une section partie
  qui garde son emplacement ne change pas un pixel — plus rien ne le désigne —
  mais la table grandit d'une entrée par section JAMAIS vue, soit des millions
  sur un vol de huit cents régions. La mutation passait tous les tests de
  dessin ; celui qui la tue COMPTE : un emplacement par lot, et pas un de plus.
- **Un trou qui dessine se cache derrière le build.** Retirer la garde du
  shader des quads ne changeait aucun pixel : un trou y retombe sur la branche
  par défaut de `coin`, donc un vrai quad d'un bloc au coin de l'emplacement
  0 — posé sur la face d'un cube, puis au FOND de la scène vue de la caméra
  par défaut. Le test compare maintenant les images vues des deux côtés, sur
  une case laissée vide exprès. Deux angles morts de suite dans le même test :
  une image ne prouve que ce qu'elle MONTRE.
- **Un test aléatoire qui dure sept minutes ne vérifie rien de plus.** Des
  sections pleines faisaient des millions de faces par pas, chacune comparée
  dans un `Vec<u8>` alloué. La dynamique vérifiée — trous, découpes,
  recollements, réemplois — ne dépend pas du volume : des sections creuses et
  une clé sans allocation, 446 s → 8 s, et les six mutations meurent pareil.
- **Une marge « d'une case » élargie sur les trois axes remaille les
  diagonales pour rien.** Le mailleur ne lit que les six voisins par face ;
  une colonne qui arrive ne change donc que ses quatre voisines par face, et
  la boîte élargie en remaillait neuf. `sections_touchees` rend la CROIX :
  5 colonnes au lieu de 9, et 214 → 68 images sur 400 au-delà du budget. Les
  lectures de peau d'arête et de coin existent (`Opacite::vide`, `bouchee`),
  mais ce sont des sorties anticipées qui ne changent pas le résultat. **Le
  contrat est celui du mailleur d'aujourd'hui** : l'occlusion ambiante lira
  les diagonales, et le test qui croise la croix avec un remaillage complet —
  éditions tirées aux arêtes et aux coins, là seulement où croix et boîte
  diffèrent — rougira ce jour-là. Le remède sera `sections_autour`, pas un
  test qu'on fait taire.
- **L'O(1) amorti d'un `Vec` est un pic en O(scène).** Chaque pic de plus de
  quatre millisecondes des deux arènes était un doublement de capacité — 16 ms
  pour recopier 2,4 millions d'instances — et aucun n'était le tassement qu'on
  soupçonnait. Mesuré en instrumentant l'agrandissement, pas deviné. Une scène
  qui remplit son budget aurait figé l'image plus d'un dixième de seconde au
  doublement suivant. Les arènes sont rangées par PAGES (`tf_render::Pages`) :
  grandir ajoute une page, rien ne bouge, et le GPU se remplit page par page
  sans recoller un tableau contigu — ce qui referait exactement la recopie.
- **Un test sauté faute de donnée cachait une course.** Les tests de
  `chantier.rs` exigeaient `TF_PACK` et ne tournaient donc jamais ; branchés
  sur le codex écrit à la volée, deux à quatre sur neuf tombaient au hasard.
  Le fichier avait SON `Jetable`, nommé par le seul numéro de processus et
  l'étiquette : deux tests qui demandaient « codex » partageaient le dossier,
  et le premier qui finissait effaçait le pack de l'autre. Troisième fois du
  même piège — un nom unique à l'échelle du processus ne l'est pas. Il n'y a
  plus qu'un `Jetable`, celui de `commun`, et il porte un compteur.
- **Refaire la scène GPU à chaque arrivée, c'est renvoyer le monde.**
  `regarnir` reconstruisait tout — tampons remplis de la scène entière,
  pipelines recompilés, atlas remonté avec ses mips — à chaque image où une
  cellule arrivait. Invisible sur un rastériseur logiciel, et c'est pour ça
  que personne ne l'avait mesuré : `--example vol -- --gpu refaire` l'a
  chiffré à 18 ms par image en médiane sur du bâti, 56 au pire, et QUINZE
  gigaoctets envoyés pour un vol de 400 images. Les arènes savaient déjà ce
  qui avait changé ; `Scene::synchroniser` s'en sert : 0,4 ms, 196 Mo.
- **`write_buffer` passe AVANT les commandes de sa soumission.** Un tampon
  qui grandit recopie l'ancien dans le neuf par une copie GPU ; si cette
  copie part avec le dessin, les écritures de l'image — exécutées au début
  de la même soumission — sont écrasées par l'ancien contenu. La copie est
  donc soumise SEULE, avant toute écriture. Un test fait grandir et réécrire
  en place dans la même synchronisation, et c'est la mutation qu'il tue.
- **Un tampon plus grand que son contenu ment à `arrayLength`.** La passe de
  modèles dichotomise ses poses jusqu'à `arrayLength(&poses)` ; un tampon
  qui a grandi d'avance porte au-delà des zéros, que la recherche prend pour
  des poses. Liées à leur taille EXACTE, et reliées quand leur NOMBRE change
  — pas seulement quand le tampon grandit. Les deux mutations passaient tous
  les tests tant que les tampons grandissaient pile à la taille demandée :
  il a fallu une petite croissance, qui laisse de la marge, puis des poses
  DANS la marge. Et, encore, une case (0, 0, 0) vide : c'est là qu'une pose
  de zéros dessine.
- **Un repli qui recharge la zone boucle dès qu'on streame.** Une texture
  plus grande que l'atlas le faisait « rebâtir » — c'est-à-dire recharger la
  ZONE. Sous le streaming, la zone rechargée ne contient pas la cellule qui
  avait amené la texture : l'atlas renaît au même côté, la caméra redemande
  la cellule, et tout recommence. Cinquante-trois rechargements en trente
  secondes sur le vrai codex, dont les crânes d'oiseau sont en 32 × 32 —
  attrapé en ouvrant la FENÊTRE, aucun test ne volait avec une texture plus
  grande. Le refus protégeait de « réécrire tous les pixels de l'atlas »,
  soit quelques mégaoctets ; il coûtait la zone, 867 ms sur du bâti, en
  boucle. L'atlas grandit maintenant sur place, et une arrivée ne peut plus
  recharger (`debug_assert` dans `integrer`). Le prix d'un repli se chiffre
  AVANT de le choisir.
- **L'identifiant 0 n'est pas l'air.** Le mailleur remplissait ce qui manque
  — section absente, chunk non chargé, indice corrompu — avec `0`, en le
  croyant l'air. Or `Interner::new()` part VIDE : 0 est le premier état
  décodé, `minecraft:deepslate` sur un monde 1.18. Un chunk non chargé
  devenait un mur de deepslate INVISIBLE : ses voisines perdaient leurs
  faces de bord, une save qui omet ses sections vides aurait perdu le dessus
  de ses builds, et le réticule s'arrêtait dans le vide au bord de ce qui est
  chargé. Aucun test ne le voyait : toutes leurs tables posent l'air en 0.
  Ce qui manque vaut maintenant `tf_mesh::ABSENT` (`StateId::MAX`), hors de
  toute table donc lu comme de l'air ; un test maille avec une table où 0
  est un bloc PLEIN. Deux tests en dépendaient sans le savoir — un trou au
  milieu du terrain devait faire BAISSER le nombre de quads, un petit disque
  devait peser sa part du grand — et ils avaient raison pour une mauvaise
  raison : une paroi qui manque allège la scène.
- **Une règle de lecture d'avant le streaming le trouait.** `remailler`
  retirait toutes les sections visées par une édition, puis ne relisait que
  celles de la ZONE d'ouverture — quand la zone était toute la scène, c'était
  juste. Sous le streaming, éditer là où l'on avait volé effaçait la cellule
  de la scène, et comme elle restait inscrite à la fenêtre de résidence,
  personne ne la redemandait : un trou définitif au premier geste d'édition
  loin du départ. On relit maintenant les visées dont la cellule est
  RÉSIDENTE, seulement elles — relire le reste ferait naître des sections
  qu'aucune éviction ne lâcherait — et on repèse ce qu'on vient d'éditer.
  Attrapé en cherchant pourquoi une mutation survivait : lire un morceau de
  code pour une raison fait trouver ce qu'il fait pour une autre.
- **Une voisine ne lit d'une section que l'opacité de la couche qui la
  touche.** La croix remaillait les quatre colonnes voisines de chaque
  cellule arrivée ou partie, quoi qu'elle porte à leur contact.
  `touchees_par_le_contenu` ne garde que les voisines dont la couche bordière
  a quelque chose d'opaque, relevé AVANT et APRÈS le changement — et après
  que la table a accueilli les états neufs, sinon un état jamais vu se lit
  « pas opaque » et la voisine garde sa face. Mesuré sur `Build` : 23 % de
  sections remaillées en moins, 9 % de maillage — modeste, parce que ce bâti
  touche ses bords partout ; un build entouré d'air, lui, n'en remaille plus
  une seule. Le test qui croise ce remaillage avec un maillage complet
  rougira le jour de l'occlusion ambiante, comme celui de la croix.
- **Une phase qui MONTE au fil d'un vol est un parcours de la scène.** Le vol
  par défaut ne remplit que 178 Mo : les phases `chantier` et `peser` y
  valaient 0,1 ms, donc rien. Au rayon 16, découpées par tranches de cent
  images, elles montaient de 0,1 à 0,5 et 0,4 ms à mesure que la scène
  grandissait — une liste de lots filtrée et retriée à chaque remplacement,
  et une pesée qui visitait tous les lots pour en trouver quelques dizaines.
  Au budget déclaré de 1,5 Go, plusieurs millisecondes par image. Une mesure
  prise à UNE taille de scène ne voit pas une pente ; une mesure découpée
  dans le temps, si. `tf_mesh::Maillages` tient le maillage par section :
  plat, 0,2 ms pour les deux.
- **Une région corrompue se lisait comme une région vide.** `tf_anvil::read`
  laisse vide un emplacement illisible pour sauver les 1023 autres — c'est
  voulu — mais sans le compter : un `.mca` abîmé s'affichait comme du vide,
  sans un mot. Il est compté (`Region::illisibles`), le bilan de lecture
  l'additionne, et le fil le DIT une fois par lecture. Vérifié au passage,
  parce que c'était la crainte : une région illisible n'est PAS relue en
  boucle — ses cellules reviennent vides et sont inscrites, deux lectures
  pour six cents images immobiles. Corollaire trouvé en écrivant le message :
  le compteur d'en-vol décomptait toute réponse, donc un message d'échec
  aurait fait croire le fil libre une cellule trop tôt, et la demande serait
  repartie chercher celle qui était encore en route.
- **Un maillage hors du fil s'applique dans l'ordre de DÉPART.** Chaque
  travail emporte un extrait de la grille telle qu'elle était à son départ ;
  appliqués dans l'ordre de retour, un vieux maillage revenu tard repasserait
  par-dessus un neuf — une cellule vidée se redessinerait pleine. Même
  raison, deux corollaires : une édition VIDE l'atelier avant de se mailler
  sur place, et un rechargement OUBLIE ce qui était en route. Aucun vol ne
  le vérifie sinon par chance : `retarder_le_prochain_maillage` force le
  retour tardif, `maillage_juste` compare la scène à un maillage complet de
  sa grille.
- **Une édition enfouie ne prouve rien.** La mutation « l'édition ne vide pas
  l'atelier » passait : le test posait de l'émeraude à y = −40, dans la
  pierre, et un bloc caché ne change aucune face visible — le vieux maillage
  posé par-dessus était identique au neuf. En plein ciel, elle meurt. Un test
  de maillage doit changer ce qui SE VOIT.
- **La réserve globale de `rayon` prend tous les cœurs.** Le maillage parti
  hors du fil principal y tournait sur quatre fils, plus le fil de
  chargement, plus le fil qui dessine : six pour quatre cœurs, et le fil
  principal perdait la main au hasard, au milieu de n'importe quelle phase —
  une synchronisation GPU de 0,4 ms en prenait 5,6 sans rien envoyer de plus.
  Mesuré avant d'écrire : `RAYON_NUM_THREADS=3` ramenait la médiane de 3,3 à
  2,7 ms et le p95 de ~9 à 5,4, pour le même nombre de cellules. La réserve
  du maillage (`reserve_de_maillage`) garde un cœur libre.
- **Réécrire les blocs sans l'éclairage, c'est laisser la lumière d'avant.**
  Le splice ne remplaçait que les champs de blocs : `SkyLight`, `BlockLight`
  et les `Heightmaps` décrivaient toujours l'ancien contenu. En jeu, une salle
  creusée sortait noire, un mur neuf ne portait pas d'ombre, la pluie
  traversait un toit posé — tant que le joueur ne modifiait pas un bloc à
  côté. Rien ne le signalait, et aucun test ne regardait ces champs : ils
  étaient « ce que le lecteur ne touche pas, donc ne peut pas abîmer ». On ne
  recalcule rien soi-même — ce serait réécrire le moteur d'éclairage du jeu
  pour se tromper là où lui ne se trompe pas : on lui DEMANDE de le faire,
  par ses propres mécanismes de chargement (`isLightOn` à 0, `Heightmaps`
  retiré), sur les seuls chunks dont les BLOCS changent. Le journal enregistre
  ces éditions avec les autres : annuler rend aussi l'éclairage d'origine.
- **Une copie qui garde l'`UUID` de l'original en perd UN des deux.** Le jeu
  jette au chargement toute entité dont l'`UUID` existe déjà, avec un simple
  avertissement dans son journal — donc l'original OU la copie, selon celui
  des deux chunks qui se charge en second. Une copie reçoit un `UUID` neuf,
  haché sur l'ancien et la position d'arrivée (rejouable, et deux collages
  identiques rendent les mêmes, que la pose remplace) ; un déplacement garde
  le sien, c'est la même entité.
- **Une entité accrochée suit le bloc qui la PORTE, pas la case où elle
  flotte.** Un cadre sur la face extérieure d'un mur occupe la case d'à côté,
  hors du bâtiment — parfois dans le chunk d'à côté. Choisir par la position
  laissait tous les cadres de façade derrière un mur déplacé ; d'où aussi une
  lecture qui déborde d'un bloc. Même règle, un cran plus loin, pour ce dont
  une entité se SOUVIENT : un lit suit s'il est dans la sélection, un poste de
  travail resté dans l'atelier d'en face non.
- **Le jeu n'écrit pas de chunk d'entités VIDE : il le supprime.** Poser la
  première entité d'un chunk, c'est en CRÉER un — et l'annuler doit le faire
  disparaître, pas laisser une coquille. Le journal ne savait parler que de
  chunks existants ; un chunk absent se lit maintenant comme un tampon vide,
  dans les deux sens.
- **Le rejeu était le seul chemin d'écriture qui ignorait les `.mcc`.**
  Trouvé en lui apprenant à créer des chunks : il ne résolvait pas le talon
  d'une charge déportée (l'annulation d'un chunk de plus d'un mégaoctet
  échouait) et n'écrivait pas le `.mcc` d'un chunk qui repassait au-delà (un
  talon vers un fichier absent : le chunk perdu). Chaque chemin qui écrit une
  région doit porter les deux moitiés ; un chemin de plus est un endroit de
  plus où l'oublier.
- **Une table indexée par la clé qui devrait être unique avale le doublon
  qu'on cherche.** Le test du déplacement relisait les entités dans une table
  par `UUID` : un déplacement qui aurait oublié de retirer l'original passait,
  la copie écrasant l'original DANS LA TABLE. Trouvé par mutation. On compte
  les vues, puis les clés, et on exige l'égalité des deux.
