//! Le pilotage de la caméra, et la répartition des boutons.
//!
//! **Le point fixe est le JOUEUR, jamais le build.** Tourner fait pivoter le
//! regard autour de l'œil ; l'œil ne bouge pas. C'est ce que fait Minecraft,
//! donc ce que la main de quiconque construit sait déjà faire — et une
//! habitude de jeu ne se rééduque pas, elle se sert.
//!
//! Une orbite autour d'un pivot posé sur le build a été écrite, puis retirée :
//! c'est la convention de la CAO, pas celle d'un monde où l'on vole. Elle
//! obligeait en plus à décider d'un pivot à chaque bascule de mode — le seul
//! morceau de tout ce module qui était difficile à rendre juste. Pour tourner
//! autour d'un bâtiment, on vole autour, exactement comme en jeu.
//!
//! ## La répartition des boutons
//!
//! | Geste | Effet |
//! |---|---|
//! | **Molette ENFONCÉE**, glisser | tourner le regard |
//! | Molette enfoncée + Maj, glisser | se déplacer de côté et en hauteur |
//! | Molette, rouler | avancer et reculer |
//! | **Clic gauche** | à la sélection et aux outils |
//! | **Clic droit** | à la sélection et aux outils |
//!
//! C'est la caméra de SketchUp (molette) et la sélection de WorldEdit (gauche
//! = coin 1, droit = coin 2) réunies sans se marcher dessus. Donner la caméra
//! au clic droit coûterait l'un des deux coins de WorldEdit, et c'est le geste
//! que tout utilisateur de WorldEdit connaît par cœur.
//!
//! Le piège en creux est celui d'`ExeWorldEdit` : *un outil qui coupe la
//! caméra entière enferme l'utilisateur*. Ici il ne peut pas se produire — la
//! caméra a un bouton à elle, qu'aucun outil ne prend.
//!
//! ## Ce que le module NE fait pas
//!
//! Il ne connaît ni fenêtre, ni `winit`, ni souris : il prend des ANGLES et
//! des distances déjà décidés, et rend une `Camera`. C'est la même frontière
//! que celle du mailleur avec les packs de ressources — ce qui se teste sans
//! écran doit pouvoir se tester sans écran.

use crate::camera::Camera;

/// Les deux modes de travail.
///
/// Nommés d'après ce qu'ils FONT, pas d'après les outils qu'ils imitent :
/// WorldEdit et MCEdit transforment ce qui existe, SketchUp permet de
/// concevoir. C'est la distinction de `docs/VISION.md`, et elle survivra aux
/// noms de ces trois logiciels.
///
/// **La caméra ne dépend PAS du mode** : on vole dans les deux, le point fixe
/// est le joueur dans les deux. Ce qui change est ce que font le clic gauche
/// et le clic droit — un VOLUME d'un côté, des ENTITÉS de l'autre. C'est pour
/// ça que le mode vit ici, avec la répartition des boutons, et pas dans la
/// caméra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Transformer ce qui existe. Gauche et droit posent les deux coins d'une
    /// sélection, comme WorldEdit.
    #[default]
    Edition,
    /// Concevoir. Gauche et droit désignent des faces, des arêtes, des
    /// composants.
    Conception,
}

/// L'inclinaison maximale, à un cheveu de la verticale.
///
/// **Pas π/2 exactement.** À la verticale pile, la direction du regard est
/// colinéaire au « haut » du monde, leur produit vectoriel est nul, et la base
/// de la vue devient dégénérée : l'image bascule d'un quart de tour sans
/// prévenir. Un demi-degré de marge coûte ce qu'on ne voit pas et évite ce
/// qu'on ne comprend pas. Minecraft fait exactement pareil.
pub const PENTE_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.0087;

/// La direction que regardent un cap et une pente.
///
/// Une seule fois, ici : tout ce qui s'oriente s'en sert, et deux copies de la
/// même trigonométrie finissent par diverger sur un signe — c'est arrivé
/// quatre fois dans ce dépôt, sur les rotations de modèles.
pub fn direction(cap: f32, pente: f32) -> [f32; 3] {
    let (sc, cc) = cap.sin_cos();
    let (sp, cp) = pente.sin_cos();
    [cc * cp, sp, sc * cp]
}

/// Le cap et la pente d'une direction — l'inverse de `direction`.
pub fn angles(d: [f32; 3]) -> (f32, f32) {
    let plat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    (d[2].atan2(d[0]), d[1].atan2(plat))
}

/// Où est le joueur, et où il regarde. **Rien d'autre.**
///
/// Pas de pivot, pas de rayon, pas de cible : le point fixe est l'œil. Tourner
/// change les angles et laisse la position ; se déplacer change la position et
/// laisse les angles. Les deux gestes sont indépendants, et c'est ce qui rend
/// le pilotage prévisible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vue {
    pub position: [f32; 3],
    /// Lacet, en radians. 0 regarde vers +X (l'Est), et croît vers +Z (le Sud).
    pub cap: f32,
    /// Tangage, en radians. Positif regarde vers le haut.
    pub pente: f32,
}

impl Vue {
    pub fn nouvelle(position: [f32; 3], cap: f32, pente: f32) -> Vue {
        Vue {
            position,
            cap,
            pente: pente.clamp(-PENTE_MAX, PENTE_MAX),
        }
    }

    /// Celle qui regarde une boîte, cadrée de trois quarts.
    ///
    /// Ce n'est PAS une orbite : on se pose au point d'où la boîte tient dans
    /// le cadre, et on regarde dans sa direction. Ensuite on vole, comme
    /// partout ailleurs.
    pub fn cadrer(min: [f32; 3], max: [f32; 3], aspect: f32) -> Vue {
        let c = Camera::cadrer(min, max, aspect);
        let (cap, pente) = angles([
            c.cible[0] - c.oeil[0],
            c.cible[1] - c.oeil[1],
            c.cible[2] - c.oeil[2],
        ]);
        Vue::nouvelle(c.oeil, cap, pente)
    }

    /// **Tourner : le regard pivote, l'œil ne bouge pas.**
    ///
    /// La pente est BORNÉE, jamais enroulée. Passer par-dessus la tête
    /// retourne l'image, et c'est le défaut qu'on met des heures à décrire
    /// alors qu'il suffit de ne pas le permettre.
    pub fn tourner(&mut self, d_cap: f32, d_pente: f32) {
        self.cap += d_cap;
        self.pente = (self.pente + d_pente).clamp(-PENTE_MAX, PENTE_MAX);
    }

    /// Avancer de `avant` le long du regard, `droite` sur le côté, `haut`
    /// vers le ciel du MONDE.
    ///
    /// La verticale est celle du monde et pas celle de la caméra : sinon
    /// « monter » en regardant le sol fait reculer, ce que personne n'attend
    /// d'un vol de créatif. Le côté est pris dans le PLAN, pour la même
    /// raison — un pas de côté en piqué ne doit pas plonger.
    pub fn deplacer(&mut self, avant: f32, droite: f32, haut: f32) {
        let d = direction(self.cap, self.pente);
        let (sc, cc) = self.cap.sin_cos();
        let cote = [sc, 0.0, -cc];
        for k in 0..3 {
            self.position[k] += d[k] * avant + cote[k] * droite;
        }
        self.position[1] += haut;
    }

    /// Le panoramique de la molette enfoncée + Maj : de côté et en hauteur,
    /// dans le plan de l'écran.
    ///
    /// Le « haut » est ici celui de l'ÉCRAN, incliné comme le regard, parce
    /// qu'un panoramique suit la souris : tirer vers le haut de l'écran doit
    /// faire monter ce qu'on voit, pas partir à la verticale quand on regarde
    /// déjà le ciel.
    pub fn glisser(&mut self, droite: f32, haut: f32) {
        let d = direction(self.cap, self.pente);
        let (sc, cc) = self.cap.sin_cos();
        let cote = [sc, 0.0, -cc];
        let vrai_haut = [-d[0] * d[1], d[0] * d[0] + d[2] * d[2], -d[2] * d[1]];
        let n = (vrai_haut[0] * vrai_haut[0]
            + vrai_haut[1] * vrai_haut[1]
            + vrai_haut[2] * vrai_haut[2])
            .sqrt()
            .max(1e-6);
        for k in 0..3 {
            self.position[k] += cote[k] * droite + vrai_haut[k] / n * haut;
        }
    }

    /// La caméra correspondante. `modele` fournit le champ de vision et les
    /// plans — ce que le pilotage n'a pas à connaître.
    pub fn camera(&self, modele: &Camera) -> Camera {
        let d = direction(self.cap, self.pente);
        Camera {
            oeil: self.position,
            cible: [
                self.position[0] + d[0],
                self.position[1] + d[1],
                self.position[2] + d[2],
            ],
            ..*modele
        }
    }
}
