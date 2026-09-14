# Vision, et ce qu'elle impose au code

Ce document n'est pas une déclaration d'intention. C'est la liste des
**décisions techniques** que la vision long terme contraint, et de celles
qu'elle interdit. Il est là pour qu'on n'ait pas à la redécouvrir à chaque
session — et pour qu'on refuse, en connaissance de cause, les raccourcis qui
coûteraient une refonte dans deux ans.

## L'objectif

Pas « un MCEdit moderne ». Les **fondations d'un écosystème de création de
contenu Minecraft**, dont la première mission — et le cœur permanent — est
d'être le meilleur éditeur de mondes possible.

Ordre de priorité, sans ambiguïté :

1. performances à grande échelle
2. stabilité
3. maintenabilité
4. extensibilité

Et une règle qui tranche tous les arbitrages : **aucune évolution future ne
doit dégrader le cœur.** Un éditeur qui devient lent parce qu'il sait faire des
quêtes a perdu ce qui le rendait utile.

## Ce qui viendra se greffer

PNJ, quêtes et dialogues, routes et comportements d'IA, outils RPG, outils
cinématiques, caméras avancées, génération procédurale, captures haute
qualité, rendu avec shaders, mise en scène, storytelling, création de serveurs
RP.

Rien de tout cela ne sera dans le cœur. Tout passera par des **greffons**.

## Ce qui est tranché

Trois questions posées, trois réponses qui bornent le travail — et qui le
réduisent, ce qui est la bonne nouvelle.

**Pas de PNJ ni de quêtes en 1.0.** Ils viendront en 2.0, par un greffon confié
à quelques personnes qui écrivent les quêtes. Conséquence directe : *aucun
magasin de documents à concevoir aujourd'hui*. Il reste la couture — un format
de projet **versionné** et un journal **typé** — pour qu'activer le greffon en
2.0 n'oblige pas à migrer les projets de 1.0.

**Solo.** Un seul éditeur à la fois, sur le poste de la personne ; on s'échange
des fichiers exportés. Le journal reste **linéaire** et le staging une copie
locale. C'était la question la plus coûteuse à retrofiter de tout le projet :
un log d'opérations synchronisé aurait changé le staging, l'annulation et le
stockage. La réponse nous l'épargne.

**Annulation persistante, avec points de reprise nommés.** On ferme, on rouvre,
on peut encore annuler — et on peut revenir à « avant la muraille ». Ça impose
un journal sur disque à format **figé et versionné**, ce qui était de toute
façon nécessaire pour les entrées de greffon de la 2.0.

---

# Les six contraintes que ça impose

## 1. Le greffon décrit, le cœur exécute

**C'est la contrainte la plus importante du document, et la plus facile à
casser par inadvertance.**

Si un greffon peut appeler `set_block(x, y, z)`, alors toute opération de
greffon retombe à l'étage bloc — et les trois étages qui font la vitesse du
moteur ne servent plus à rien. Mesuré : 349 µs contre 33,1 ms sur la même
opération, soit × 95 perdus dès le premier greffon.

L'API de greffon est donc **déclarative et par lot**, jamais un rappel par
bloc :

| Le greffon fournit | Le cœur en fait |
|---|---|
| un masque (prédicat sur un état, une hauteur, un biome) | un test par SECTION quand il se factorise |
| un motif (table pondérée, bruit, dégradé) | une réécriture de palette quand c'est possible |
| une forme (sphère, spline, volume) | un `classify(section) → Toute / Aucune / Partielle` |
| une opération composée des trois | le plan d'exécution à trois étages |

Un greffon qui a vraiment besoin de visiter chaque bloc le déclare, et paie ce
que ça coûte — mais ce doit être l'exception qu'on écrit explicitement, pas ce
qui arrive par défaut.

**Corollaire à ne jamais oublier :** l'API de greffon se conçoit *avant* d'être
utile. La rendre déclarative plus tard voudrait dire réécrire tous les greffons
existants.

## 2. Le journal d'annulation n'est pas un journal de blocs

Un greffon qui pose un PNJ, écrit un dialogue ou trace une route doit pouvoir
être **annulé par le même Ctrl+Z** que celui qui défait un `//set`. Sinon
l'utilisateur apprend à ne pas faire confiance à l'annulation, et c'est fini.

Le journal est donc une **suite d'entrées réversibles typées**, dont
l'instantané de section n'est qu'un cas parmi d'autres :

```
JournalEntry
 ├── Sections   { instantanés zstd des sections touchées }
 ├── Document   { avant / après d'un document de projet }
 └── Greffon    { identifiant du greffon + sa charge de défaisage }
```

**C'était l'erreur que j'étais sur le point de commettre** en concevant le
journal comme « des instantanés de section compressés ». Ça coûte presque rien
à faire correctement aujourd'hui, et c'est une refonte de toute la pile d'undo
plus tard — donc de tout ce qui l'utilise.

## 3. Un monde n'est pas seulement des blocs

**Rien de ceci n'est construit en 1.0** — voir « Ce qui est tranché ». Ce qui
est construit, c'est la place où ça viendra : un format de projet versionné,
extensible sans migration.

PNJ, quêtes, routes, points de caméra seront des **documents de projet**, pas
des blocs. Ils auront besoin de trois choses que la grille de voxels ne donne
pas :

- **Une identité stable.** Une quête désigne un PNJ ; ce lien doit survivre à
  tout ce qu'on fait au monde. Donc un identifiant opaque, jamais une position.
- **Un ancrage qui suit les éditions.** Un PNJ posé à (120, 64, −300) doit
  bouger si l'utilisateur translate la région qui le porte. C'est le même
  problème que les *block entities* d'un build pivoté, que `we-engine` a déjà
  payé : « un build pivoté abandonne ses coffres ». Toute opération qui déplace
  des blocs déplace aussi ce qui y est ancré.
- **Une migration.** Le format d'un document de greffon changera. Il porte donc
  sa version, dès la première ligne écrite.

## 4. Ce que le cœur ne comprend pas, il ne doit pas pouvoir l'abîmer

Déjà acquis, et ça devient central plutôt qu'accessoire. Le splice ne
reconstruit jamais un chunk : il remplace les octets qu'il connaît et recopie
le reste. Un greffon — comme un mod — peut donc écrire ses données dans le
NBT d'un chunk sans que le cœur ait à les connaître.

C'est ce qui permettra à un greffon de quêtes de stocker ses marqueurs là où
Minecraft les relira, sans qu'on ait à toucher `tf-anvil`.

## 5. Deux moteurs de rendu, pas un

« Rendu avec shaders » et « captures haute qualité » ne sont pas le même
problème que « 60 FPS sur 100 millions de blocs ».

- Le rendu **interactif** optimise la latence : culling d'occlusion, LOD,
  multi-draw indirect, et il a le droit de simplifier ce qui est loin.
- Le rendu **hors ligne** optimise la fidélité : il peut prendre trente
  secondes, tout charger, et ne rien simplifier.

Vouloir un seul pipeline qui fasse les deux donne un rendu interactif ralenti
par des options qu'il n'utilise pas. Ils partagent la **géométrie** et les
**matériaux** ; ils ne partagent pas la boucle.

Conséquence sur `tf-render` : un **graphe de passes nommées**, pas un pipeline
figé. C'est ce qui permettra à un greffon d'insérer une passe sans toucher au
cœur — et c'est beaucoup moins cher à poser au départ qu'à retrofiter.

## 6. La frontière de greffon est du WASM, pas une bibliothèque native

Un greffon natif (`.dll`, `.so`) qui plante emporte l'application, et les
données non sauvegardées de l'utilisateur avec. Rust n'a d'ailleurs pas d'ABI
stable : chaque version du compilateur casserait tous les greffons.

WASM donne le bac à sable, une ABI stable et la portabilité. Il est plus lent
qu'un appel natif — et c'est précisément pourquoi la contrainte n° 1 existe :
si le greffon décrit au lieu d'exécuter, le coût de la frontière est payé une
fois par opération, pas une fois par bloc.

Les greffons de **première partie** peuvent rester natifs et compilés avec le
cœur. Ceux des autres passent par le bac à sable.

---

# Ce que ça ne change pas

La vision ne remet en cause aucune décision déjà prise, et en confirme
plusieurs :

- **Rust** — un greffon WASM demande un hôte (`wasmtime`), et l'écosystème Rust
  est le plus solide là-dessus. Et la sûreté mémoire compte davantage encore
  quand du code tiers tourne dans le processus.
- **wgpu direct plutôt que Bevy** — un graphe de passes qu'on possède est
  exactement ce que la contrainte n° 5 demande. Le render graph de Bevy change
  à chaque version : bâtir une API de greffon dessus la casserait à chaque mise
  à jour.
- **Le round-trip lossless par splice** — voir la contrainte n° 4.
- **Les trois étages d'opération** — voir la contrainte n° 1.
- **La discipline de mesure** — un écosystème rend les régressions plus faciles
  à introduire et plus difficiles à voir. Le bench devient plus utile, pas
  moins.

# Ce qu'on ne fait PAS maintenant

Le runtime de greffons ne se construit pas avant que le cœur soit fini. Ce qui
se pose aujourd'hui, ce sont les **coutures** : les registres au lieu des
`enum` figés, le journal typé au lieu du journal de sections, le document de
projet extensible au lieu du champ ajouté à la va-vite.

Une couture coûte quelques heures aujourd'hui. La même, retrofitée, coûte une
refonte — et c'est exactement ce que ce projet existe pour éviter.
