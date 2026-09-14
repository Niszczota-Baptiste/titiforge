//! L'atlas — et c'est un **TABLEAU de textures**, pas une planche.
//!
//! Le maillage est glouton : un quad couvre plusieurs blocs, et sa texture
//! doit donc se RÉPÉTER. Sur une planche d'atlas, `fract` sort de la tuile et
//! mord sur la voisine — d'où la frange classique de la mauvaise texture au
//! bord des faces, que tout le monde finit par découvrir en production.
//!
//! Une texture-tableau (`DataArrayTexture` côté three.js, un
//! `TextureViewDimension::D2Array` côté wgpu) donne **une couche par tuile** :
//! la répétition ne peut pas en sortir. Il n'y a rien à border, rien à
//! rogner, et aucun réglage de mip à négocier.

use std::collections::BTreeMap;

use crate::source::Source;
use crate::texture::{self, Tuile};

/// Côté maximal d'une couche.
///
/// Un pack HD en 512 donnerait 3 556 couches de 512 × 512 × 4 = **3,7 Go**.
/// Le plafond n'est pas une prudence : c'est la différence entre une
/// application qui ouvre un pack et une qui meurt en l'ouvrant.
pub const COTE_MAX: u32 = 128;

#[derive(Debug, Clone)]
pub struct Couche {
    pub nom: String,
    /// Nombre d'images d'animation de la texture d'origine.
    pub images: u32,
    pub transparente: bool,
    pub moyenne: [f32; 3],
}

/// Un tableau de textures : toutes les couches au même côté.
#[derive(Debug, Default)]
pub struct Atlas {
    pub cote: u32,
    /// RGBA8, `cote * cote * 4` octets par couche, dans l'ordre de `couches`.
    pub pixels: Vec<u8>,
    pub couches: Vec<Couche>,
    index: BTreeMap<String, u32>,
    /// Les textures qu'on n'a pas su lire, pour que le trou se VOIE.
    pub manquantes: Vec<String>,
}

impl Atlas {
    /// Bâtit le tableau depuis une liste de noms de texture.
    ///
    /// `chemin` dit où les chercher : un pack Minecraft et le codex du site ne
    /// les rangent pas pareil.
    pub fn batir<S: Source + ?Sized>(
        src: &S,
        noms: impl IntoIterator<Item = String>,
        chemin: &dyn Fn(&str) -> Vec<String>,
    ) -> Atlas {
        let mut lues: Vec<(String, Tuile)> = Vec::new();
        let mut manquantes = Vec::new();
        let mut vus: BTreeMap<String, ()> = BTreeMap::new();

        for nom in noms {
            if vus.insert(nom.clone(), ()).is_some() {
                continue;
            }
            let mut trouvee = None;
            for c in chemin(&nom) {
                if let Ok(t) = texture::lire(src, &c) {
                    trouvee = Some(t);
                    break;
                }
            }
            match trouvee {
                Some(t) => lues.push((nom, t)),
                None => manquantes.push(nom),
            }
        }

        // Le côté du tableau est le PLUS GRAND rencontré, plafonné. On agrandit
        // les petites plutôt que de réduire les grandes : agrandir au plus
        // proche voisin ne perd aucun pixel, réduire en perd la moitié.
        let cote = lues
            .iter()
            .map(|(_, t)| t.cote)
            .max()
            .unwrap_or(16)
            .clamp(1, COTE_MAX);

        let par_couche = (cote * cote * 4) as usize;
        let mut pixels = Vec::with_capacity(par_couche * lues.len());
        let mut couches = Vec::with_capacity(lues.len());
        let mut index = BTreeMap::new();

        for (nom, t) in lues {
            // Seule la PREMIÈRE image d'une animation entre dans le tableau.
            // Les trente-deux vues du feu empilées sur une face donneraient un
            // bloc écrasé ; l'animation se joue en changeant de couche, et
            // c'est le travail du rendu, pas de l'atlas.
            let img = t.image(0);
            let redim = if t.cote == cote {
                img.to_vec()
            } else {
                Tuile::agrandir(img, t.cote, cote)
            };
            index.insert(nom.clone(), couches.len() as u32);
            couches.push(Couche {
                nom,
                images: t.images,
                transparente: t.transparente,
                moyenne: t.moyenne,
            });
            pixels.extend_from_slice(&redim);
        }

        Atlas {
            cote,
            pixels,
            couches,
            index,
            manquantes,
        }
    }

    pub fn couche(&self, nom: &str) -> Option<u32> {
        self.index.get(nom).copied()
    }

    pub fn len(&self) -> usize {
        self.couches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.couches.is_empty()
    }

    pub fn octets(&self) -> usize {
        self.pixels.len()
    }

    /// Le facteur par lequel multiplier une couleur de teinte pour cette
    /// couche.
    ///
    /// Les textures teintées du jeu sont GRISES : `grass_block_top.png` vaut
    /// (147, 147, 147). En les multipliant telles quelles par un vert de
    /// biome, le sol sort deux fois trop sombre. On divise donc par la moyenne
    /// RÉELLE de la tuile — et le résultat est BORNÉ à 1 : une teinte ne peut
    /// qu'assombrir, c'est une multiplication, la même limite que dans le jeu.
    pub fn facteur_de_teinte(&self, couche: u32) -> [f32; 3] {
        let Some(c) = self.couches.get(couche as usize) else {
            return [1.0; 3];
        };
        let mut out = [1.0f32; 3];
        for (sortie, moyenne) in out.iter_mut().zip(c.moyenne.iter()) {
            if *moyenne > 0.5 {
                // Plafonné : une tuile presque noire ferait exploser le
                // facteur, et le bloc sortirait fluorescent.
                *sortie = (255.0 / moyenne).min(8.0);
            }
        }
        out
    }

    /// Les couches qui portent de la transparence — celles dont le bloc ne peut
    /// pas boucher sa case.
    pub fn transparentes(&self) -> Vec<&str> {
        self.couches
            .iter()
            .filter(|c| c.transparente)
            .map(|c| c.nom.as_str())
            .collect()
    }
}
