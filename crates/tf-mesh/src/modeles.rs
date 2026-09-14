//! La passe **modèles** : tout ce qui ne bouche pas sa case.
//!
//! Escaliers, dalles, chaises, vases, tombes. Sur la cible Minefield c'est
//! **deux tiers du catalogue**, et près de la moitié de la géométrie d'un
//! build : 2,87 M de blocs-modèles portant 9,46 M de cuboïdes, contre 28,7 M de
//! cubes pleins qui se fondent en quads (`docs/fixtures.md`). Ce n'est donc pas
//! un repli pour quelques escaliers — c'est un chemin principal, et il se
//! mesure comme tel.
//!
//! Chaque cuboïde émet les faces que son modèle DÉCLARE. Une face n'est
//! masquée que si elle porte `cullface`, qu'elle est à ras du bord du bloc, et
//! que le voisin de ce côté est opaque. Les trois conditions comptent : une
//! face au milieu du bloc reste visible quoi qu'il y ait à côté.

use crate::forme::{Formes, FACES};
use crate::maillage::{Maillage, Quad};
use crate::voisinage::{Voisinage, COTE};

use crate::glouton::axes_du_plan;

pub fn mailler(v: &Voisinage, f: &dyn Formes, out: &mut Maillage) {
    let n = COTE as i32;
    for y in 0..n {
        for z in 0..n {
            for x in 0..n {
                let id = v.get(x, y, z);
                if f.est_air(id) || f.opaque(id) {
                    continue;
                }
                let cuboides = f.cuboides(id);
                if cuboides.is_empty() {
                    continue;
                }
                // Les six voisins, lus UNE fois par bloc. Les relire par
                // cuboïde multiplierait les lectures par le nombre de
                // cuboïdes — jusqu'à 82 pour un seul bloc.
                let mut voisin_opaque = [false; 6];
                for face in FACES {
                    let p = face.pas();
                    voisin_opaque[face.indice()] = f.opaque(v.get(x + p[0], y + p[1], z + p[2]));
                }

                for c in cuboides {
                    for face in FACES {
                        if c.faces & face.bit() == 0 {
                            continue;
                        }
                        if c.cull & face.bit() != 0
                            && c.au_bord(face)
                            && voisin_opaque[face.indice()]
                        {
                            continue;
                        }
                        let axe = face.axe();
                        let (au, av) = axes_du_plan(axe);
                        let profondeur = if face.positif() {
                            c.max[axe] as i32
                        } else {
                            c.min[axe] as i32
                        };
                        let mut min = [0i32; 3];
                        min[axe] = profondeur;
                        min[au] = c.min[au] as i32;
                        min[av] = c.min[av] as i32;
                        let taille = [
                            (c.max[au] - c.min[au]) as i16,
                            (c.max[av] - c.min[av]) as i16,
                        ];
                        // Un cuboïde plat sur cet axe n'a pas de face à
                        // montrer : l'émettre donnerait un quad d'aire nulle,
                        // invisible et facturé.
                        if taille[0] == 0 || taille[1] == 0 {
                            continue;
                        }
                        out.quads.push(Quad {
                            min: [
                                (x * 16 + min[0]) as i16,
                                (y * 16 + min[1]) as i16,
                                (z * 16 + min[2]) as i16,
                            ],
                            taille,
                            face,
                            id,
                        });
                        out.quads_modele += 1;
                    }
                }
            }
        }
    }
}
