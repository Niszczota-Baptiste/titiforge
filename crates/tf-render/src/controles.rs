//! Les deux façons de se déplacer, et le passage de l'une à l'autre.
//!
//! **Éditer un monde et concevoir un bâtiment ne se pilotent pas pareil.**
//! Pour transformer ce qui existe — le geste de WorldEdit et de MCEdit — on
//! VOLE dans le monde : la caméra va où l'on veut, et ce qu'on regarde suit.
//! Pour concevoir — le geste de SketchUp — on ORBITE autour de ce qu'on
//! construit : l'objet reste au centre, et c'est nous qui tournons autour.
//!
//! Ce ne sont pas deux réglages du même contrôleur. Ce sont deux modèles
//! mentaux, et vouloir les servir avec un seul donne un pilotage qui n'est bon
//! pour aucun des deux : une orbite dont le pivot glisse, ou un vol qui refuse
//! de reculer parce qu'un rayon minimum l'en empêche.
//!
//! ## Ce que le module NE fait pas
//!
//! Il ne connaît ni fenêtre, ni `winit`, ni souris : il prend des ANGLES et
//! des distances déjà décidés, et rend une `Camera`. C'est la même frontière
//! que celle du mailleur avec les packs de ressources — ce qui se teste sans
//! écran doit pouvoir se tester sans écran.
//!
//! ## Le piège du bouton
//!
//! Le bouton de bascule a l'air gratuit parce que `Camera` porte déjà un œil
//! et une cible, donc « rien à convertir ». C'est faux dans un sens : en vol,
//! la cible est un point arbitraire posé devant le nez. Orbiter autour d'elle
//! ferait pivoter l'utilisateur autour d'un point à un mètre de lui, ce qui se
//! lit « la caméra est devenue folle ». **Le pivot d'une orbite se
//! DÉCIDE** — ce qu'on regarde vraiment, ou le centre de la sélection.

use crate::camera::Camera;

/// Les deux modes de travail.
///
/// Nommés d'après ce qu'ils FONT, pas d'après les outils qu'ils imitent :
/// WorldEdit et MCEdit transforment ce qui existe, SketchUp permet de
/// concevoir. C'est la distinction de `docs/VISION.md`, et elle survivra aux
/// noms de ces trois logiciels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Transformer ce qui existe : on vole dans le monde.
    #[default]
    Edition,
    /// Concevoir : on tourne autour de ce qu'on bâtit.
    Conception,
}

/// L'inclinaison maximale, à un cheveu de la verticale.
///
/// **Pas π/2 exactement.** À la verticale pile, la direction du regard est
/// colinéaire au « haut » du monde, leur produit vectoriel est nul, et la base
/// de la vue devient dégénérée : l'image bascule d'un quart de tour sans
/// prévenir. Un demi-degré de marge coûte ce qu'on ne voit pas et évite ce
/// qu'on ne comprend pas.
pub const PENTE_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.0087;

/// Distance minimale d'une orbite.
///
/// Zéro donnerait un œil confondu avec sa cible, donc une direction nulle,
/// donc une matrice de vue pleine de `NaN` — et `NaN` ne plante pas, il
/// affiche du noir.
pub const RAYON_MIN: f32 = 0.05;

/// Le vol libre : une position, deux angles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vol {
    pub position: [f32; 3],
    /// Lacet, en radians. 0 regarde vers +X (l'Est), et croît vers +Z (le Sud).
    pub cap: f32,
    /// Tangage, en radians. Positif regarde vers le haut.
    pub pente: f32,
}

/// L'orbite : un pivot, deux angles, un rayon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbite {
    pub pivot: [f32; 3],
    pub cap: f32,
    pub pente: f32,
    pub rayon: f32,
}

/// La direction que regardent un cap et une pente.
///
/// Une seule fois, ici : les deux contrôleurs s'en servent, et deux copies de
/// la même trigonométrie finissent par diverger sur un signe — c'est arrivé
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

impl Vol {
    pub fn nouveau(position: [f32; 3], cap: f32, pente: f32) -> Vol {
        Vol {
            position,
            cap,
            pente: pente.clamp(-PENTE_MAX, PENTE_MAX),
        }
    }

    /// Tourner. La pente est BORNÉE, jamais enroulée : passer par-dessus la
    /// tête retourne l'image, et c'est le défaut qu'on met des heures à
    /// décrire alors qu'il suffit de ne pas le permettre.
    pub fn tourner(&mut self, d_cap: f32, d_pente: f32) {
        self.cap += d_cap;
        self.pente = (self.pente + d_pente).clamp(-PENTE_MAX, PENTE_MAX);
    }

    /// Avancer de `avant` le long du regard, `droite` sur le côté, `haut`
    /// vers le ciel du MONDE.
    ///
    /// La verticale est celle du monde et pas celle de la caméra : sinon
    /// « monter » en regardant le sol fait reculer, ce que personne n'attend
    /// d'un vol de créatif.
    pub fn deplacer(&mut self, avant: f32, droite: f32, haut: f32) {
        let d = direction(self.cap, self.pente);
        // Le côté est pris dans le PLAN, pour la même raison.
        let (sc, cc) = self.cap.sin_cos();
        let cote = [sc, 0.0, -cc];
        for k in 0..3 {
            self.position[k] += d[k] * avant + cote[k] * droite;
        }
        self.position[1] += haut;
    }

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

impl Orbite {
    pub fn nouvelle(pivot: [f32; 3], cap: f32, pente: f32, rayon: f32) -> Orbite {
        Orbite {
            pivot,
            cap,
            pente: pente.clamp(-PENTE_MAX, PENTE_MAX),
            rayon: rayon.max(RAYON_MIN),
        }
    }

    pub fn tourner(&mut self, d_cap: f32, d_pente: f32) {
        self.cap += d_cap;
        self.pente = (self.pente + d_pente).clamp(-PENTE_MAX, PENTE_MAX);
    }

    /// Zoomer, en FACTEUR et non en pas fixe.
    ///
    /// Un pas fixe est inutilisable aux deux bouts : trop lent pour traverser
    /// un build, et il traverse l'objet d'un cran quand on est contre. Le
    /// facteur rend le zoom identique quelle que soit l'échelle, ce qui est
    /// exactement ce qu'on veut d'un outil qui sert du bloc à la ville.
    pub fn zoomer(&mut self, facteur: f32) {
        self.rayon = (self.rayon * facteur.max(1e-3)).max(RAYON_MIN);
    }

    /// Déplacer le PIVOT dans le plan de l'écran — le panoramique.
    ///
    /// C'est le pivot qui bouge, pas l'œil : autrement le rayon changerait et
    /// le geste suivant tournerait autour d'autre chose.
    pub fn glisser(&mut self, droite: f32, haut: f32) {
        let (sc, cc) = self.cap.sin_cos();
        let cote = [sc, 0.0, -cc];
        // Le « haut » de l'écran, incliné comme le regard.
        let d = direction(self.cap, self.pente);
        let vrai_haut = [-d[0] * d[1], d[0] * d[0] + d[2] * d[2], -d[2] * d[1]];
        let n = (vrai_haut[0] * vrai_haut[0]
            + vrai_haut[1] * vrai_haut[1]
            + vrai_haut[2] * vrai_haut[2])
            .sqrt()
            .max(1e-6);
        for k in 0..3 {
            self.pivot[k] += cote[k] * droite + vrai_haut[k] / n * haut;
        }
    }

    pub fn camera(&self, modele: &Camera) -> Camera {
        // L'œil est DERRIÈRE le pivot, à `rayon` : on regarde le pivot.
        let d = direction(self.cap, self.pente);
        Camera {
            oeil: [
                self.pivot[0] - d[0] * self.rayon,
                self.pivot[1] - d[1] * self.rayon,
                self.pivot[2] - d[2] * self.rayon,
            ],
            cible: self.pivot,
            ..*modele
        }
    }
}

/// Le pilotage courant : un mode, et l'état des deux contrôleurs.
///
/// **Les deux états vivent en même temps, et c'est délibéré.** Un mode qui
/// jetterait l'autre ferait du bouton un travail : on reviendrait au vol et il
/// faudrait retrouver où l'on était. Ils sont tenus SYNCHRONES par la bascule,
/// pas par le hasard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pilotage {
    pub mode: Mode,
    pub vol: Vol,
    pub orbite: Orbite,
}

impl Pilotage {
    /// Un pilotage qui regarde une boîte, en mode Édition.
    pub fn cadrer(min: [f32; 3], max: [f32; 3], aspect: f32) -> Pilotage {
        let c = Camera::cadrer(min, max, aspect);
        let d = [
            c.cible[0] - c.oeil[0],
            c.cible[1] - c.oeil[1],
            c.cible[2] - c.oeil[2],
        ];
        let rayon = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
            .sqrt()
            .max(RAYON_MIN);
        let (cap, pente) = angles([d[0] / rayon, d[1] / rayon, d[2] / rayon]);
        Pilotage {
            mode: Mode::Edition,
            vol: Vol::nouveau(c.oeil, cap, pente),
            orbite: Orbite::nouvelle(c.cible, cap, pente, rayon),
        }
    }

    pub fn camera(&self, modele: &Camera) -> Camera {
        match self.mode {
            Mode::Edition => self.vol.camera(modele),
            Mode::Conception => self.orbite.camera(modele),
        }
    }

    /// Bascule vers `vers`, **en gardant le point de vue**.
    ///
    /// `vise` est ce que l'utilisateur regarde — le premier bloc solide sous
    /// le réticule. `None` quand le rayon ne touche rien : le pivot se pose
    /// alors devant le nez, parce qu'un pivot inventé à l'origine du monde
    /// téléporterait l'utilisateur à des milliers de blocs de là.
    ///
    /// **Une bascule ne bouge JAMAIS l'image.** C'est la propriété entière :
    /// on change de centre de rotation, pas de point de vue. Corollaire, et
    /// c'est là que la première écriture s'est trompée : le pivot est
    /// PROJETÉ sur le rayon du regard. Posé tel quel à côté, il donnait un
    /// œil recalculé ailleurs — mesuré, un saut de quarante blocs pour un
    /// pivot à quarante blocs de l'axe. Quand `vise` est un bloc sous le
    /// réticule il est déjà sur le rayon et la projection ne fait rien ; quand
    /// ce n'en est pas un, elle garde la seule chose qui compte, la DISTANCE.
    ///
    /// Pour recentrer délibérément sur autre chose — une sélection hors
    /// champ — c'est `recentrer`, qui elle bouge la caméra et le DIT.
    ///
    /// **L'aller-retour ne dérive donc pas.** Sinon le bouton coûterait un
    /// recadrage à chaque appui — le genre de dérive qu'on attribue à sa
    /// souris pendant des semaines.
    pub fn basculer(&mut self, vers: Mode, vise: Option<[f32; 3]>) {
        if vers == self.mode {
            return;
        }
        match vers {
            // Vol → orbite : le pivot est ce qu'on REGARDE, à sa distance.
            // L'œil ne bouge pas, donc l'image ne bouge pas : seul le centre
            // de rotation change, et c'était tout l'objet de la bascule.
            Mode::Conception => {
                let d = direction(self.vol.cap, self.vol.pente);
                // La distance du pivot, mesurée LE LONG DU REGARD. Un point
                // hors axe garde sa profondeur, pas sa position : c'est ce
                // qui rend la bascule totale au lieu d'imposer à l'appelant
                // un contrat qu'il violerait en silence.
                let rayon = match vise {
                    Some(v) => {
                        let w = [
                            v[0] - self.vol.position[0],
                            v[1] - self.vol.position[1],
                            v[2] - self.vol.position[2],
                        ];
                        (w[0] * d[0] + w[1] * d[1] + w[2] * d[2]).max(RAYON_MIN)
                    }
                    // Rien sous le réticule : on garde le rayon de la
                    // dernière orbite, donc l'échelle à laquelle on
                    // travaillait.
                    None => self.orbite.rayon.max(RAYON_MIN),
                };
                self.orbite = Orbite {
                    pivot: [
                        self.vol.position[0] + d[0] * rayon,
                        self.vol.position[1] + d[1] * rayon,
                        self.vol.position[2] + d[2] * rayon,
                    ],
                    // Les angles du VOL : c'est le regard qui fait foi, et
                    // c'est ce qui garantit que l'œil retombe exactement où
                    // il était.
                    cap: self.vol.cap,
                    pente: self.vol.pente,
                    rayon,
                };
            }
            // Orbite → vol : on se pose là où l'œil est déjà, en regardant
            // dans la même direction. Rien à décider.
            Mode::Edition => {
                let c = self.orbite.camera(&Camera {
                    oeil: [0.0; 3],
                    cible: [0.0; 3],
                    fov: 1.0,
                    proche: 0.1,
                    loin: 2.0,
                });
                self.vol = Vol {
                    position: c.oeil,
                    cap: self.orbite.cap,
                    pente: self.orbite.pente,
                };
            }
        }
        self.mode = vers;
    }

    /// Recentrer l'orbite sur un point, en gardant la distance.
    ///
    /// **Celle-ci bouge la caméra, et c'est voulu** — d'où un nom à part. On
    /// s'en sert pour « tourner autour de ma sélection » quand la sélection
    /// n'est pas sous le réticule : la caméra pivote pour la regarder. La
    /// confondre avec `basculer` ferait d'un bouton de mode un geste qui
    /// déplace le point de vue, ce que personne n'attend d'un bouton de mode.
    ///
    /// Le vol est tenu synchrone : revenir en Édition repartira d'où l'orbite
    /// a laissé l'œil, pas d'un état périmé.
    pub fn recentrer(&mut self, pivot: [f32; 3], rayon: Option<f32>) {
        self.orbite.pivot = pivot;
        if let Some(r) = rayon {
            self.orbite.rayon = r.max(RAYON_MIN);
        }
        let c = self.orbite.camera(&Camera {
            oeil: [0.0; 3],
            cible: [0.0; 3],
            fov: 1.0,
            proche: 0.1,
            loin: 2.0,
        });
        self.vol = Vol {
            position: c.oeil,
            cap: self.orbite.cap,
            pente: self.orbite.pente,
        };
    }
}
