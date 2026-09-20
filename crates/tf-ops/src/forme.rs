//! Les FORMES — sphère, cylindre, pyramide — décrites comme des données.
//!
//! ## Pourquoi ce n'est pas un masque
//!
//! `masque.rs` le dit : un masque est un prédicat sur l'ÉTAT seul, jamais sur
//! la position, et c'est la condition de l'étage palette. Une forme est tout
//! l'inverse — elle ne regarde QUE la position. La mettre dans `Masque` ferait
//! retomber `//replace` à l'étage bloc.
//!
//! Une forme est donc l'autre moitié de la question : le masque dit QUELS
//! ÉTATS, la forme dit OÙ. Elles se croisent dans le plan, et chacune garde
//! son chemin rapide.
//!
//! ## Le chemin rapide d'une forme est le même que celui d'un masque
//!
//! Un masque évite d'itérer en répondant sur la PALETTE ; une forme évite
//! d'itérer en répondant sur la SECTION. Une sphère de rayon 60 occupe 52 %
//! de sa boîte englobante : les sections entièrement dedans gardent l'étage
//! palette, celles entièrement dehors ne sont pas même décodées, et seule la
//! coque paie le parcours par bloc.
//!
//! D'où `couverture` — trois réponses, jamais deux. Confondre « dehors » et
//! « partiellement dedans » ferait décoder la moitié du volume pour rien ;
//! confondre « dedans » et « partiellement » lui ferait perdre l'étage
//! palette. Chacune des trois est EXACTE, et un test le prouve en comparant
//! les 4 096 cases de chaque section au verdict annoncé.
//!
//! ## Les rayons sont EFFECTIFS
//!
//! Un rayon porté ici vaut le rayon demandé **plus un demi**, comme dans
//! WorldEdit : `//sphere 5` doit donner onze blocs de diamètre, pas dix. Les
//! constructeurs (`sphere`, `cylindre`, `pyramide`) font l'ajustement une
//! fois ; construire une variante à la main demande de le faire soi-même.

use tf_world::coords::{BBox, BlockPos, SectionPos};

/// Ce qu'une forme fait d'une section entière.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Couverture {
    /// Les 4 096 cases sont dedans. L'étage palette reste ouvert.
    Dedans,
    /// Aucune ne l'est. La section n'a même pas à être décodée.
    Dehors,
    /// Il faut regarder case par case.
    Partielle,
}

/// Un volume, décrit comme une DONNÉE.
///
/// C'est la couture d'extensibilité de `docs/VISION.md` appliquée aux formes :
/// un greffon décrira sa forme, le cœur choisira l'étage. Une API qui
/// laisserait un greffon rendre un prédicat `Fn(x, y, z)` interdirait pour
/// toujours les deux chemins rapides ci-dessus.
#[derive(Debug, Clone, PartialEq)]
pub enum Forme {
    /// Toute la sélection. Le défaut, et la seule forme qui ne coûte rien.
    Boite,
    /// `Σ (dᵢ / rᵢ)² ≤ 1`. Une sphère est un ellipsoïde à trois rayons égaux.
    Ellipsoide { centre: [i32; 3], rayons: [f64; 3] },
    /// Ellipse en XZ, hauteur franche. L'axe est Y — c'est celui qu'on veut
    /// neuf fois sur dix, et un cylindre couché se décrit comme un ellipsoïde
    /// très allongé en attendant mieux.
    Cylindre {
        centre: [i32; 3],
        rayons: [f64; 2],
        /// Distance du centre à chaque extrémité, en blocs.
        demi_hauteur: f64,
    },
    /// Pyramide à base carrée, pointe en haut — ou en bas si `renversee`.
    Pyramide {
        /// Le CENTRE DE LA BASE, pas le centre du volume.
        base: [i32; 3],
        demi_cote: f64,
        hauteur: f64,
        renversee: bool,
    },
    /// Un pavé droit, bornes INCLUSES.
    ///
    /// Utile seul (restreindre une opération à un sous-volume), mais surtout
    /// comme moitié d'une coque : `//walls` et `//faces` ne sont rien
    /// d'autre qu'un pavé moins un pavé plus petit. Les écrire comme des
    /// opérations à part aurait donné deux calculs de plus à tenir justes.
    Pave { min: [i32; 3], max: [i32; 3] },
    /// Ce qui est dans l'une sans être dans l'autre : une coque.
    ///
    /// Écrit comme une composition plutôt qu'un drapeau `creux` sur chaque
    /// variante : creuser une sphère, un cylindre et une pyramide sont trois
    /// calculs différents, et trois drapeaux finiraient par diverger. Ici il
    /// n'y a qu'une règle, et elle vaut pour toute forme présente ou future.
    Coque {
        dehors: Box<Forme>,
        dedans: Box<Forme>,
    },
}

impl Forme {
    /// Une sphère de ce rayon, à la convention de WorldEdit.
    pub fn sphere(centre: [i32; 3], rayon: f64) -> Forme {
        Forme::ellipsoide(centre, [rayon; 3])
    }

    pub fn ellipsoide(centre: [i32; 3], rayons: [f64; 3]) -> Forme {
        Forme::Ellipsoide {
            centre,
            rayons: [eff(rayons[0]), eff(rayons[1]), eff(rayons[2])],
        }
    }

    pub fn cylindre(centre: [i32; 3], rayon: f64, hauteur: f64) -> Forme {
        Forme::Cylindre {
            centre,
            rayons: [eff(rayon), eff(rayon)],
            demi_hauteur: hauteur / 2.0,
        }
    }

    /// `demi_base` est compté comme un rayon — une pyramide de demi-base 5 a
    /// une base de onze blocs, comme `//pyramid 5` dans WorldEdit. Compter en
    /// CÔTÉ donnerait un nombre pair et une pyramide sans arête centrale.
    pub fn pyramide(base: [i32; 3], demi_base: f64, hauteur: f64, renversee: bool) -> Forme {
        Forme::Pyramide {
            base,
            demi_cote: eff(demi_base),
            hauteur,
            renversee,
        }
    }

    pub fn pave(b: BBox) -> Forme {
        Forme::Pave {
            min: [b.min.x, b.min.y, b.min.z],
            max: [b.max.x, b.max.y, b.max.z],
        }
    }

    /// Les quatre parois VERTICALES d'une boîte, sans plancher ni plafond.
    ///
    /// C'est `//walls`, et c'est le sens qu'on veut : on entoure une cour,
    /// on ne l'enferme pas. Les six faces se demandent avec `faces`.
    pub fn murs(b: BBox, epaisseur: f64) -> Forme {
        let e = epaisseur.max(0.0) as i32;
        Forme::Coque {
            dehors: Box::new(Forme::pave(b)),
            // Le creux garde TOUTE la hauteur : c'est ce qui fait qu'un mur
            // n'a pas de toit.
            dedans: Box::new(Forme::Pave {
                min: [b.min.x + e, b.min.y, b.min.z + e],
                max: [b.max.x - e, b.max.y, b.max.z - e],
            }),
        }
    }

    /// Les six faces d'une boîte — `//faces`.
    pub fn faces(b: BBox, epaisseur: f64) -> Forme {
        let e = epaisseur.max(0.0) as i32;
        Forme::Coque {
            dehors: Box::new(Forme::pave(b)),
            dedans: Box::new(Forme::Pave {
                min: [b.min.x + e, b.min.y + e, b.min.z + e],
                max: [b.max.x - e, b.max.y - e, b.max.z - e],
            }),
        }
    }

    /// La même, creusée d'une coque de `epaisseur` blocs.
    ///
    /// L'intérieur est la MÊME forme, rétrécie : c'est ce qui fait qu'une
    /// sphère creuse a une coque d'épaisseur constante, et non un fond plus
    /// épais que ses flancs.
    pub fn creuse(self, epaisseur: f64) -> Forme {
        let Some(dedans) = self.retrecie(epaisseur) else {
            return self;
        };
        Forme::Coque {
            dehors: Box::new(self),
            dedans: Box::new(dedans),
        }
    }

    fn retrecie(&self, e: f64) -> Option<Forme> {
        let moins = |v: f64| (v - e).max(0.0);
        Some(match self {
            Forme::Boite | Forme::Coque { .. } => return None,
            Forme::Pave { min, max } => Forme::Pave {
                min: [min[0] + e as i32, min[1] + e as i32, min[2] + e as i32],
                max: [max[0] - e as i32, max[1] - e as i32, max[2] - e as i32],
            },
            Forme::Ellipsoide { centre, rayons } => Forme::Ellipsoide {
                centre: *centre,
                rayons: [moins(rayons[0]), moins(rayons[1]), moins(rayons[2])],
            },
            Forme::Cylindre {
                centre,
                rayons,
                demi_hauteur,
            } => Forme::Cylindre {
                centre: *centre,
                rayons: [moins(rayons[0]), moins(rayons[1])],
                demi_hauteur: moins(*demi_hauteur),
            },
            Forme::Pyramide {
                base,
                demi_cote,
                hauteur,
                renversee,
            } => Forme::Pyramide {
                // La base remonte AUSSI : sans ça, la pyramide creuse n'aurait
                // pas de plancher, et une coque ouverte par le bas n'est pas
                // une coque.
                base: [
                    base[0],
                    base[1] + if *renversee { -(e as i32) } else { e as i32 },
                    base[2],
                ],
                demi_cote: moins(*demi_cote),
                hauteur: moins(*hauteur),
                renversee: *renversee,
            },
        })
    }

    /// La boîte englobante, ou `None` pour `Boite` (qui n'en a pas : c'est la
    /// sélection qui la donne).
    ///
    /// C'est elle qu'on passe comme sélection à `appliquer` : une opération ne
    /// paie que sa PORTÉE, et la portée d'une sphère n'est pas la sélection
    /// où l'utilisateur l'a posée.
    pub fn bornes(&self) -> Option<BBox> {
        let b = |c: [i32; 3], r: [f64; 3]| {
            let d = |v: f64| v.floor().max(0.0) as i32;
            BBox::new(
                BlockPos {
                    x: c[0] - d(r[0]),
                    y: c[1] - d(r[1]),
                    z: c[2] - d(r[2]),
                },
                BlockPos {
                    x: c[0] + d(r[0]),
                    y: c[1] + d(r[1]),
                    z: c[2] + d(r[2]),
                },
            )
        };
        Some(match self {
            Forme::Boite => return None,
            Forme::Ellipsoide { centre, rayons } => b(*centre, *rayons),
            Forme::Cylindre {
                centre,
                rayons,
                demi_hauteur,
            } => b(*centre, [rayons[0], *demi_hauteur, rayons[1]]),
            Forme::Pyramide {
                base,
                demi_cote,
                hauteur,
                renversee,
            } => {
                let h = hauteur.floor().max(0.0) as i32;
                let c = demi_cote.floor().max(0.0) as i32;
                let (y0, y1) = if *renversee {
                    (base[1] - h, base[1])
                } else {
                    (base[1], base[1] + h)
                };
                BBox::new(
                    BlockPos {
                        x: base[0] - c,
                        y: y0,
                        z: base[2] - c,
                    },
                    BlockPos {
                        x: base[0] + c,
                        y: y1,
                        z: base[2] + c,
                    },
                )
            }
            Forme::Pave { min, max } => BBox::new(
                BlockPos {
                    x: min[0],
                    y: min[1],
                    z: min[2],
                },
                BlockPos {
                    x: max[0],
                    y: max[1],
                    z: max[2],
                },
            ),
            // La coque tient dans son extérieur.
            Forme::Coque { dehors, .. } => return dehors.bornes(),
        })
    }

    /// Cette case est-elle dedans ? En coordonnées MONDE.
    ///
    /// **Appelée par bloc — mais seulement sur les sections `Partielle`.** Les
    /// autres ont déjà répondu pour leurs 4 096 cases d'un coup.
    pub fn contient(&self, x: i32, y: i32, z: i32) -> bool {
        match self {
            Forme::Boite => true,
            Forme::Ellipsoide { centre, rayons } => {
                norme(
                    [
                        (x - centre[0]) as f64,
                        (y - centre[1]) as f64,
                        (z - centre[2]) as f64,
                    ],
                    *rayons,
                ) <= 1.0
            }
            Forme::Cylindre {
                centre,
                rayons,
                demi_hauteur,
            } => {
                ((y - centre[1]) as f64).abs() <= *demi_hauteur
                    && norme(
                        [(x - centre[0]) as f64, 0.0, (z - centre[2]) as f64],
                        [rayons[0], f64::INFINITY, rayons[1]],
                    ) <= 1.0
            }
            Forme::Pyramide {
                base,
                demi_cote,
                hauteur,
                renversee,
            } => {
                let dy = if *renversee {
                    (base[1] - y) as f64
                } else {
                    (y - base[1]) as f64
                };
                if dy < 0.0 || dy > *hauteur {
                    return false;
                }
                let c = demi_cote_a(*demi_cote, *hauteur, dy);
                ((x - base[0]) as f64).abs() <= c && ((z - base[2]) as f64).abs() <= c
            }
            Forme::Pave { min, max } => {
                x >= min[0]
                    && x <= max[0]
                    && y >= min[1]
                    && y <= max[1]
                    && z >= min[2]
                    && z <= max[2]
            }
            Forme::Coque { dehors, dedans } => {
                dehors.contient(x, y, z) && !dedans.contient(x, y, z)
            }
        }
    }

    /// Ce que la forme fait de cette section, en UNE réponse.
    ///
    /// Chacune des trois est exacte : le test croisé compare le verdict aux
    /// 4 096 cases. Une réponse « Dedans » fausse écrirait hors de la forme,
    /// une « Dehors » fausse laisserait un trou — et ni l'une ni l'autre ne
    /// se verrait sur une capture d'écran.
    ///
    /// **`#[inline]` n'est pas décoratif ici.** L'étage section est en O(1) :
    /// son coût entier est un appel par section, 24 576 sur une région
    /// pleine. Laissée hors ligne, cette fonction — dont le corps contient
    /// tout le calcul des cinq variantes — coûtait **+11,9 %** à `//set`,
    /// mesuré binaire contre binaire. En ligne, le cas `Boite` se réduit à un
    /// test de discriminant que l'appelant peut sortir de sa boucle.
    #[inline]
    pub fn couverture(&self, pos: SectionPos) -> Couverture {
        // Le cas par défaut avant tout calcul : `Forme::Boite` ne doit même
        // pas payer la conversion de la section en coordonnées monde.
        if matches!(self, Forme::Boite) {
            return Couverture::Dedans;
        }
        let o = pos.min_block();
        self.couverture_boite([o.x, o.y, o.z], [o.x + 15, o.y + 15, o.z + 15])
    }

    /// Le vrai calcul, gardé HORS LIGNE : c'est lui qui est gros, et il ne
    /// sert qu'aux sections d'une forme réelle.
    #[inline(never)]
    fn couverture_boite(&self, lo: [i32; 3], hi: [i32; 3]) -> Couverture {
        match self {
            Forme::Boite => Couverture::Dedans,
            Forme::Ellipsoide { centre, rayons } => ellipsoide_couverture(*centre, *rayons, lo, hi),
            Forme::Cylindre {
                centre,
                rayons,
                demi_hauteur,
            } => {
                let plat = ellipsoide_couverture(
                    [centre[0], 0, centre[2]],
                    [rayons[0], f64::INFINITY, rayons[1]],
                    [lo[0], 0, lo[2]],
                    [hi[0], 0, hi[2]],
                );
                let vertical = intervalle(
                    centre[1] as f64 - demi_hauteur,
                    centre[1] as f64 + demi_hauteur,
                    lo[1],
                    hi[1],
                );
                croiser(plat, vertical)
            }
            Forme::Pyramide {
                base,
                demi_cote,
                hauteur,
                renversee,
            } => {
                // Les hauteurs relatives de la tranche, dans le sens de la
                // pyramide. `dy` croît de la base vers la pointe.
                let (dy_bas, dy_haut) = if *renversee {
                    ((base[1] - hi[1]) as f64, (base[1] - lo[1]) as f64)
                } else {
                    ((lo[1] - base[1]) as f64, (hi[1] - base[1]) as f64)
                };
                if dy_haut < 0.0 || dy_bas > *hauteur {
                    return Couverture::Dehors;
                }
                // La plus LARGE des demi-côtés de la tranche est à son `dy` le
                // plus bas, la plus étroite au plus haut : le côté décroît.
                let large = demi_cote_a(*demi_cote, *hauteur, dy_bas.max(0.0));
                let etroit = demi_cote_a(*demi_cote, *hauteur, dy_haut.min(*hauteur));
                let dehors_xz = carre(base[0], base[2], large, lo, hi) == Couverture::Dehors;
                if dehors_xz {
                    return Couverture::Dehors;
                }
                let toute_la_tranche = dy_bas >= 0.0 && dy_haut <= *hauteur;
                if toute_la_tranche && carre(base[0], base[2], etroit, lo, hi) == Couverture::Dedans
                {
                    return Couverture::Dedans;
                }
                Couverture::Partielle
            }
            Forme::Pave { min, max } => {
                let mut r = Couverture::Dedans;
                for i in 0..3 {
                    r = croiser(r, intervalle(min[i] as f64, max[i] as f64, lo[i], hi[i]));
                }
                r
            }
            Forme::Coque { dehors, dedans } => {
                match (
                    dehors.couverture_boite(lo, hi),
                    dedans.couverture_boite(lo, hi),
                ) {
                    // Hors de l'extérieur, ou entièrement dans le trou.
                    (Couverture::Dehors, _) | (_, Couverture::Dedans) => Couverture::Dehors,
                    (Couverture::Dedans, Couverture::Dehors) => Couverture::Dedans,
                    _ => Couverture::Partielle,
                }
            }
        }
    }
}

/// Le rayon EFFECTIF : celui qu'on demande, plus un demi.
///
/// `//sphere 5` doit donner onze blocs de diamètre. Sans le demi, on en donne
/// dix et la sphère paraît décentrée — c'est la convention de WorldEdit, et
/// s'en écarter ferait que le même chiffre ne donne pas le même bâtiment.
fn eff(r: f64) -> f64 {
    if r <= 0.0 {
        0.0
    } else {
        r + 0.5
    }
}

fn norme(d: [f64; 3], r: [f64; 3]) -> f64 {
    let un = |d: f64, r: f64| {
        if r.is_infinite() {
            0.0
        } else if r <= 0.0 {
            // Un rayon nul n'accepte que le centre exact, et surtout ne rend
            // pas NaN : `0 / 0` propagerait le NaN dans la comparaison, qui
            // est FAUSSE dans les deux sens — la forme serait vide ET pleine.
            if d == 0.0 {
                0.0
            } else {
                f64::INFINITY
            }
        } else {
            let q = d / r;
            q * q
        }
    };
    un(d[0], r[0]) + un(d[1], r[1]) + un(d[2], r[2])
}

/// Le demi-côté d'une pyramide à la hauteur `dy` au-dessus de sa base.
fn demi_cote_a(demi_cote: f64, hauteur: f64, dy: f64) -> f64 {
    if hauteur <= 0.0 {
        return demi_cote;
    }
    (demi_cote * (1.0 - dy / hauteur)).max(0.0)
}

/// Couverture d'une boîte par un ellipsoïde — les deux bornes à la fois.
///
/// Le point le plus PROCHE du centre décide du « dehors », le plus LOIN du
/// « dedans ». Les deux sont des coins de la boîte (ou sa projection) parce
/// qu'un ellipsoïde aligné sur les axes est séparable : la distance se calcule
/// axe par axe.
fn ellipsoide_couverture(
    centre: [i32; 3],
    rayons: [f64; 3],
    lo: [i32; 3],
    hi: [i32; 3],
) -> Couverture {
    let mut proche = [0.0f64; 3];
    let mut loin = [0.0f64; 3];
    for i in 0..3 {
        let c = centre[i] as f64;
        let (a, b) = (lo[i] as f64, hi[i] as f64);
        proche[i] = if c < a {
            a - c
        } else if c > b {
            c - b
        } else {
            0.0
        };
        loin[i] = (a - c).abs().max((b - c).abs());
    }
    if norme(proche, rayons) > 1.0 {
        Couverture::Dehors
    } else if norme(loin, rayons) <= 1.0 {
        Couverture::Dedans
    } else {
        Couverture::Partielle
    }
}

/// Couverture d'une boîte par un intervalle sur Y.
fn intervalle(y0: f64, y1: f64, lo: i32, hi: i32) -> Couverture {
    let (a, b) = (lo as f64, hi as f64);
    if b < y0 || a > y1 {
        Couverture::Dehors
    } else if a >= y0 && b <= y1 {
        Couverture::Dedans
    } else {
        Couverture::Partielle
    }
}

/// Couverture d'une boîte par un carré centré, en XZ.
fn carre(cx: i32, cz: i32, demi: f64, lo: [i32; 3], hi: [i32; 3]) -> Couverture {
    let x = intervalle(cx as f64 - demi, cx as f64 + demi, lo[0], hi[0]);
    let z = intervalle(cz as f64 - demi, cz as f64 + demi, lo[2], hi[2]);
    croiser(x, z)
}

/// L'INTERSECTION de deux couvertures : dedans partout, ou rien nulle part.
fn croiser(a: Couverture, b: Couverture) -> Couverture {
    match (a, b) {
        (Couverture::Dehors, _) | (_, Couverture::Dehors) => Couverture::Dehors,
        (Couverture::Dedans, Couverture::Dedans) => Couverture::Dedans,
        _ => Couverture::Partielle,
    }
}
