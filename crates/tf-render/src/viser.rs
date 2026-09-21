//! **Quel bloc, et quelle FACE, sous le curseur.**
//!
//! Tout ce qui se fait à la souris en dépend : poser un coin de sélection,
//! poser un bloc, en casser un, pousser-tirer une face, accrocher une
//! inférence. Sans ça, une coque ne peut rien faire d'autre que voler.
//!
//! ## Pourquoi ce n'est pas « l'intersection d'un rayon et d'une boîte »
//!
//! Un monde Minefield fait quatre-vingts milliards de blocs. On ne teste pas
//! un rayon contre chacun : on MARCHE de case en case le long du rayon, en
//! sautant à chaque fois au prochain plan de grille — l'algorithme
//! d'Amanatides et Woo. Le coût est celui de la DISTANCE parcourue, pas du
//! monde, et il ne visite que des cases que le rayon traverse vraiment.
//!
//! ## Le monde arrive par une couture, jamais par un `import`
//!
//! `viser` prend un prédicat « cette case arrête-t-elle le rayon ? ». C'est la
//! même frontière que `Formes` pour le mailleur : ce qui a besoin de savoir
//! *où* vivent les données passe par là. La sélection veut arrêter sur ce qui
//! n'est pas de l'air ; un outil de terrain voudra peut-être traverser les
//! feuillages ; un greffon décidera autre chose. Aucun des trois n'a à
//! modifier ce fichier.

use tf_mesh::forme::Face;

/// Ce qu'un rayon a touché.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touche {
    /// La case ARRÊTÉE — celle qui a dit « stop ». C'est le bloc qu'on casse,
    /// qu'on remplace, dont on lit l'état.
    pub case: [i32; 3],
    /// La case d'AVANT, celle d'où le rayon venait. C'est là qu'on pose.
    ///
    /// Rendue plutôt que recalculée par l'appelant, et c'est un piège payé
    /// dans `ExeWorldEdit` : *poser et effacer ne visent pas la même case*.
    /// Un rayon touche une FACE, donc un plan entre deux cases, et laisser
    /// chacun refaire le pas de son côté donne un outil qui pose un bloc dans
    /// le mur une fois sur deux. `None` si le rayon partait déjà DANS un
    /// solide : il n'y a pas de case d'avant.
    pub avant: Option<[i32; 3]>,
    /// La face par laquelle on est ENTRÉ dans `case`, vue de l'extérieur.
    ///
    /// `None` au même cas : partant de l'intérieur d'un solide, on n'a
    /// traversé aucune face.
    pub face: Option<Face>,
    /// Distance parcourue le long du rayon, dans l'unité de `direction`
    /// — donc en blocs si elle est normalisée.
    pub distance: f32,
}

/// Le nombre de cases qu'on accepte de traverser, quoi qu'il arrive.
///
/// Un garde-fou et non une portée : la portée se donne en `portee`. Il est là
/// pour le cas dégénéré — une direction quasi nulle, un `NaN` venu d'une
/// caméra mal initialisée — où les pas deviennent infinitésimaux et la boucle
/// ne finit jamais. Une boucle qui ne finit pas fige la fenêtre, et personne
/// ne sait dire pourquoi.
pub const PAS_MAX: u32 = 16_384;

/// Marche le long d'un rayon et rend la première case qui l'arrête.
///
/// `origine` est un point MONDE en flottants (l'œil), `direction` n'a pas
/// besoin d'être normalisée mais `portee` et `distance` s'expriment dans son
/// unité — la normaliser est donc ce qu'on veut neuf fois sur dix.
///
/// `arrete` reçoit une case et dit si le rayon s'y arrête.
pub fn viser(
    origine: [f32; 3],
    direction: [f32; 3],
    portee: f32,
    arrete: &dyn Fn([i32; 3]) -> bool,
) -> Option<Touche> {
    // Une direction nulle ou non finie ne vise rien. Le dire ici plutôt que
    // de laisser les divisions produire des `NaN` : `NaN` compare faux dans
    // les DEUX sens, donc la boucle ne s'arrêterait sur aucune condition.
    if !direction.iter().all(|c| c.is_finite())
        || !origine.iter().all(|c| c.is_finite())
        || direction.iter().all(|c| *c == 0.0)
        || !portee.is_finite()
        || portee <= 0.0
    {
        return None;
    }

    // La case de départ. Division PLANCHER : le bloc −0,5 est dans la case
    // −1, pas la case 0. C'est l'invariant du dépôt, et il vaut ici comme
    // pour les régions.
    let mut case = [
        origine[0].floor() as i32,
        origine[1].floor() as i32,
        origine[2].floor() as i32,
    ];

    // On part peut-être DANS un solide — la caméra est dans un mur. Le dire
    // tout de suite : il n'y a ni face traversée ni case d'avant, et inventer
    // l'une des deux ferait poser un bloc à un endroit arbitraire.
    if arrete(case) {
        return Some(Touche {
            case,
            avant: None,
            face: None,
            distance: 0.0,
        });
    }

    let mut pas = [0i32; 3];
    // Distance le long du rayon jusqu'au prochain plan de grille, par axe.
    let mut prochain = [f32::INFINITY; 3];
    // Ce que coûte la traversée d'une case entière, par axe.
    let mut delta = [f32::INFINITY; 3];

    for k in 0..3 {
        if direction[k] > 0.0 {
            pas[k] = 1;
            delta[k] = 1.0 / direction[k];
            prochain[k] = ((case[k] + 1) as f32 - origine[k]) / direction[k];
        } else if direction[k] < 0.0 {
            pas[k] = -1;
            delta[k] = -1.0 / direction[k];
            prochain[k] = (case[k] as f32 - origine[k]) / direction[k];
        }
        // Une composante EXACTEMENT nulle laisse `INFINITY` : le rayon ne
        // franchit jamais un plan de cet axe, donc il n'est jamais le
        // minimum. C'est ce qui rend un rayon parfaitement axial — un
        // utilisateur qui regarde droit devant — juste sans cas particulier.
    }

    let mut distance = 0.0f32;
    for _ in 0..PAS_MAX {
        // L'axe dont le plan arrive le plus tôt.
        let axe = if prochain[0] < prochain[1] && prochain[0] < prochain[2] {
            0
        } else if prochain[1] < prochain[2] {
            1
        } else {
            2
        };
        if prochain[axe] > portee {
            return None;
        }
        distance = prochain[axe];
        let precedente = case;
        case[axe] += pas[axe];
        prochain[axe] += delta[axe];

        if arrete(case) {
            // La face TRAVERSÉE est celle du côté d'où l'on vient : en
            // avançant vers +X on entre par la face −X. L'inverser ferait
            // poser les blocs de l'autre côté du mur, ce qui se lit « l'outil
            // vise à côté » et ne désigne pas la cause.
            let face = match (axe, pas[axe] > 0) {
                (0, true) => Face::MoinsX,
                (0, false) => Face::PlusX,
                (1, true) => Face::MoinsY,
                (1, false) => Face::PlusY,
                (2, true) => Face::MoinsZ,
                _ => Face::PlusZ,
            };
            return Some(Touche {
                case,
                avant: Some(precedente),
                face: Some(face),
                distance,
            });
        }
    }
    // Le garde-fou a parlé : plutôt rendre « rien » qu'une case tirée au sort
    // après seize mille pas.
    let _ = distance;
    None
}

/// La direction d'un rayon partant de l'œil vers un point de l'ÉCRAN.
///
/// `ndc` est en coordonnées normalisées : −1 à gauche et en BAS, +1 à droite
/// et en haut. Le centre de l'écran est `[0, 0]`, et c'est là que vise un
/// réticule.
///
/// **Le sens vertical se dit, il ne se devine pas.** Une souris donne des
/// pixels comptés depuis le HAUT, l'espace normalisé compte depuis le bas :
/// c'est à l'appelant de faire `1 - 2 * y / hauteur`, une fois, et pas à
/// chacun de retrouver le signe. Un axe inversé donne un outil qui vise
/// symétriquement — plausible, et faux.
pub fn rayon_ecran(camera: &crate::camera::Camera, ndc: [f32; 2], aspect: f32) -> [f32; 3] {
    let avant = norm([
        camera.cible[0] - camera.oeil[0],
        camera.cible[1] - camera.oeil[1],
        camera.cible[2] - camera.oeil[2],
    ]);
    let droite = norm(croix(avant, [0.0, 1.0, 0.0]));
    let haut = croix(droite, avant);
    // La demi-hauteur du plan à une unité : la même tangente que la matrice
    // de projection. Un facteur différent ferait viser à côté du curseur, de
    // plus en plus loin du centre — le défaut qu'on croit être un décalage de
    // souris.
    let t = (camera.fov * 0.5).tan();
    let mut d = [0.0f32; 3];
    for k in 0..3 {
        d[k] = avant[k] + droite[k] * (ndc[0] * t * aspect) + haut[k] * (ndc[1] * t);
    }
    norm(d)
}

fn croix(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n < 1e-9 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / n, v[1] / n, v[2] / n]
}
