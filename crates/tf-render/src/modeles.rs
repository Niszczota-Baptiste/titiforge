//! L'arène des blocs-MODÈLES : une POSE par bloc, la géométrie une seule fois.
//!
//! Sur la cible Minefield, deux tiers du catalogue ne sont pas des cubes :
//! escaliers, dalles, chaises, vases, lanternes. La passe gloutonne ne les
//! voit pas, et jusqu'ici rien ne les dessinait — sur la première capture
//! d'une vraie save, 3 957 blocs manquaient à l'appel sans que l'image ne le
//! dise.
//!
//! On ne peut pas les ajouter en quads : mesuré dans `tf-mesh`, 349 000
//! blocs-modèles produisent **5,8 millions de quads**, neuf dixièmes du
//! maillage. Or ces quads sont la MÊME géométrie répétée — deux dalles de
//! chêne côte à côte n'ont pas deux modèles, elles ont deux positions.
//!
//! Alors la géométrie vit UNE fois, dans `faces`, indexée par état ; et un
//! bloc posé ne pèse que sa `Pose`. Le GPU recolle les deux.
//!
//! ## Un seul appel de dessin, et comment
//!
//! Chaque pose a un nombre de faces DIFFÉRENT — une dalle en a six, un sac de
//! friandises Minefield en a jusqu'à 492. On ne peut donc pas dessiner « n
//! faces par instance ».
//!
//! Trois façons de s'en sortir, et on prend la troisième :
//!
//! 1. aplatir côté processeur, une instance par face — c'est exactement ce que
//!    la `Pose` existe pour éviter ;
//! 2. un appel par nombre de faces distinct — une quinzaine d'appels, alors
//!    que la cible du projet est **moins de cinq** ;
//! 3. une somme préfixe : la pose *i* sait à quel rang commence sa première
//!    face dans le flot global. On dessine `F` instances d'un quad, et le
//!    sommet retrouve sa pose par **recherche dichotomique**. Un appel, zéro
//!    octet par face, vingt itérations sur un million de poses.

use bytemuck::{Pod, Zeroable};
use tf_anvil::StateId;
use tf_mesh::forme::{Cuboide, FACES};
use tf_mesh::{Adresse, Chantier};

/// Une face d'un cuboïde de modèle. Partagée par tous les blocs de cet état.
///
/// **64 octets, et ils vivent une seule fois.** Une centaine de kilo-octets
/// pour tout le catalogue d'une scène : c'est la table, pas la géométrie.
///
/// Les bornes sont en `[f32; 4]` et non `[f32; 3]`, et ce n'est pas du
/// gaspillage : **un `vec3<f32>` s'aligne sur SEIZE octets en WGSL.** Une
/// structure Rust en `[f32; 3]` mise en face décale tout ce qui suit d'un
/// champ sur deux, et le shader lit des bornes prises au hasard dans la table
/// voisine — mesuré, ça sort en traînées qui filent à l'infini. Le `vec4`
/// rend la correspondance ÉVIDENTE plutôt que de la faire reposer sur des
/// règles de bourrage qu'on relit mal. Un test compare les deux tailles.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct FaceModele {
    /// Bornes du cuboïde, en seizièmes, LOCALES au bloc. Le quatrième
    /// composant est du bourrage.
    pub min: [f32; 4],
    pub max: [f32; 4],
    /// Les uv, en seizièmes.
    pub uv: [f32; 4],
    /// 0 = −X, 1 = +X, 2 = −Y, 3 = +Y, 4 = −Z, 5 = +Z.
    pub face: u32,
    pub couche: u32,
    pub teinte: u32,
    /// 1 si la face porte `cullface` ET touche le bord du bloc de ce côté.
    ///
    /// Les deux conditions sont pesées ICI, une fois par état, plutôt que par
    /// bloc dans le shader : une face au milieu du bloc reste visible quoi
    /// qu'il y ait à côté, et c'est une propriété du modèle.
    pub cullable: u32,
}

/// Un bloc-modèle posé. **16 octets.**
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct Pose {
    /// `x | y << 8 | z << 16 | voisins_opaques << 24`, le tout local à la
    /// section.
    pub local: u32,
    /// Quelle tranche — donc quelle origine de section.
    pub section: u32,
    /// Rang de sa PREMIÈRE face dans le flot global. C'est la somme préfixe,
    /// et c'est ce que le shader dichotomise.
    pub debut_face: u32,
    /// Où commencent les faces de son modèle dans `faces`.
    pub debut_modele: u32,
}

/// Les origines de section, une par tranche de poses.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct Origine {
    /// Coin de la section, en seizièmes, en monde. Quatrième composant :
    /// bourrage, pour la même raison d'alignement que `FaceModele`.
    pub position: [f32; 4],
}

/// Tout ce que la passe de modèles envoie au GPU.
#[derive(Debug, Default)]
pub struct AreneModeles {
    pub faces: Vec<FaceModele>,
    pub poses: Vec<Pose>,
    /// Les tranches, pour un remaillage partiel plus tard.
    pub tranches: Vec<(Adresse, u32, u32)>,
    /// Total des faces à dessiner. C'est le nombre d'INSTANCES.
    pub faces_a_dessiner: u32,
}

impl AreneModeles {
    pub fn is_empty(&self) -> bool {
        self.faces_a_dessiner == 0
    }

    pub fn octets(&self) -> usize {
        self.faces.len() * std::mem::size_of::<FaceModele>()
            + self.poses.len() * std::mem::size_of::<Pose>()
    }

    /// Empile la passe de modèles d'un chantier.
    ///
    /// `modele` rend, pour un état ET UN BIOME, ses cuboïdes et l'habillage de
    /// chacun. Un état qui n'en a pas ne pose rien — et une pose sans face
    /// serait une instance qui ne dessine rien, donc du travail pur.
    ///
    /// **Le biome entre dans la clé de la table, pas dans la pose.** Une
    /// `FaceModele` porte déjà sa teinte ; ce qu'il lui manquait, c'est de
    /// pouvoir en avoir une par biome. La table est mémoïsée, donc il suffit
    /// de la mémoïser sur `(état, biome)` : la pose reste à SEIZE octets, le
    /// shader ne change pas d'une ligne, et il n'y a ni nouveau tampon ni
    /// nouvel empaquetage à tenir juste — c'est-à-dire aucun des trois
    /// endroits où cette chose se serait cassée en silence.
    ///
    /// Ce que ça coûte est borné par le zéro que le mailleur écrit : `biome`
    /// vaut 0 pour tout état qui ne prend pas la couleur de son biome, donc la
    /// quasi-totalité du catalogue garde UNE table, exactement comme avant.
    /// Seuls les feuillages et les vignes se dupliquent, et seulement autant
    /// de fois qu'il y a de biomes où ils POUSSENT.
    pub fn depuis(
        chantier: &Chantier,
        modele: &dyn Fn(StateId, StateId) -> Vec<FaceModele>,
    ) -> AreneModeles {
        let mut a = AreneModeles::default();
        // La géométrie d'un état n'est construite QU'UNE fois par biome où il
        // se teinte — une seule fois tout court pour le reste du catalogue,
        // quel que soit le nombre de blocs qui la portent. C'est tout
        // l'intérêt.
        let mut connus: std::collections::HashMap<(StateId, StateId), (u32, u32)> =
            std::collections::HashMap::new();

        for (section, lot) in chantier.lots.iter().enumerate() {
            let debut_pose = a.poses.len() as u32;
            for p in &lot.poses.poses {
                let (debut_modele, nombre) = *connus.entry((p.id, p.biome)).or_insert_with(|| {
                    let f = modele(p.id, p.biome);
                    let debut = a.faces.len() as u32;
                    a.faces.extend(f);
                    (debut, a.faces.len() as u32 - debut)
                });
                if nombre == 0 {
                    continue;
                }
                a.poses.push(Pose {
                    local: p.pos[0] as u32
                        | (p.pos[1] as u32) << 8
                        | (p.pos[2] as u32) << 16
                        | (p.voisins_opaques as u32) << 24,
                    section: section as u32,
                    debut_face: a.faces_a_dessiner,
                    debut_modele,
                });
                a.faces_a_dessiner += nombre;
            }
            if a.poses.len() as u32 == debut_pose {
                continue; // section sans bloc-modèle : rien à noter
            }
            a.tranches
                .push((lot.adresse, debut_pose, a.poses.len() as u32 - debut_pose));
        }
        a
    }

    /// La même, pour un appelant qui n'a pas de biomes.
    ///
    /// Bancs et tests de géométrie : leur catalogue n'est pas teinté, donc le
    /// mailleur leur écrit `biome = 0` partout et la table se mémoïse sur
    /// l'état seul, au bit près comme avant que les biomes existent. La porte
    /// est là pour le DIRE, pas pour offrir un second comportement.
    pub fn sans_biome(
        chantier: &Chantier,
        modele: &dyn Fn(StateId) -> Vec<FaceModele>,
    ) -> AreneModeles {
        AreneModeles::depuis(chantier, &|id, _| modele(id))
    }
}

/// L'habillage des six faces d'un cuboïde : couche d'atlas, teinte, uv.
///
/// Un tuple et non la structure de `tf-assets` : le rendu n'a pas à dépendre
/// d'un lecteur de packs pour savoir ce qu'est une couche de texture. C'est la
/// même raison qui garde `Formes` en trait plutôt qu'en table concrète.
pub type HabillageFaces = [(u32, [f32; 3], [f32; 4]); 6];

/// L'origine de chaque lot du chantier, dans l'ORDRE DES LOTS.
///
/// Une seule table pour les deux passes. Deux se décaleraient le jour où l'une
/// saute une section vide — et tout un pan du build se dessinerait ailleurs,
/// sans la moindre erreur. L'index d'une section EST son rang de lot ; c'est
/// la seule chose que les deux arènes ont à partager, et un test le fige.
pub fn origines(chantier: &Chantier) -> Vec<Origine> {
    chantier
        .lots
        .iter()
        .map(|lot| {
            let [x, y, z] = lot.origine();
            Origine {
                position: [x as f32 * 16.0, y as f32 * 16.0, z as f32 * 16.0, 0.0],
            }
        })
        .collect()
}

/// Les faces d'un état, prêtes pour le GPU.
///
/// `cuboides` et `habillage` viennent du même parcours (`table_rendu`), donc
/// le n-ième habillage va avec le n-ième cuboïde — structurellement, pas par
/// convention.
pub fn faces_de(cuboides: &[Cuboide], habillage: &[HabillageFaces]) -> Vec<FaceModele> {
    let mut out = Vec::new();
    for (c, hab) in cuboides.iter().zip(habillage) {
        for f in FACES {
            if c.faces & f.bit() == 0 {
                continue;
            }
            let (couche, teinte, uv) = hab[f.indice()];
            out.push(FaceModele {
                min: [c.min[0], c.min[1], c.min[2], 0.0],
                max: [c.max[0], c.max[1], c.max[2], 0.0],
                uv,
                face: f as u32,
                couche,
                teinte: crate::arene::en_rgba8(teinte),
                // `cull` ne porte que les faces qui DÉCLARENT `cullface`, et
                // seules celles à ras du bord peuvent être masquées : une face
                // au milieu du bloc reste visible quoi qu'il y ait à côté.
                cullable: u32::from(c.cull & f.bit() != 0 && c.au_bord(f)),
            });
        }
    }
    out
}
