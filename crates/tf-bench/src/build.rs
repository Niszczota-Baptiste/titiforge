//! Un **build Minefield**, et non du terrain.
//!
//! Les fixtures de la phase 0 imitent un sous-sol : palettes d'une dizaine
//! d'entrées courtes, cubes pleins partout, sections entièrement homogènes.
//! C'est juste pour mesurer le chargement Anvil, et **faux** pour tout ce qui
//! touche au rendu — ça donne 100 % de cubes pleins là où la cible en a un
//! tiers (`catalogue.rs`). Mesurer le maillage dessus mesurerait le mauvais
//! chemin, et dimensionnerait la phase 2 sur un cas qui n'existe pas.
//!
//! Ce générateur produit ce que les gens construisent vraiment sur le serveur :
//! des salles, des murs, des piliers, un sol, un toit, et du décor semé le long
//! des murs. Conséquences mesurables, toutes voulues :
//!
//! - **beaucoup d'air** — un bâtiment est surtout du vide ;
//! - **de longues arêtes** de blocs identiques, ce que le maillage glouton sait
//!   exploiter ;
//! - **du décor épars** en blocs-modèles, ce qu'il ne sait pas exploiter ;
//! - **des palettes de 5 à 40 entrées** à noms longs, donc des largeurs de 3 à
//!   6 bits et des octets de palette réalistes ;
//! - **presque aucune section homogène** sous le toit.
//!
//! Le tirage se hache sur la POSITION (`hash3`), jamais sur un état : c'est
//! l'invariant n° 5 du projet, et il s'applique aussi aux fixtures. Un
//! générateur à état ne rejouerait à l'identique que si la boucle visitait les
//! cases dans le même ordre — contrainte invisible qu'une parallélisation du
//! générateur casserait sans bruit.

use std::io::Write;

use tf_anvil::{Packing, SECTOR};
use tf_nbt::{tag, PaletteEntryRef, Writer};

use crate::catalogue::{Forme, BLOCS};

/// Hachage de position. Même rôle que le `hash3` du moteur : rejouable, et
/// indépendant de l'ordre de parcours.
#[inline]
pub fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed ^ 0x9E37_79B9;
    h ^= (x as u32).wrapping_mul(0x85EB_CA6B);
    h = h.rotate_left(13).wrapping_mul(0xC2B2_AE35);
    h ^= (y as u32).wrapping_mul(0x27D4_EB2F);
    h = h.rotate_left(17).wrapping_mul(0x1656_67B1);
    h ^= (z as u32).wrapping_mul(0x165E_1B95);
    h ^= h >> 16;
    h
}

/// Les blocs du catalogue d'une forme donnée, dans l'ordre de la table.
fn de_forme(f: Forme) -> Vec<&'static str> {
    BLOCS
        .iter()
        .filter(|(_, forme, _)| *forme == f)
        .map(|(n, _, _)| *n)
        .collect()
}

/// Propriétés posées sur un bloc de décor.
///
/// 54 % des blocs Minefield portent un état, et `facing` est de loin la plus
/// fréquente (24 572 occurrences). Une palette dont aucune entrée n'a de
/// propriété sous-estime sa propre taille en octets et ne fait jamais
/// travailler le tri de clés.
const ORIENTATIONS: [&str; 4] = ["north", "east", "south", "west"];

#[derive(Debug, Clone, Copy)]
pub struct Build {
    /// Côté en chunks. 32 = une région pleine.
    pub side: u32,
    /// Sections par chunk, depuis y = −64.
    pub sections: usize,
    pub seed: u32,
    pub packing: Packing,
    /// Hauteur sous plafond d'un étage, en blocs.
    pub etage: i32,
    /// Côté d'une salle, en blocs. Les murs tombent sur les multiples.
    pub salle: i32,
    /// Chances sur cent qu'une case le long d'un mur porte du décor.
    ///
    /// **C'est un réglage, pas une mesure.** Personne ici n'a de vrai build
    /// Minefield à recenser : la proportion de blocs-modèles dans un bâtiment
    /// dépend de qui l'a construit. La faire passer pour un fait mesuré serait
    /// exactement l'erreur que ce dépôt s'interdit. Elle est donc explicite, et
    /// les benchs la BALAIENT au lieu de parier sur une valeur.
    ///
    /// À 55, le relevé donne 9,1 % des blocs posés en modèles, portant 42 % des
    /// cuboïdes à émettre.
    pub densite_decor: u32,
}

impl Default for Build {
    fn default() -> Self {
        Build {
            side: 32,
            sections: 24,
            seed: 1789,
            packing: Packing::NoStraddle,
            etage: 6,
            salle: 11,
            densite_decor: 55,
        }
    }
}

impl Build {
    /// Une région pleine bâtie : 1024 chunks.
    pub fn region_pleine() -> Self {
        Build::default()
    }

    /// Un quart de région — pour les benchs qu'on veut voir tourner vite.
    pub fn petit() -> Self {
        Build {
            side: 16,
            sections: 12,
            ..Build::default()
        }
    }

    /// Le même build, à une autre densité de décor. Sert à BALAYER le réglage
    /// plutôt qu'à parier dessus.
    pub fn avec_decor(self, densite: u32) -> Self {
        Build {
            densite_decor: densite.min(100),
            ..self
        }
    }

    /// Un seul chunk, pour les tests.
    pub fn minuscule() -> Self {
        Build {
            side: 1,
            sections: 8,
            ..Build::default()
        }
    }

    pub fn blocs(&self) -> usize {
        (self.side as usize) * (self.side as usize) * self.sections * 4096
    }

    /// Le `y` du premier bloc bâti — le bas de la première section.
    pub fn sol(&self) -> i32 {
        -64
    }

    /// Le `y` du toit.
    ///
    /// Le bâti remplit tout ce qu'on lui donne, moins **une section de ciel**.
    /// Ce n'est pas la proportion d'une vraie save (un build de 80 blocs dans
    /// un monde de 384 laisse 79 % de sections vides) et c'est délibéré : ce
    /// qu'on mesure ici, c'est le maillage, pas le temps qu'il met à ne rien
    /// faire. La fixture Anvil, elle, garde son profil de terrain avec ses
    /// sections homogènes.
    pub fn toit(&self) -> i32 {
        self.sol() + (self.sections as i32 - 1) * 16
    }

    /// Le bloc en (x, y, z) MONDE : `None` pour de l'air.
    ///
    /// Rend le nom et ses propriétés. Pur, et fonction de la seule position :
    /// deux appels donnent le même résultat, dans n'importe quel ordre.
    pub fn bloc(&self, x: i32, y: i32, z: i32) -> Option<(&'static str, Option<&'static str>)> {
        if y < self.sol() || y > self.toit() {
            return None;
        }
        // Tout se compte depuis le sol : les murs et les planchers doivent
        // tomber aux mêmes hauteurs quel que soit le repère du monde.
        let y = y - self.sol();
        let h = hash3(x, y + self.sol(), z, self.seed);

        let structure = de_forme(Forme::Cube);
        let decor = de_forme(Forme::Modele);

        // Un bloc de structure est choisi par ZONE, pas par case : c'est ce qui
        // fait de longues arêtes identiques, donc des quads gloutons. Un tirage
        // par case donnerait du bruit, que rien ne saurait fusionner.
        let zone = hash3(
            x.div_euclid(self.salle),
            y.div_euclid(self.etage),
            z.div_euclid(self.salle),
            self.seed ^ 0x5151,
        ) as usize;

        let mur_x = x.rem_euclid(self.salle) == 0;
        let mur_z = z.rem_euclid(self.salle) == 0;
        let plancher = y.rem_euclid(self.etage) == 0;

        // Une porte par mur : sans ouvertures, chaque salle serait une boîte
        // fermée et le mailleur ne verrait jamais de face intérieure.
        let porte = y.rem_euclid(self.etage) > 0
            && y.rem_euclid(self.etage) <= 2
            && ((mur_x && (z.rem_euclid(self.salle) - self.salle / 2).abs() <= 1)
                || (mur_z && (x.rem_euclid(self.salle) - self.salle / 2).abs() <= 1));

        if porte {
            return None;
        }

        if mur_x || mur_z || plancher {
            let n = structure[zone % structure.len()];
            // Les piliers d'angle prennent un autre bloc : un détail que le
            // glouton ne peut pas fusionner avec le mur.
            if mur_x && mur_z {
                return Some((structure[(zone + 7) % structure.len()], None));
            }
            return Some((n, None));
        }

        // Décor le long des murs, au niveau du sol de l'étage.
        let contre_mur = x.rem_euclid(self.salle) == 1
            || z.rem_euclid(self.salle) == 1
            || x.rem_euclid(self.salle) == self.salle - 1
            || z.rem_euclid(self.salle) == self.salle - 1;
        let au_sol = y.rem_euclid(self.etage) == 1;

        // Le décor d'une salle est un THÈME, pas un tirage dans tout le
        // catalogue : un bâtisseur choisit une dizaine de blocs et les répète.
        // Sans ça, une section ramassait 132 entrées de palette — 8 bits par
        // indice là où un build réel en demande 5 ou 6, et une palette qui pèse
        // quatre fois trop lourd en octets.
        // Le thème couvre un BÂTIMENT, pas une salle : une section de 16 cases
        // chevauche deux ou trois salles, donc en tirer le thème par salle
        // ramassait une douzaine de thèmes par section — 121 entrées de palette
        // mesurées, au lieu des 20 à 40 d'un build réel.
        const THEME: usize = 6;
        let theme = hash3(
            x.div_euclid(64),
            y.div_euclid(48),
            z.div_euclid(64),
            self.seed ^ 0x7E7E,
        ) as usize
            % decor.len();
        let choisir = |k: usize| decor[(theme + k % THEME) % decor.len()];

        if contre_mur && au_sol && h % 100 < self.densite_decor {
            return Some((
                choisir(h as usize >> 8),
                Some(ORIENTATIONS[(h >> 3) as usize % 4]),
            ));
        }
        // Un peu de décor suspendu : lanternes, poutres.
        if au_sol && h % 1000 < 25 {
            return Some((choisir(h as usize >> 11), None));
        }
        None
    }
}

/// Une palette locale à une section : `nom|props` → indice.
#[derive(Default)]
struct Palette {
    cles: Vec<(&'static str, Option<&'static str>)>,
}

impl Palette {
    fn indice(&mut self, e: (&'static str, Option<&'static str>)) -> u16 {
        if let Some(i) = self.cles.iter().position(|c| *c == e) {
            return i as u16;
        }
        self.cles.push(e);
        (self.cles.len() - 1) as u16
    }
}

fn section_payload(b: &Build, cx: i32, cz: i32, sy: i8) -> Vec<u8> {
    let mut pal = Palette::default();
    // L'air d'abord : c'est l'indice 0, et une section entièrement vide se
    // réduit alors à une palette d'une entrée, sans tableau d'indices.
    pal.indice(("minecraft:air", None));

    let mut idx = vec![0u16; 4096];
    let base_y = sy as i32 * 16;
    for y in 0..16i32 {
        for z in 0..16i32 {
            for x in 0..16i32 {
                let e = b.bloc(cx * 16 + x, base_y + y, cz * 16 + z);
                let Some(e) = e else { continue };
                // Ordre YZX — le même que le format.
                idx[(y * 256 + z * 16 + x) as usize] = pal.indice(e);
            }
        }
    }

    // `PaletteEntryRef` emprunte ses propriétés : elles doivent survivre à
    // l'appel, d'où ce tampon tenu à côté.
    let props: Vec<Vec<(String, String)>> = pal
        .cles
        .iter()
        .map(|(_, p)| match p {
            Some(v) => vec![("facing".to_string(), v.to_string())],
            None => Vec::new(),
        })
        .collect();
    let entrees: Vec<PaletteEntryRef> = pal
        .cles
        .iter()
        .zip(props.iter())
        .map(|((n, _), p)| PaletteEntryRef { name: n, props: p })
        .collect();

    if entrees.len() == 1 {
        return tf_nbt::block_states_payload(&entrees, &[]);
    }
    let bits = tf_anvil::bits_for(entrees.len()) as usize;
    let data = tf_anvil::pack(&idx, bits, b.packing);
    tf_nbt::block_states_payload(&entrees, &data)
}

/// `cx`/`cz` sont LOCAUX à la région — c'est eux qui décident du contenu —
/// et `(mx, mz)` sont les coordonnées MONDE annoncées par le chunk. Les deux
/// coïncident pour `r.0.0` et divergent partout ailleurs : le même bâtiment,
/// posé à un autre endroit du monde.
fn chunk_nbt(b: &Build, cx: i32, cz: i32, mx: i32, mz: i32) -> Vec<u8> {
    let mut w = Writer::with_capacity(96 * 1024);
    w.field(tag::COMPOUND, "");
    w.field(tag::INT, "DataVersion").i32_payload(2975); // 1.18.2 — la cible
    w.field(tag::INT, "xPos").i32_payload(mx);
    w.field(tag::INT, "yPos").i32_payload(-4);
    w.field(tag::INT, "zPos").i32_payload(mz);
    w.field(tag::STRING, "Status").raw_str("minecraft:full");

    w.field(tag::COMPOUND, "Heightmaps");
    w.field(tag::LONG_ARRAY, "MOTION_BLOCKING");
    w.long_array_payload(&vec![0x0123_4567_89AB_CDEF; 37]);
    w.field(tag::LONG_ARRAY, "WORLD_SURFACE");
    w.long_array_payload(&vec![0xFEDC_BA98_7654_3210; 37]);
    w.end();

    w.field(tag::LIST, "sections");
    w.list_header(tag::COMPOUND, b.sections);
    for k in 0..b.sections {
        let sy = -4i8 + k as i8;
        w.field(tag::BYTE, "Y").i8_payload(sy);
        w.field(tag::COMPOUND, "block_states");
        w.raw(&section_payload(b, cx, cz, sy));
        w.end();
    }

    w.field(tag::LIST, "block_entities")
        .list_header(tag::COMPOUND, 0);
    w.end();
    w.into_bytes()
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// Construit le `.mca` d'un build, comme s'il était `r.0.0`.
pub fn region(b: &Build) -> Vec<u8> {
    region_en(b, 0, 0)
}

/// **Le même build, mais POSÉ quelque part dans le monde.**
///
/// `region` écrit `xPos`/`zPos` comme si la région était `r.0.0` : ses chunks
/// annoncent (0, 0), (1, 0)… quel que soit le fichier où on la range. C'est
/// sans conséquence tant qu'on n'a qu'une région, et c'est un piège dès qu'on
/// en a deux — le contenu d'un `.mca` porte ses propres coordonnées, et tout
/// ce qui les lit trouverait quatre régions empilées au même endroit. Même
/// histoire, même remède que `Terrain::region_en`.
///
/// Le CONTENU, lui, ne bouge pas : c'est le même bâtiment à chaque région, ce
/// qui est exactement ce qu'on veut d'une fixture — deux mesures prises à
/// deux endroits du monde doivent être comparables.
pub fn region_en(b: &Build, rx: i32, rz: i32) -> Vec<u8> {
    let mut locations = vec![0u8; 4096];
    let mut timestamps = vec![0u8; 4096];
    let mut body: Vec<u8> = Vec::new();
    let mut next = 2u32;

    for cz in 0..b.side {
        for cx in 0..b.side {
            let payload = zlib(&chunk_nbt(
                b,
                cx as i32,
                cz as i32,
                rx * 32 + cx as i32,
                rz * 32 + cz as i32,
            ));
            let len = payload.len() + 1;
            let total = 4 + len;
            let sectors = total.div_ceil(SECTOR);

            body.extend_from_slice(&(len as u32).to_be_bytes());
            body.push(2);
            body.extend_from_slice(&payload);
            body.resize(body.len() + (sectors * SECTOR - total), 0);

            let i = (cx + cz * 32) as usize;
            locations[i * 4..i * 4 + 4]
                .copy_from_slice(&(((next) << 8) | sectors as u32).to_be_bytes());
            timestamps[i * 4..i * 4 + 4].copy_from_slice(&1_700_000_000u32.to_be_bytes());
            next += sectors as u32;
        }
    }

    let mut out = Vec::with_capacity(8192 + body.len());
    out.extend_from_slice(&locations);
    out.extend_from_slice(&timestamps);
    out.extend_from_slice(&body);
    out
}
