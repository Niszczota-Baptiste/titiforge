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
use crate::maillage::{Instance, Instances, Maillage, Quad};
use crate::voisinage::{Voisinage, COTE};

use crate::glouton::axes_du_plan;
use crate::opacite::Opacite;

pub fn mailler<F: Formes + ?Sized>(v: &Voisinage, f: &F, out: &mut Maillage) {
    mailler_avec(v, f, &Opacite::relever(v, f), out)
}

pub fn mailler_avec<F: Formes + ?Sized>(v: &Voisinage, f: &F, op: &Opacite, out: &mut Maillage) {
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
                    voisin_opaque[face.indice()] = op.est(x + p[0], y + p[1], z + p[2]);
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
                            c.max[axe]
                        } else {
                            c.min[axe]
                        };
                        let mut min = [0.0f32; 3];
                        min[axe] = profondeur;
                        min[au] = c.min[au];
                        min[av] = c.min[av];
                        let taille = [c.max[au] - c.min[au], c.max[av] - c.min[av]];
                        // Un cuboïde plat sur cet axe n'a pas de face à
                        // montrer : l'émettre donnerait un quad d'aire nulle,
                        // invisible et facturé.
                        if taille[0] == 0.0 || taille[1] == 0.0 {
                            continue;
                        }
                        out.quads.push(Quad {
                            min: [
                                x as f32 * 16.0 + min[0],
                                y as f32 * 16.0 + min[1],
                                z as f32 * 16.0 + min[2],
                            ],
                            taille,
                            face,
                            // Ici, aucune fusion : chaque cuboïde sort avec
                            // le biome de SA case, sans compromis à faire.
                            // Zéro pour un état non teinté quand même — les
                            // deux chemins de la passe de modèles doivent
                            // répondre la même chose, sinon comparer l'un à
                            // l'autre ne prouve plus rien.
                            biome: if f.teinte_biome(id) {
                                v.biome(x, y, z)
                            } else {
                                0
                            },
                            id,
                        });
                        out.quads_modele += 1;
                    }
                }
            }
        }
    }
}

/// La même passe, en **instances** : une pose par bloc-modèle, sans géométrie.
///
/// Mesuré face à `mailler` sur un build Minefield : le rapport est de l'ordre
/// de dix-sept pour un en nombre d'éléments, et le temps suit — il n'y a plus
/// de boucle sur les cuboïdes, donc plus rien qui dépende de la complexité du
/// modèle. Un bloc à 82 cuboïdes coûte exactement ce que coûte une dalle.
///
/// Ce que ça déplace : le masquage. Les faces n'étant plus émises, elles ne
/// peuvent plus être supprimées ici ; l'instance porte donc l'opacité de ses
/// six voisins et le shader tranche. Le travail devient proportionnel au
/// nombre de BLOCS, pas au nombre de faces.
pub fn instancier<F: Formes + ?Sized>(v: &Voisinage, f: &F, out: &mut Instances) {
    instancier_avec(v, f, &Opacite::relever(v, f), out)
}

pub fn instancier_avec<F: Formes + ?Sized>(
    v: &Voisinage,
    f: &F,
    op: &Opacite,
    out: &mut Instances,
) {
    let n = COTE as i32;
    for y in 0..n {
        for z in 0..n {
            for x in 0..n {
                let id = v.get(x, y, z);
                if f.est_air(id) || f.opaque(id) {
                    continue;
                }
                if f.cuboides(id).is_empty() {
                    continue;
                }
                let mut voisins = 0u8;
                for face in FACES {
                    let p = face.pas();
                    if op.est(x + p[0], y + p[1], z + p[2]) {
                        voisins |= face.bit();
                    }
                }
                out.poses.push(Instance {
                    pos: [x as u8, y as u8, z as u8],
                    voisins_opaques: voisins,
                    id,
                    // **Zéro pour un état non teinté**, la même règle que la
                    // clé de fusion gloutonne. Ici rien ne fusionne, donc le
                    // biome ne coûterait pas un quad de plus — mais il
                    // coûterait une COPIE de la géométrie du modèle par biome
                    // de la scène, puisque la table du rendu se mémoïse sur
                    // `(état, biome)`. Un escalier n'a pas de couleur de
                    // biome ; il ne doit pas payer comme s'il en avait une.
                    biome: if f.teinte_biome(id) {
                        v.biome(x, y, z)
                    } else {
                        0
                    },
                });
            }
        }
    }
}
