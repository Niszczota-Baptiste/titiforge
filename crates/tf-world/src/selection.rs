//! **Ce sur quoi on travaille : deux coins, et les gestes qui les bougent.**
//!
//! Une sélection est l'objet central d'un éditeur de monde — tout ce que
//! `tf-ops` sait faire s'applique à une `BBox`. Ce module tient l'ÉTAT que la
//! coque manipule : les deux coins, les agrandissements par face, l'alignement
//! sur le découpage, et la face qu'un rayon désigne.
//!
//! ## Pourquoi la face visée est ici et pas dans le rendu
//!
//! Attraper une face pour la tirer est le premier tiers de SketchUp. Ce n'est
//! pas du dessin : c'est de la géométrie sur la sélection, et c'est l'entrée
//! de `//expand`. Le mettre dans le rendu obligerait la coque à demander à
//! l'écran ce que la sélection sait déjà.
//!
//! ## Les bornes sont INCLUSES, la géométrie est exclusive
//!
//! `BBox` va de `min` à `max` **inclus** — « de −10 à 10 » fait 21 blocs. Mais
//! un bloc occupe une case d'une unité de côté : le solide d'une sélection va
//! donc de `min` à `max + 1`. Confondre les deux rend la dernière rangée de
//! blocs impossible à attraper, ce qui se lit « le bord de la sélection ne
//! répond pas » et ne désigne pas la cause. La conversion est faite une fois,
//! par `BBox::coins`.

use crate::coords::{BBox, BlockPos};
use crate::decoupe::Niveau;

/// Les six directions du repère Minecraft.
///
/// **+X = Est, +Z = Sud, +Y = Haut** — la convention de tout le dépôt.
///
/// `tf_mesh::forme::Face` décrit les mêmes six directions pour le mailleur, et
/// les deux ne peuvent pas partager un type : ni `tf-world` ni `tf-mesh` ne
/// dépend de l'autre, et les faire dépendre mettrait le mailleur sous
/// l'éditeur ou l'inverse. La parade n'est pas d'espérer qu'elles restent
/// d'accord, c'est de les MESURER l'une contre l'autre : un test de
/// `tf-render` — le seul crate qui voie les deux — croise les six par leur
/// `pas()`, donc par ce qu'elles VEULENT DIRE et non par leur rang.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    MoinsX,
    PlusX,
    MoinsY,
    PlusY,
    MoinsZ,
    PlusZ,
}

pub const DIRECTIONS: [Direction; 6] = [
    Direction::MoinsX,
    Direction::PlusX,
    Direction::MoinsY,
    Direction::PlusY,
    Direction::MoinsZ,
    Direction::PlusZ,
];

impl Direction {
    /// Son nom, dans le repère Minecraft : **+X = Est, +Z = Sud, +Y = Haut**.
    ///
    /// Ici et nulle part ailleurs. La ligne de commande lisait déjà
    /// « est|ouest|nord|sud|haut|bas » avec sa propre table, et une coque en
    /// aurait écrit une deuxième : ce dépôt a payé QUATRE fois le piège des
    /// tables qui divergent, et celle-ci se serait trompée de signe sur Z —
    /// l'axe dont personne ne se rappelle le sens.
    pub const fn nom(self) -> &'static str {
        match self {
            Direction::MoinsX => "ouest",
            Direction::PlusX => "est",
            Direction::MoinsY => "bas",
            Direction::PlusY => "haut",
            Direction::MoinsZ => "nord",
            Direction::PlusZ => "sud",
        }
    }

    /// L'inverse de `nom`, tolérant sur la casse et les espaces.
    pub fn depuis_nom(nom: &str) -> Option<Direction> {
        let n = nom.trim().to_lowercase();
        DIRECTIONS.iter().copied().find(|d| d.nom() == n)
    }

    /// L'axe : 0 = X, 1 = Y, 2 = Z.
    pub const fn axe(self) -> usize {
        (self as usize) >> 1
    }

    /// Vrai du côté POSITIF de l'axe.
    pub const fn positif(self) -> bool {
        (self as usize) & 1 == 1
    }

    /// Le pas d'un bloc dans cette direction.
    pub const fn pas(self) -> [i32; 3] {
        let d = if self.positif() { 1 } else { -1 };
        match self.axe() {
            0 => [d, 0, 0],
            1 => [0, d, 0],
            _ => [0, 0, d],
        }
    }

    pub const fn opposee(self) -> Direction {
        DIRECTIONS[(self as usize) ^ 1]
    }

    pub const fn depuis(axe: usize, positif: bool) -> Direction {
        DIRECTIONS[(axe % 3) * 2 + positif as usize]
    }

    /// Le VERROU d'axe qu'impose une pose contre cette face.
    ///
    /// **Sans lui, l'inférence ramène le bloc DANS le mur.** On vise la face
    /// Est d'une paroi, on pose donc à `x + 1` ; mais la paroi est à un bloc,
    /// donc dans la tolérance, et l'accrochage aligne `x` dessus — le bloc
    /// neuf atterrit à l'intérieur de ce qu'on visait. Mesuré en écrivant la
    /// jonction : l'accroche était juste, la pose aussi, et leur composition
    /// fausse.
    ///
    /// La règle : **l'axe de la face contre laquelle on pose est décidé par la
    /// pose, pas par l'inférence.** Les deux autres restent libres — et ce
    /// sont eux qui portent tout l'intérêt, puisque c'est dans le plan de la
    /// face qu'on veut s'aligner sur ce qui est bâti.
    pub fn verrou(self, pose: BlockPos) -> [Option<i32>; 3] {
        let mut v = [None; 3];
        v[self.axe()] = Some([pose.x, pose.y, pose.z][self.axe()]);
        v
    }
}

impl BBox {
    /// Les deux coins du SOLIDE, en flottants.
    ///
    /// `max + 1` parce que le bloc `max` occupe une case entière. C'est la
    /// conversion entre des bornes incluses et une géométrie, et elle se fait
    /// ICI, une fois : recopiée sur place, elle est oubliée une fois sur deux,
    /// et la dernière rangée de blocs cesse d'être attrapable.
    pub fn coins(&self) -> ([f32; 3], [f32; 3]) {
        (
            [self.min.x as f32, self.min.y as f32, self.min.z as f32],
            [
                self.max.x as f32 + 1.0,
                self.max.y as f32 + 1.0,
                self.max.z as f32 + 1.0,
            ],
        )
    }
}

/// L'état de la sélection : deux coins, posés indépendamment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub coin1: Option<BlockPos>,
    pub coin2: Option<BlockPos>,
}

impl Selection {
    pub fn nouvelle() -> Selection {
        Selection::default()
    }

    /// La boîte, ou `None` tant qu'il manque un coin.
    ///
    /// **Un seul coin n'est PAS une sélection d'un bloc.** Rendre une boîte
    /// dès le premier clic laisserait une opération partir sur une case
    /// choisie à moitié — et `//set` sur un bloc ne ressemble pas assez à une
    /// erreur pour qu'on la remarque.
    pub fn boite(&self) -> Option<BBox> {
        Some(BBox::new(self.coin1?, self.coin2?))
    }

    pub fn poser_coin1(&mut self, p: BlockPos) {
        self.coin1 = Some(p);
    }

    pub fn poser_coin2(&mut self, p: BlockPos) {
        self.coin2 = Some(p);
    }

    pub fn vider(&mut self) {
        *self = Selection::default();
    }

    /// Inclut un point dans la sélection, sans rien perdre.
    ///
    /// Le premier point pose les deux coins : une sélection d'un bloc est un
    /// résultat légitime quand on l'a DEMANDÉ, contrairement à une moitié de
    /// geste.
    pub fn etendre_a(&mut self, p: BlockPos) {
        match self.boite() {
            None => {
                self.coin1 = Some(p);
                self.coin2 = Some(p);
            }
            Some(mut b) => {
                b.extend(p);
                self.coin1 = Some(b.min);
                self.coin2 = Some(b.max);
            }
        }
    }

    /// `//expand` : pousse UNE face de `n` blocs. `n` négatif la ramène
    /// (`//contract`).
    ///
    /// **Une face ne traverse jamais la face opposée.** Contracter au-delà
    /// laisserait une boîte retournée, donc une sélection qui couvre
    /// brusquement l'autre côté — et `BBox::new` la normaliserait sans rien
    /// dire, ce qui est pire que de refuser. On s'arrête à un bloc
    /// d'épaisseur, comme WorldEdit.
    ///
    /// Rend `false` s'il n'y a pas de sélection, ou si le geste ne change
    /// rien : une interface qui ne sait pas distinguer les deux propose des
    /// boutons qui ont l'air cassés.
    pub fn agrandir(&mut self, d: Direction, n: i32) -> bool {
        let Some(b) = self.boite() else { return false };
        let k = d.axe();
        let bas = [b.min.x, b.min.y, b.min.z];
        let haut = [b.max.x, b.max.y, b.max.z];
        let (mut bas, mut haut) = (bas, haut);
        if d.positif() {
            // En `i64` : un `//expand 2000000000` près de `i32::MAX` ferait
            // revenir la face de l'autre côté du monde, et l'opération
            // suivante écrirait là-bas.
            haut[k] = (haut[k] as i64 + n as i64).clamp(bas[k] as i64, i32::MAX as i64) as i32;
        } else {
            bas[k] = (bas[k] as i64 - n as i64).clamp(i32::MIN as i64, haut[k] as i64) as i32;
        }
        let neuve = BBox::new(
            BlockPos::new(bas[0], bas[1], bas[2]),
            BlockPos::new(haut[0], haut[1], haut[2]),
        );
        if neuve == b {
            return false;
        }
        self.coin1 = Some(neuve.min);
        self.coin2 = Some(neuve.max);
        true
    }

    /// Déplace la sélection entière.
    pub fn deplacer(&mut self, d: [i32; 3]) -> bool {
        let Some(b) = self.boite() else { return false };
        let bouge = |p: BlockPos| {
            BlockPos::new(
                (p.x as i64 + d[0] as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                (p.y as i64 + d[1] as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                (p.z as i64 + d[2] as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            )
        };
        self.coin1 = Some(bouge(b.min));
        self.coin2 = Some(bouge(b.max));
        true
    }

    /// `//chunk` : étend aux cellules entières du découpage.
    pub fn aligner(&mut self, niveau: Niveau) -> bool {
        let Some(b) = self.boite() else { return false };
        let a = b.aligner(niveau);
        if a == b {
            return false;
        }
        self.coin1 = Some(a.min);
        self.coin2 = Some(a.max);
        true
    }

    /// Étend la HAUTEUR aux sections entières.
    ///
    /// L'autre moitié de ce qui donne l'étage palette : `aligner` ne touche
    /// qu'à x et z, et une sélection alignée sur les chunks dont la hauteur
    /// tombe au milieu d'une tranche de seize ne couvre AUCUNE section
    /// entière.
    pub fn aligner_sections(&mut self) -> bool {
        let Some(b) = self.boite() else { return false };
        let a = b.aligner_sections();
        if a == b {
            return false;
        }
        self.coin1 = Some(a.min);
        self.coin2 = Some(a.max);
        true
    }

    /// La face de la sélection qu'un rayon désigne, et à quelle distance.
    ///
    /// **C'est ce qu'on attrape pour pousser-tirer.** Méthode des tranches :
    /// le rayon entre par la plus TARDIVE des trois entrées et sort par la
    /// plus PRÉCOCE des trois sorties ; s'il entre après être sorti, il rate.
    ///
    /// Depuis l'intérieur de la boîte, c'est la face de SORTIE qui est rendue
    /// — celle qu'on regarde. Rendre « rien » y serait faux : on voit bien une
    /// paroi, et un outil qui refuse d'attraper ce qu'on voit passe pour cassé.
    pub fn face_visee(&self, origine: [f32; 3], direction: [f32; 3]) -> Option<(Direction, f32)> {
        let b = self.boite()?;
        if !origine.iter().all(|c| c.is_finite()) || !direction.iter().all(|c| c.is_finite()) {
            return None;
        }
        let (min, max) = b.coins();
        let (mut entree, mut sortie) = (f32::NEG_INFINITY, f32::INFINITY);
        let (mut face_entree, mut face_sortie) = (None, None);

        for k in 0..3 {
            if direction[k] == 0.0 {
                // Parallèle à cette paire de plans : soit on est entre les
                // deux pour toujours, soit on ne les rencontrera jamais.
                if origine[k] < min[k] || origine[k] > max[k] {
                    return None;
                }
                continue;
            }
            let t1 = (min[k] - origine[k]) / direction[k];
            let t2 = (max[k] - origine[k]) / direction[k];
            // Quelle face est la PLUS PROCHE dépend du sens de marche : en
            // allant vers +X on rencontre d'abord la face −X.
            let (proche, loin, f_proche, f_loin) = if direction[k] > 0.0 {
                (
                    t1,
                    t2,
                    Direction::depuis(k, false),
                    Direction::depuis(k, true),
                )
            } else {
                (
                    t2,
                    t1,
                    Direction::depuis(k, true),
                    Direction::depuis(k, false),
                )
            };
            if proche > entree {
                entree = proche;
                face_entree = Some(f_proche);
            }
            if loin < sortie {
                sortie = loin;
                face_sortie = Some(f_loin);
            }
            if entree > sortie {
                return None;
            }
        }
        // Toute la boîte est DERRIÈRE : on ne vise rien.
        if sortie < 0.0 {
            return None;
        }
        if entree >= 0.0 {
            face_entree.map(|f| (f, entree))
        } else {
            // On est dedans : la face qu'on regarde est celle par où l'on
            // sortirait.
            face_sortie.map(|f| (f, sortie))
        }
    }
}

/// **Combien de blocs on a tiré le long d'un axe.**
///
/// Le geste de pousser-tirer : on a attrapé une face, on déplace la souris, et
/// il faut en tirer un NOMBRE DE BLOCS le long de la normale. Ce n'est pas un
/// glissement d'écran multiplié par une sensibilité — ça donnerait un geste
/// qui dérive selon la distance et l'angle, ce que personne ne sait corriger
/// à l'œil. C'est la projection du rayon de souris sur la DROITE de l'axe :
/// le point de la droite le plus proche du rayon.
///
/// `ancre` est le point du monde où le geste a commencé, `d` l'axe suivi.
/// Rend le déplacement en blocs, arrondi.
///
/// **`None` quand le rayon est trop parallèle à l'axe.** Là, la projection
/// part à l'infini : un pixel de souris vaudrait des centaines de blocs.
/// SketchUp refuse aussi — et refuser, c'est ne pas bouger, pas bouger
/// n'importe comment.
pub fn glissement(
    ancre: [f32; 3],
    d: Direction,
    origine: [f32; 3],
    direction: [f32; 3],
) -> Option<i32> {
    if !ancre.iter().all(|c| c.is_finite())
        || !origine.iter().all(|c| c.is_finite())
        || !direction.iter().all(|c| c.is_finite())
    {
        return None;
    }
    let u = {
        let p = d.pas();
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    let v = direction;
    let n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if n2 < 1e-12 {
        return None;
    }
    // Le classique « point de la droite A le plus proche de la droite B » :
    // avec `u` unitaire, uu vaut 1 et le système se réduit à deux produits
    // scalaires.
    let w = [
        ancre[0] - origine[0],
        ancre[1] - origine[1],
        ancre[2] - origine[2],
    ];
    let uv = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let wu = w[0] * u[0] + w[1] * u[1] + w[2] * u[2];
    let wv = w[0] * v[0] + w[1] * v[1] + w[2] * v[2];
    // `denom` s'annule quand le rayon est colinéaire à l'axe. On s'arrête
    // BIEN avant zéro : à un degré près de la colinéarité, un pixel vaut déjà
    // des dizaines de blocs.
    let denom = n2 - uv * uv;
    if denom.abs() < 1e-3 * n2 {
        return None;
    }
    let t = (uv * wv - wu * n2) / denom;
    if !t.is_finite() {
        return None;
    }
    // Un tirage se compte en blocs ENTIERS. L'arrondi au plus proche, pas la
    // troncature : tronquer collerait le geste à zéro sur toute la première
    // moitié du premier bloc, ce qui se lit « la poignée ne répond pas ».
    let n = t.round();
    if n.abs() > i32::MAX as f32 {
        return None;
    }
    Some(n as i32)
}

/// **La TRANCHE qu'un pousser-tirer a ajoutée ou retirée.**
///
/// C'est elle que l'opération écrit, et pas la sélection entière : tirer une
/// face de trois blocs sur un bâtiment de cent mille ne doit pas réécrire le
/// bâtiment. C'est l'invariant n° 8 — une opération ne paie que sa portée —
/// appliqué au geste.
///
/// `avant` et `apres` sont les boîtes de part et d'autre du geste, `d` la face
/// attrapée. Rend `None` quand rien n'a bougé sur cet axe.
///
/// **Les bornes d'une `BBox` sont INCLUSES**, et c'est là que ça se joue : la
/// tranche ajoutée commence à `ancien_max + 1`, pas à `ancien_max`. Un bloc
/// d'écart se lit « la dernière rangée ne se remplit pas », ou pire, « une
/// rangée de trop est écrasée ».
pub fn tranche(avant: BBox, apres: BBox, d: Direction) -> Option<BBox> {
    let k = d.axe();
    let (a0, a1) = (
        [avant.min.x, avant.min.y, avant.min.z][k],
        [avant.max.x, avant.max.y, avant.max.z][k],
    );
    let (b0, b1) = (
        [apres.min.x, apres.min.y, apres.min.z][k],
        [apres.max.x, apres.max.y, apres.max.z][k],
    );
    // La tranche prend les DEUX autres axes de la boîte d'après : c'est là
    // qu'on écrit, et c'est elle qui décrit le volume final.
    let (bas, haut) = if d.positif() {
        if b1 > a1 {
            (a1 + 1, b1) // tiré : ce qui s'ajoute au-delà de l'ancienne face
        } else if b1 < a1 {
            (b1 + 1, a1) // poussé : ce qui vient d'en sortir
        } else {
            return None;
        }
    } else if b0 < a0 {
        (b0, a0 - 1)
    } else if b0 > a0 {
        (a0, b0 - 1)
    } else {
        return None;
    };
    let mut min = [apres.min.x, apres.min.y, apres.min.z];
    let mut max = [apres.max.x, apres.max.y, apres.max.z];
    min[k] = bas;
    max[k] = haut;
    Some(BBox::new(
        BlockPos::new(min[0], min[1], min[2]),
        BlockPos::new(max[0], max[1], max[2]),
    ))
}
