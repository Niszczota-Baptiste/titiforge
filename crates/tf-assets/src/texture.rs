//! Lire les textures d'un pack.
//!
//! Rien de tout ça n'est devinable : les chiffres viennent du pack du serveur,
//! 3 556 textures recensées.
//!
//! - **92,7 % sont en 16 × 16, et le reste NON.** Il y a du 32 × 32 dans le
//!   même pack. Un lecteur qui suppose 16 rognerait ou étirerait le reste.
//! - **242 sont des bandes d'animation** : une image carrée empilée `n` fois,
//!   jusqu'à **32 images** pour une seule texture. Les prendre pour une image
//!   unique écraserait trente-deux vues du feu sur une face.
//! - **Cinq types de PNG cohabitent** — RGBA, palette, RGB, gris, et même du
//!   4 bits. Ne lire que le RGBA laisserait 1 010 textures sur le carreau.

use crate::source::{Result, Source, SourceError};

/// Une texture lue, normalisée en RGBA 8 bits.
#[derive(Debug, Clone)]
pub struct Tuile {
    /// Côté d'UNE image. Pour une bande d'animation, c'est la largeur.
    pub cote: u32,
    /// Nombre d'images empilées verticalement. 1 pour une texture fixe.
    ///
    /// Déduit de la GÉOMÉTRIE (`hauteur / largeur`), et c'est ce que fait le
    /// jeu quand le `.mcmeta` ne dit rien. Le codex extrait du site n'en
    /// contient aucun, donc c'est la seule voie ici.
    pub images: u32,
    /// RGBA8, toutes les images à la suite : `cote * cote * images * 4` octets.
    pub pixels: Vec<u8>,
    /// Au moins un pixel n'est pas complètement opaque.
    ///
    /// C'est la réponse à une question qu'un pack ne pose nulle part : le verre
    /// REMPLIT son bloc et ne doit masquer personne. Une liste de noms en dur
    /// ne couvrirait aucun bloc `minefield:*`.
    pub transparente: bool,
    /// Moyenne RÉELLE des canaux, sur les pixels non totalement transparents.
    ///
    /// Sert aux blocs teintés. Les textures teintées du jeu sont GRISES —
    /// mesuré, `grass_block_top.png` vaut (147, 147, 147) — et c'est le jeu qui
    /// les multiplie par une couleur de biome. Le facteur se DIVISE par cette
    /// moyenne, sinon `gris × vert` donne un vert deux fois trop sombre.
    pub moyenne: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureError {
    Illisible(String),
    /// Une bande dont la hauteur n'est pas un multiple de la largeur n'est ni
    /// une texture carrée ni une animation. Deviner l'un ou l'autre
    /// déformerait le bloc.
    FormeInattendue {
        largeur: u32,
        hauteur: u32,
    },
    Trop {
        pixels: u64,
    },
    Source(SourceError),
}

impl std::fmt::Display for TextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextureError::Illisible(n) => write!(f, "texture illisible : {n}"),
            TextureError::FormeInattendue { largeur, hauteur } => write!(
                f,
                "texture de {largeur} × {hauteur} : ni carrée, ni une bande d'animation"
            ),
            TextureError::Trop { pixels } => {
                write!(f, "texture de {pixels} pixels, au-delà du plafond")
            }
            TextureError::Source(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TextureError {}

impl From<SourceError> for TextureError {
    fn from(e: SourceError) -> Self {
        TextureError::Source(e)
    }
}

/// Plafond de décodage. Un PNG vient du disque d'un utilisateur, et un
/// en-tête forgé annonce 65 535 × 65 535 : la réservation tue le processus
/// avant qu'on ait lu un seul pixel.
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Décode un PNG en RGBA8, quel que soit son type.
pub fn decoder(octets: &[u8], nom: &str) -> std::result::Result<Tuile, TextureError> {
    let mut lecteur = png::Decoder::new(octets);
    // Palette, gris, 4 bits, tRNS : tout est ramené à du RGBA8. Sans ça, 1 010
    // des 3 556 textures du pack ne se liraient pas.
    lecteur.set_transformations(png::Transformations::ALPHA | png::Transformations::EXPAND);
    let mut r = lecteur
        .read_info()
        .map_err(|_| TextureError::Illisible(nom.to_string()))?;
    let info = r.info();
    let (w, h) = (info.width, info.height);
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(TextureError::Trop {
            pixels: (w as u64) * (h as u64),
        });
    }
    if w == 0 || h == 0 || h % w != 0 {
        return Err(TextureError::FormeInattendue {
            largeur: w,
            hauteur: h,
        });
    }
    let mut brut = vec![0u8; r.output_buffer_size()];
    let sortie = r
        .next_frame(&mut brut)
        .map_err(|_| TextureError::Illisible(nom.to_string()))?;
    brut.truncate(sortie.buffer_size());

    let pixels = match sortie.color_type {
        png::ColorType::Rgba => brut,
        png::ColorType::GrayscaleAlpha => brut
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Rgb => brut
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => brut.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return Err(TextureError::Illisible(nom.to_string())),
    };

    let mut transparente = false;
    let mut somme = [0u64; 3];
    let mut compte = 0u64;
    for p in pixels.chunks_exact(4) {
        if p[3] < 255 {
            transparente = true;
        }
        // Un pixel totalement transparent n'a pas de couleur : l'inclure
        // tirerait la moyenne vers le noir d'un fond qu'on ne voit jamais.
        if p[3] > 0 {
            somme[0] += p[0] as u64;
            somme[1] += p[1] as u64;
            somme[2] += p[2] as u64;
            compte += 1;
        }
    }
    let moyenne = if compte == 0 {
        [0.0; 3]
    } else {
        [
            somme[0] as f32 / compte as f32,
            somme[1] as f32 / compte as f32,
            somme[2] as f32 / compte as f32,
        ]
    };

    Ok(Tuile {
        cote: w,
        images: h / w,
        pixels,
        transparente,
        moyenne,
    })
}

impl Tuile {
    /// Les pixels d'UNE image de l'animation.
    pub fn image(&self, k: u32) -> &[u8] {
        let par_image = (self.cote * self.cote * 4) as usize;
        let k = (k.min(self.images.saturating_sub(1))) as usize;
        &self.pixels[k * par_image..(k + 1) * par_image]
    }

    pub fn est_animee(&self) -> bool {
        self.images > 1
    }

    /// Redimensionne une image au plus proche voisin.
    ///
    /// **On agrandit, on ne réduit pas.** Un tableau de textures n'a qu'une
    /// taille de couche ; réduire une 32 × 32 vers 16 perdrait la moitié de ses
    /// pixels, agrandir une 16 × 16 vers 32 n'en perd aucun. Le plus proche
    /// voisin et non une interpolation : Minecraft est en pixels nets, et un
    /// lissage ferait baver chaque bord de bloc.
    pub fn agrandir(pixels: &[u8], de: u32, vers: u32) -> Vec<u8> {
        if de == vers {
            return pixels.to_vec();
        }
        let mut out = vec![0u8; (vers * vers * 4) as usize];
        for y in 0..vers {
            let sy = y * de / vers;
            for x in 0..vers {
                let sx = x * de / vers;
                let s = ((sy * de + sx) * 4) as usize;
                let d = ((y * vers + x) * 4) as usize;
                out[d..d + 4].copy_from_slice(&pixels[s..s + 4]);
            }
        }
        out
    }
}

/// Lit une texture depuis une source, par son identifiant de modèle.
pub fn lire<S: Source + ?Sized>(src: &S, chemin: &str) -> std::result::Result<Tuile, TextureError> {
    let octets: Vec<u8> = src.lire(chemin)?;
    decoder(&octets, chemin)
}

pub type TextureResult = Result<Tuile>;
