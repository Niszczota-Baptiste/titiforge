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

/// Ce qu'une extension d'atlas a fait.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ajout {
    /// Couches réellement ajoutées.
    pub ajoutees: usize,
    /// Une texture neuve dépasse le côté courant : rien n'a été ajouté, et
    /// l'atlas doit être REBÂTI pour ne pas la réduire. Porte son nom, parce
    /// qu'un refus qui ne dit pas de quoi il parle envoie chercher ailleurs.
    pub trop_grande: Option<String>,
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
        let (lues, manquantes) = Self::lire_tuiles(src, noms, chemin, &BTreeMap::new());

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

    /// Lit les tuiles d'une liste de noms, en sautant celles que `deja`
    /// connaît, et rend aussi ce qu'on n'a pas su lire.
    ///
    /// Écrite UNE fois parce que [`Atlas::batir`] et [`Atlas::etendre`] en ont
    /// toutes deux besoin. Ce dépôt a payé QUATRE fois le piège des deux
    /// implémentations d'une même règle qui divergent ; la cinquième aurait
    /// porté sur l'ordre des chemins d'un pack, donc sur quelle texture gagne
    /// quand deux packs en déclarent une du même nom.
    fn lire_tuiles<S: Source + ?Sized>(
        src: &S,
        noms: impl IntoIterator<Item = String>,
        chemin: &dyn Fn(&str) -> Vec<String>,
        deja: &BTreeMap<String, u32>,
    ) -> (Vec<(String, Tuile)>, Vec<String>) {
        let mut lues: Vec<(String, Tuile)> = Vec::new();
        let mut manquantes = Vec::new();
        let mut vus: BTreeMap<String, ()> = BTreeMap::new();

        for nom in noms {
            if deja.contains_key(&nom) || vus.insert(nom.clone(), ()).is_some() {
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
        (lues, manquantes)
    }

    /// **Ajoute des couches sans toucher aux existantes.**
    ///
    /// C'est ce qui évite de recharger la ZONE quand un bloc jamais vu
    /// apparaît. Mesuré sur une zone de 64 chunks : poser un bloc connu coûte
    /// 0,7 ms, poser un bloc neuf en coûtait **22,3** — × 33, parce que tout
    /// était rebâti. Et le coût du rechargement est en O(zone) : sur une
    /// région bâtie il vaut 867 ms, donc près d'une seconde de fenêtre figée
    /// pour avoir posé UN bloc. C'est le `warmup(extent)` d'`ExeWorldEdit`,
    /// sous un autre nom.
    ///
    /// Les indices de couche déjà attribués ne bougent PAS : c'est la
    /// propriété qui rend l'extension sûre, puisque le maillage déjà produit
    /// les porte. Un test l'exige.
    ///
    /// **Rend `trop_grande` quand une texture neuve dépasse le côté courant.**
    /// Un tableau n'a qu'une taille de couche, et la règle du dépôt est
    /// d'agrandir les petites plutôt que de réduire les grandes — réduire
    /// perdrait la moitié des pixels. Accueillir une tuile plus grande
    /// demanderait donc de réécrire TOUS les pixels : on le dit, et
    /// l'appelant rebâtit. Rare (92,7 % du pack du serveur est en 16 × 16) et
    /// jamais silencieux.
    pub fn etendre<S: Source + ?Sized>(
        &mut self,
        src: &S,
        noms: impl IntoIterator<Item = String>,
        chemin: &dyn Fn(&str) -> Vec<String>,
    ) -> Ajout {
        let (lues, manquantes) = Self::lire_tuiles(src, noms, chemin, &self.index);
        let mut ajout = Ajout::default();
        if let Some((nom, _)) = lues.iter().find(|(_, t)| t.cote > self.cote) {
            ajout.trop_grande = Some(nom.clone());
            return ajout;
        }
        let par_couche = (self.cote * self.cote * 4) as usize;
        self.pixels.reserve(par_couche * lues.len());
        for (nom, t) in lues {
            let img = t.image(0);
            let redim = if t.cote == self.cote {
                img.to_vec()
            } else {
                Tuile::agrandir(img, t.cote, self.cote)
            };
            self.index.insert(nom.clone(), self.couches.len() as u32);
            self.couches.push(Couche {
                nom,
                images: t.images,
                transparente: t.transparente,
                moyenne: t.moyenne,
            });
            self.pixels.extend_from_slice(&redim);
            ajout.ajoutees += 1;
        }
        // Un trou doit se VOIR, à l'extension comme au bâti.
        self.manquantes.extend(manquantes);
        ajout
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

    /// Le facteur qui COMPENSE le gris d'une tuile, pour une couleur PLATE.
    ///
    /// Les textures teintées du jeu sont GRISES : `grass_block_top.png` vaut
    /// (147, 147, 147). Une vue qui ne pose pas la texture — une icône, une
    /// carte, un aperçu en couleurs unies — n'a rien pour porter ce gris : il
    /// faut le rendre au facteur, sinon l'herbe y sort deux fois trop sombre.
    ///
    /// **Le chemin TEXTURÉ ne doit pas s'en servir.** Là, la texture porte
    /// déjà le gris, et la règle est celle du jeu : `texel × teinte`.
    /// Compenser y ferait dépasser 1 au canal vert (1,286 pour l'herbe), et
    /// comme une teinte ne peut qu'assombrir il serait écrêté — le vert
    /// perdrait son avance sur le rouge et le sol sortirait OLIVE. Mesuré :
    /// (145, 147, 89) au lieu de (84, 109, 51). Voir
    /// `apparence::teinte_finale`.
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

/// Les niveaux de mip d'une couche, du plus grand au plus petit.
///
/// **Sans eux, tout build vu de loin est du BRUIT.** Une texture de 16 × 16
/// écrasée dans dix pixels d'écran échantillonne un pixel sur deux, et le
/// résultat scintille à chaque mouvement de caméra. Mesuré à l'œil sur la
/// première capture : un mur de pierre ressemblait à de la neige.
///
/// La moyenne est **pondérée par l'alpha**, et ce n'est pas un raffinement :
/// une feuille a des pixels transparents dont la couleur est arbitraire, en
/// général noire. Les moyenner à poids égal borderait chaque feuille de noir à
/// mesure qu'on s'en éloigne — c'est le défaut que Minecraft a mis des années
/// à corriger dans son propre mipmapping.
pub fn mips(cote: u32, pixels: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let mut out = vec![(cote, pixels.to_vec())];
    let mut c = cote;
    while c > 1 {
        let (precedent_cote, precedent) = out.last().unwrap();
        let pc = *precedent_cote;
        c = (pc / 2).max(1);
        let mut niveau = vec![0u8; (c * c * 4) as usize];
        for y in 0..c {
            for x in 0..c {
                let mut somme = [0f32; 3];
                let mut alpha = 0f32;
                let mut poids = 0f32;
                for dy in 0..2u32 {
                    for dx in 0..2u32 {
                        let sx = (x * 2 + dx).min(pc - 1);
                        let sy = (y * 2 + dy).min(pc - 1);
                        let i = ((sy * pc + sx) * 4) as usize;
                        let a = precedent[i + 3] as f32 / 255.0;
                        somme[0] += precedent[i] as f32 * a;
                        somme[1] += precedent[i + 1] as f32 * a;
                        somme[2] += precedent[i + 2] as f32 * a;
                        alpha += a;
                        poids += 1.0;
                    }
                }
                let d = ((y * c + x) * 4) as usize;
                if alpha > 0.0 {
                    for k in 0..3 {
                        niveau[d + k] = (somme[k] / alpha).round().clamp(0.0, 255.0) as u8;
                    }
                }
                niveau[d + 3] = ((alpha / poids) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
        out.push((c, niveau));
    }
    out
}

impl Atlas {
    /// Le nombre de niveaux de mip d'une couche.
    pub fn niveaux(&self) -> u32 {
        32 - self.cote.max(1).leading_zeros()
    }

    /// Toutes les couches, à tous les niveaux : `[niveau][couche]`.
    ///
    /// Un niveau à la fois, toutes couches confondues — c'est l'ordre dans
    /// lequel le GPU les attend.
    pub fn pyramide(&self) -> Vec<(u32, Vec<u8>)> {
        let par_couche = (self.cote * self.cote * 4) as usize;
        let mut par_niveau: Vec<(u32, Vec<u8>)> = Vec::new();
        for (i, _) in self.couches.iter().enumerate() {
            let src = &self.pixels[i * par_couche..(i + 1) * par_couche];
            for (n, (cote, données)) in mips(self.cote, src).into_iter().enumerate() {
                if par_niveau.len() <= n {
                    par_niveau.push((cote, Vec::new()));
                }
                par_niveau[n].1.extend_from_slice(&données);
            }
        }
        par_niveau
    }
}
