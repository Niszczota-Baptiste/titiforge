//! Les textures et le tableau d'atlas.
//!
//! Les PNG de test sont ENCODÉS à la volée, pas commités : un binaire dans le
//! dépôt est opaque en revue et impossible à faire évoluer. Et ils reproduisent
//! les cas difficiles du vrai pack — palette, gris, alpha, bandes d'animation,
//! tailles mêlées — plutôt que de les contourner. Une texture de démonstration
//! déjà simple masque exactement le défaut qu'on veut voir.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tf_assets::texture::{decoder, TextureError};
use tf_assets::{Atlas, Dossier, Tuile};

// ── encoder des PNG de test ─────────────────────────────────────────────────

fn png_rgba(largeur: u32, hauteur: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, largeur, hauteur);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(pixels).unwrap();
    }
    out
}

fn png_rgb(largeur: u32, hauteur: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, largeur, hauteur);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(pixels).unwrap();
    }
    out
}

fn png_gris(largeur: u32, hauteur: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, largeur, hauteur);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(pixels).unwrap();
    }
    out
}

fn uni_rgba(cote: u32, c: [u8; 4]) -> Vec<u8> {
    png_rgba(cote, cote, &c.repeat((cote * cote) as usize))
}

// ── le décodage ─────────────────────────────────────────────────────────────

#[test]
fn les_cinq_types_de_png_du_pack_se_lisent_tous() {
    // Mesuré sur le pack du serveur : RGBA 2 546, palette 550, RGB 445,
    // gris 10. Ne lire que le RGBA laisserait 1 010 textures sur le carreau.
    let rgba = decoder(&uni_rgba(4, [10, 20, 30, 255]), "rgba").unwrap();
    assert_eq!(&rgba.pixels[..4], &[10, 20, 30, 255]);

    let rgb = decoder(&png_rgb(4, 4, &[40u8, 50, 60].repeat(16)), "rgb").unwrap();
    assert_eq!(
        &rgb.pixels[..4],
        &[40, 50, 60, 255],
        "un PNG sans canal alpha est OPAQUE, pas transparent"
    );
    assert!(!rgb.transparente);

    let gris = decoder(&png_gris(4, 4, &[77u8; 16]), "gris").unwrap();
    assert_eq!(&gris.pixels[..4], &[77, 77, 77, 255]);
}

#[test]
fn une_bande_d_animation_se_reconnait_a_sa_geometrie() {
    // 242 textures du pack sont des bandes, jusqu'à 32 images. Les prendre
    // pour une image unique écraserait trente-deux vues du feu sur une face.
    // Le codex ne contient aucun `.mcmeta` : la géométrie est la seule voie,
    // et c'est aussi ce que fait le jeu quand le mcmeta ne dit rien.
    let mut px = Vec::new();
    for k in 0..3u8 {
        px.extend(std::iter::repeat_n([k * 10, 0, 0, 255], 16).flatten());
    }
    let t = decoder(&png_rgba(4, 12, &px), "feu").unwrap();
    assert_eq!(t.cote, 4);
    assert_eq!(t.images, 3);
    assert!(t.est_animee());
    assert_eq!(t.image(0)[0], 0, "première image");
    assert_eq!(t.image(1)[0], 10);
    assert_eq!(t.image(2)[0], 20);
    assert_eq!(t.image(9)[0], 20, "au-delà, on borne au lieu de déborder");
}

#[test]
fn une_texture_carree_a_une_seule_image() {
    let t = decoder(&uni_rgba(16, [1, 2, 3, 255]), "x").unwrap();
    assert_eq!(t.images, 1);
    assert!(!t.est_animee());
}

#[test]
fn une_forme_qui_n_est_ni_carree_ni_une_bande_est_refusee() {
    // 16 × 17 n'est ni l'un ni l'autre. Deviner déformerait le bloc.
    let e = decoder(&png_rgba(4, 7, &[0u8; 4 * 7 * 4]), "bizarre").unwrap_err();
    assert_eq!(
        e,
        TextureError::FormeInattendue {
            largeur: 4,
            hauteur: 7
        }
    );
}

#[test]
fn un_png_abime_ne_fait_pas_paniquer() {
    assert!(matches!(
        decoder(b"pas un png", "x"),
        Err(TextureError::Illisible(_))
    ));
    let mut tronque = uni_rgba(4, [0, 0, 0, 255]);
    tronque.truncate(tronque.len() / 2);
    assert!(decoder(&tronque, "x").is_err());
}

// ── transparence ────────────────────────────────────────────────────────────

#[test]
fn un_seul_pixel_non_opaque_rend_la_texture_transparente() {
    // C'est la réponse à une question qu'un pack ne pose nulle part : le verre
    // REMPLIT son bloc et ne doit masquer personne. Une liste de noms en dur
    // ne couvrirait aucun bloc `minefield:*`.
    let mut px = [200u8, 200, 200, 255].repeat(16);
    px[3] = 128; // un seul pixel à moitié transparent
    let t = decoder(&png_rgba(4, 4, &px), "verre").unwrap();
    assert!(t.transparente);

    let opaque = decoder(&uni_rgba(4, [200, 200, 200, 255]), "pierre").unwrap();
    assert!(!opaque.transparente);
}

#[test]
fn un_pixel_totalement_transparent_ne_compte_pas_dans_la_moyenne() {
    // L'inclure tirerait la moyenne vers le noir d'un fond qu'on ne voit
    // jamais — et le facteur de teinte serait faux pour toutes les plantes.
    let mut px = Vec::new();
    px.extend([100u8, 100, 100, 255]); // un pixel visible
    px.extend([0u8, 0, 0, 0].repeat(15)); // quinze pixels de fond
    let t = decoder(&png_rgba(4, 4, &px), "plante").unwrap();
    assert_eq!(
        t.moyenne,
        [100.0, 100.0, 100.0],
        "la moyenne porte sur ce qu'on VOIT"
    );
}

#[test]
fn une_texture_entierement_transparente_a_une_moyenne_nulle_sans_diviser_par_zero() {
    let t = decoder(&uni_rgba(4, [0, 0, 0, 0]), "vide").unwrap();
    assert_eq!(t.moyenne, [0.0, 0.0, 0.0]);
}

#[test]
fn la_moyenne_est_celle_du_pack_pas_une_estimation() {
    // Les textures teintées du jeu sont GRISES : `grass_block_top.png` vaut
    // (147, 147, 147), mesuré dans le codex. Le facteur de teinte se divise par
    // cette valeur, sinon `gris × vert` donne un vert deux fois trop sombre.
    let t = decoder(&uni_rgba(8, [147, 147, 147, 255]), "herbe").unwrap();
    assert_eq!(t.moyenne, [147.0, 147.0, 147.0]);
}

// ── le tableau d'atlas ──────────────────────────────────────────────────────

struct TempDir(PathBuf);

impl TempDir {
    fn new(nom: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "tf-atlas-{nom}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    /// Écrit `<nom>.png` — la même extension que `chemins` ira chercher.
    fn png(&self, nom: &str, octets: &[u8]) {
        let p = self.0.join(format!("{nom}.png"));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, octets).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn chemins(n: &str) -> Vec<String> {
    vec![format!("{n}.png")]
}

#[test]
fn le_tableau_prend_la_plus_grande_taille_et_agrandit_les_autres() {
    // Un tableau de textures n'a qu'une taille de couche. Réduire une 32 × 32
    // vers 16 perdrait la moitié de ses pixels ; agrandir une 16 × 16 vers 32
    // n'en perd aucun.
    let d = TempDir::new("tailles");
    d.png("petite", &uni_rgba(4, [1, 2, 3, 255]));
    d.png("grande", &uni_rgba(8, [9, 8, 7, 255]));
    let src = Dossier::ouvrir(d.path()).unwrap();

    let a = Atlas::batir(&src, ["petite".to_string(), "grande".to_string()], &chemins);
    assert_eq!(a.cote, 8, "le côté du tableau est le PLUS GRAND rencontré");
    assert_eq!(a.len(), 2);
    assert_eq!(a.pixels.len(), 2 * 8 * 8 * 4);

    // La petite a bien été agrandie, pas rognée : ses 64 pixels sont tous là.
    let c = a.couche("petite").unwrap() as usize;
    let debut = c * 8 * 8 * 4;
    let couche = &a.pixels[debut..debut + 8 * 8 * 4];
    assert!(couche.chunks_exact(4).all(|p| p == [1, 2, 3, 255]));
}

#[test]
fn agrandir_au_plus_proche_voisin_ne_lisse_pas() {
    // Minecraft est en pixels NETS. Une interpolation ferait baver chaque bord
    // de bloc, et le résultat ne ressemblerait plus au jeu.
    let damier: Vec<u8> = (0..4)
        .flat_map(|y| {
            (0..4).flat_map(move |x| {
                let v = if (x + y) % 2 == 0 { 255u8 } else { 0 };
                [v, v, v, 255]
            })
        })
        .collect();
    let grand = Tuile::agrandir(&damier, 4, 8);
    let valeurs: Vec<u8> = grand.chunks_exact(4).map(|p| p[0]).collect();
    assert!(
        valeurs.iter().all(|v| *v == 0 || *v == 255),
        "aucune valeur intermédiaire : {:?}",
        &valeurs[..8]
    );
}

#[test]
fn seule_la_premiere_image_d_une_animation_entre_dans_le_tableau() {
    let d = TempDir::new("anim");
    let mut px = Vec::new();
    for k in 0..4u8 {
        px.extend(std::iter::repeat_n([k * 60, 0, 0, 255], 16).flatten());
    }
    d.png("feu", &png_rgba(4, 16, &px));
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(&src, ["feu".to_string()], &chemins);

    assert_eq!(a.cote, 4, "le côté est celui d'UNE image, pas de la bande");
    assert_eq!(a.pixels.len(), 4 * 4 * 4);
    assert_eq!(a.pixels[0], 0, "la première image");
    assert_eq!(
        a.couches[0].images, 4,
        "mais le nombre d'images est retenu : l'animation se joue en changeant \
         de couche, et c'est le travail du rendu"
    );
}

#[test]
fn une_texture_absente_se_signale_au_lieu_de_disparaitre() {
    let d = TempDir::new("manquante");
    d.png("la", &uni_rgba(4, [1, 1, 1, 255]));
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(&src, ["la".to_string(), "pas_la".to_string()], &chemins);
    assert_eq!(a.len(), 1);
    assert_eq!(a.manquantes, vec!["pas_la".to_string()]);
    assert_eq!(a.couche("pas_la"), None);
}

#[test]
fn une_texture_citee_deux_fois_n_occupe_qu_une_couche() {
    let d = TempDir::new("doublon");
    d.png("a", &uni_rgba(4, [1, 1, 1, 255]));
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(
        &src,
        ["a".to_string(), "a".to_string(), "a".to_string()],
        &chemins,
    );
    assert_eq!(a.len(), 1, "vingt blocs partagent la même planche de chêne");
}

#[test]
fn le_tableau_retient_quelles_couches_sont_transparentes() {
    let d = TempDir::new("transp");
    let mut verre = [200u8, 200, 255, 255].repeat(16);
    verre[3] = 60;
    d.png("verre", &png_rgba(4, 4, &verre));
    d.png("pierre", &uni_rgba(4, [120, 120, 120, 255]));
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(&src, ["verre".to_string(), "pierre".to_string()], &chemins);
    assert_eq!(a.transparentes(), vec!["verre"]);
}

#[test]
fn une_teinte_ne_peut_qu_assombrir() {
    // C'est une multiplication, la même limite que dans le jeu. Un facteur qui
    // éclaircirait ferait sortir des couleurs que Minecraft ne produit pas.
    let d = TempDir::new("teinte");
    d.png("herbe", &uni_rgba(4, [147, 147, 147, 255]));
    d.png("blanche", &uni_rgba(4, [255, 255, 255, 255]));
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(&src, ["herbe".to_string(), "blanche".to_string()], &chemins);
    for c in 0..a.len() as u32 {
        for f in a.facteur_de_teinte(c) {
            assert!(f >= 1.0, "le facteur COMPENSE le gris de la texture : {f}");
        }
    }
    let herbe = a.facteur_de_teinte(a.couche("herbe").unwrap());
    let blanche = a.facteur_de_teinte(a.couche("blanche").unwrap());
    assert!(
        herbe[1] > blanche[1],
        "une texture grise a besoin d'être plus compensée qu'une blanche : \
         {herbe:?} contre {blanche:?}"
    );
}

#[test]
fn un_atlas_vide_ne_divise_pas_par_zero() {
    let d = TempDir::new("vide");
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(&src, Vec::<String>::new(), &chemins);
    assert!(a.is_empty());
    assert_eq!(a.cote, 16, "un côté par défaut, jamais zéro");
    assert_eq!(a.facteur_de_teinte(0), [1.0; 3]);
}

#[test]
fn les_couches_sont_indexees_dans_l_ordre_ou_elles_arrivent() {
    let d = TempDir::new("ordre");
    let noms: Vec<String> = (0..5).map(|i| format!("t{i}")).collect();
    for (i, n) in noms.iter().enumerate() {
        d.png(n, &uni_rgba(4, [i as u8, 0, 0, 255]));
    }
    let src = Dossier::ouvrir(d.path()).unwrap();
    let a = Atlas::batir(&src, noms.clone(), &chemins);
    let index: BTreeMap<&str, u32> = noms
        .iter()
        .map(|n| (n.as_str(), a.couche(n).unwrap()))
        .collect();
    assert_eq!(index["t0"], 0);
    assert_eq!(index["t4"], 4);
    // Et la couche k porte bien les pixels de la texture k.
    for (i, n) in noms.iter().enumerate() {
        let c = a.couche(n).unwrap() as usize;
        assert_eq!(a.pixels[c * 4 * 4 * 4], i as u8, "{n}");
    }
}
