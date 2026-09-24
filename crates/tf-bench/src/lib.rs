//! Fixtures de mesure.
//!
//! Aucun fichier binaire dans le dépôt : les régions se construisent à la
//! volée depuis une graine, donc identiques d'une exécution à l'autre et
//! lisibles en revue. Les benchs n'ont ainsi besoin ni de Node, ni d'une save
//! réelle, ni de la moindre ressource extérieure.
//!
//! Le contenu ressemble volontairement à du **terrain Minecraft** plutôt qu'à
//! du bruit : palettes d'une dizaine d'entrées, beaucoup de pierre, des
//! sections entièrement pleines et d'autres entièrement vides. Une région de
//! blocs aléatoires mesurerait un cas qui n'existe pas — palette saturée,
//! compression inefficace, aucune section homogène — et orienterait les
//! optimisations vers le mauvais endroit.

#![forbid(unsafe_code)]

pub mod build;
pub mod catalogue;

pub use build::Build;
pub use catalogue::{Forme, BLOCS};

use std::io::Write;

use tf_anvil::{Packing, SECTOR};
use tf_nbt::{tag, Writer};

/// Générateur déterministe, sans dépendance.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 1 } else { seed })
    }
    /// Nommé `next_u32` et non `next` : un `next` sur un type qui n'implémente
    /// pas `Iterator` se lit comme s'il en était un, et une boucle `for` sur ce
    /// générateur ne compilerait pas pour une raison illisible.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }
}

/// Les blocs d'un sous-sol plausible.
///
/// Vingt et une entrées, et c'est un choix mesuré : une palette de 21 demande
/// **5 bits** par indice. Les largeurs de 4 et 8 bits sont les seules où les
/// deux packings du format produisent les mêmes octets — un bench qui n'aurait
/// que des palettes de 16 entrées ne ferait jamais travailler le chemin
/// intéressant, et ne verrait aucune régression dessus.
const STRATA: [&str; 21] = [
    "minecraft:deepslate",
    "minecraft:stone",
    "minecraft:andesite",
    "minecraft:diorite",
    "minecraft:granite",
    "minecraft:gravel",
    "minecraft:dirt",
    "minecraft:coarse_dirt",
    "minecraft:tuff",
    "minecraft:calcite",
    "minecraft:iron_ore",
    "minecraft:copper_ore",
    "minecraft:coal_ore",
    "minecraft:gold_ore",
    "minecraft:redstone_ore",
    "minecraft:lapis_ore",
    "minecraft:diamond_ore",
    "minecraft:water",
    "minecraft:lava",
    "minecraft:smooth_basalt",
    "minecraft:amethyst_block",
];

#[derive(Debug, Clone, Copy)]
pub struct Terrain {
    /// Côté en chunks. 32 = une région pleine.
    pub side: u32,
    /// Sections par chunk. 24 = la hauteur complète 1.18+ (y ∈ [−64, 319]).
    pub sections: usize,
    pub seed: u32,
    pub packing: Packing,
    /// Block entities par chunk. **Zéro par défaut**, et c'est délibéré :
    /// toutes les mesures publiées du dépôt ont été prises sans, et changer
    /// la fixture par défaut déplacerait la référence de tout ce qui suit.
    ///
    /// La fixture ne pose PAS un bloc de coffre sous chacune — le moteur ne
    /// le demande pas. Il suit les CASES, jamais le `id` de l'entrée : une
    /// entité de mod que personne ne sait nommer se déplace comme les autres.
    pub coffres: u32,
    /// Écrire les BIOMES des sections. **Faux par défaut**, même raison que
    /// `coffres` : toutes les mesures publiées ont été prises sans, et
    /// déplacer la fixture par défaut déplacerait la référence de tout ce qui
    /// suit. Un vrai chunk 1.18+ en porte toujours.
    pub biomes: bool,
}

impl Default for Terrain {
    fn default() -> Self {
        Terrain {
            side: 32,
            sections: 24,
            seed: 7,
            packing: Packing::NoStraddle,
            coffres: 0,
            biomes: false,
        }
    }
}

impl Terrain {
    /// Une région pleine : 1024 chunks, 24 576 sections, 100 663 296 blocs.
    pub fn region_pleine() -> Self {
        Terrain::default()
    }

    /// Un quart de région — pour les benchs qu'on veut voir tourner vite.
    pub fn petite() -> Self {
        Terrain {
            side: 16,
            sections: 12,
            ..Terrain::default()
        }
    }

    /// La même, avec des block entities. Pour tout ce qui doit prouver qu'un
    /// coffre suit son bloc.
    pub fn peuplee(coffres: u32) -> Self {
        Terrain {
            coffres,
            ..Terrain::petite()
        }
    }

    /// La même, avec les biomes des sections.
    pub fn avec_biomes() -> Self {
        Terrain {
            biomes: true,
            ..Terrain::petite()
        }
    }

    /// Les biomes d'une section, dans l'ordre de la palette. Une section sur
    /// trois est MONOBIOME — le cas sans `data`, celui qu'on oublie.
    pub fn biomes_de(sy: i8) -> &'static [&'static str] {
        if sy % 3 == 0 {
            &["minecraft:plains"]
        } else {
            &[
                "minecraft:plains",
                "minecraft:forest",
                "minecraft:river",
                "minecraft:desert",
            ]
        }
    }

    /// Le nom du k-ième coffre d'un chunk. Il porte son CHUNK D'ORIGINE, et
    /// c'est ce qui permet à un test de dire d'où vient celui qu'il relit :
    /// sans ça, les coffres se ressemblent tous et un collage qui n'aurait
    /// rien posé passerait pour une réussite.
    pub fn nom_coffre(cx: i32, cz: i32, k: u32) -> String {
        format!("{{\"text\":\"Coffre {cx}/{cz}#{k}\"}}")
    }

    /// La case du k-ième coffre d'un chunk, en coordonnées MONDE.
    ///
    /// Rendue par la fixture plutôt que recopiée dans chaque test : deux
    /// constantes indépendantes finissent par diverger, et un test qui vise
    /// la mauvaise case passerait en ne vérifiant rien.
    ///
    /// **Les `y` DÉCROISSENT avec `k`**, donc l'ordre du FICHIER est l'inverse
    /// de l'ordre YZX. Ce n'est pas un détail : le jeu écrit ses entrées dans
    /// l'ordre où elles sont apparues, pas trié. Une fixture dont les deux
    /// ordres coïncident laisse passer un collage qui AJOUTE ses entités au
    /// lieu de remplacer celle de la case — mêmes entrées, autre ordre, donc
    /// d'autres octets et un correctif de journal pour zéro changement.
    /// Vérifié par mutation : avec des `y` croissants, plus aucun test ne le
    /// voyait.
    pub fn case_coffre(cx: i32, cz: i32, k: u32) -> [i32; 3] {
        [
            cx * 16 + ((k * 5 + 1) % 16) as i32,
            -30 - (k as i32 * 3),
            cz * 16 + ((k * 7 + 2) % 16) as i32,
        ]
    }

    pub fn blocs(&self) -> usize {
        (self.side as usize) * (self.side as usize) * self.sections * 4096
    }

    pub fn sections_total(&self) -> usize {
        (self.side as usize) * (self.side as usize) * self.sections
    }
}

fn section_payload(sy: i8, rng: &mut Rng, packing: Packing) -> Vec<u8> {
    let world_y = sy as i32 * 16;

    // Au-dessus de la surface : de l'air, donc une section homogène. C'est la
    // moitié d'un vrai monde, et c'est le cas que le format optimise.
    if world_y > 64 {
        return tf_nbt::block_states_payload(
            &[tf_nbt::PaletteEntryRef {
                name: "minecraft:air",
                props: &[],
            }],
            &[],
        );
    }

    let base = if world_y < 0 {
        "minecraft:deepslate"
    } else {
        "minecraft:stone"
    };
    let mut noms: Vec<&str> = vec![base];
    for s in STRATA {
        if s != base {
            noms.push(s);
        }
    }
    let n = noms.len();

    // 6 % de veines, comme un vrai sous-sol.
    let idx: Vec<u16> = (0..4096)
        .map(|_| {
            if rng.below(100) < 6 {
                rng.below(n) as u16
            } else {
                0
            }
        })
        .collect();

    let bits = tf_anvil::bits_for(n) as usize;
    let data = tf_anvil::pack(&idx, bits, packing);
    let entries: Vec<tf_nbt::PaletteEntryRef> = noms
        .iter()
        .map(|name| tf_nbt::PaletteEntryRef { name, props: &[] })
        .collect();
    tf_nbt::block_states_payload(&entries, &data)
}

fn chunk_nbt(cx: i32, cz: i32, t: &Terrain, rng: &mut Rng) -> Vec<u8> {
    let mut w = Writer::with_capacity(64 * 1024);
    // `field` écrit DÉJÀ le nom : un `raw_str("")` de plus serait lu comme le
    // type du premier champ, donc comme un TAG_End, et le chunk se terminerait
    // aussitôt — sans erreur, juste vide.
    w.field(tag::COMPOUND, "");

    w.field(tag::INT, "DataVersion").i32_payload(3465);
    w.field(tag::INT, "xPos").i32_payload(cx);
    w.field(tag::INT, "yPos").i32_payload(-4);
    w.field(tag::INT, "zPos").i32_payload(cz);
    w.field(tag::STRING, "Status").raw_str("minecraft:full");
    // Comme tout chunk que le jeu a éclairé : c'est l'octet qu'une édition
    // doit remettre à zéro pour que le jeu rééclaire.
    w.field(tag::BYTE, "isLightOn").i8_payload(1);

    // Des Heightmaps réalistes : gros, et sans le moindre intérêt pour nous.
    // Ils font partie du coût de lecture d'un vrai chunk, donc ils doivent
    // faire partie du bench — les omettre flatterait les chiffres.
    w.field(tag::COMPOUND, "Heightmaps");
    w.field(tag::LONG_ARRAY, "MOTION_BLOCKING");
    w.long_array_payload(&vec![0x0123_4567_89AB_CDEF; 37]);
    w.field(tag::LONG_ARRAY, "WORLD_SURFACE");
    w.long_array_payload(&vec![0xFEDC_BA98_7654_3210; 37]);
    w.end();

    w.field(tag::LIST, "sections");
    w.list_header(tag::COMPOUND, t.sections);
    for k in 0..t.sections {
        let sy = -4i8 + k as i8;
        w.field(tag::BYTE, "Y").i8_payload(sy);
        w.field(tag::COMPOUND, "block_states");
        w.raw(&section_payload(sy, rng, t.packing));
        if t.biomes {
            let noms = Terrain::biomes_de(sy);
            // Quatre entrées → DEUX bits. Le plancher de quatre bits des
            // blocs n'existe pas ici, et l'appliquer écrirait un tableau
            // deux fois trop long que le jeu refuse.
            let data: Vec<u64> = if noms.len() > 1 {
                let idx: Vec<u16> = (0..64).map(|i| (i % noms.len()) as u16).collect();
                tf_anvil::pack(&idx, 2, Packing::NoStraddle)
            } else {
                Vec::new()
            };
            w.field(tag::COMPOUND, "biomes");
            w.raw(&tf_nbt::biomes_payload(noms, &data));
        }
        w.end();
    }

    if t.coffres == 0 {
        w.field(tag::LIST, "block_entities")
            .list_header(tag::COMPOUND, 0);
    } else {
        w.field(tag::LIST, "block_entities");
        w.list_header(tag::COMPOUND, t.coffres as usize);
        for k in 0..t.coffres {
            coffre(
                &mut w,
                Terrain::case_coffre(cx, cz, k),
                &Terrain::nom_coffre(cx, cz, k),
            );
        }
    }
    w.end(); // racine
    w.into_bytes()
}

/// Un coffre plein, avec de quoi perdre.
///
/// Le contenu est là exprès : une entrée vide ne prouverait rien. Ce qu'on
/// veut voir survivre à une rotation, c'est la pile d'objets et le nom
/// personnalisé — précisément ce qu'un parseur qui ré-encoderait l'entrée
/// aurait la possibilité d'abîmer.
fn coffre(w: &mut Writer, [x, y, z]: [i32; 3], nom: &str) {
    w.field(tag::STRING, "id").raw_str("minecraft:chest");
    w.field(tag::INT, "x").i32_payload(x);
    w.field(tag::INT, "y").i32_payload(y);
    w.field(tag::INT, "z").i32_payload(z);
    w.field(tag::STRING, "CustomName").raw_str(nom);
    w.field(tag::LIST, "Items");
    w.list_header(tag::COMPOUND, 2);
    for (slot, item) in [(0u8, "minecraft:diamond"), (7, "minecraft:emerald")] {
        w.field(tag::BYTE, "Slot").i8_payload(slot as i8);
        w.field(tag::STRING, "id").raw_str(item);
        w.field(tag::BYTE, "Count")
            .i8_payload(1 + (nom.len() % 32) as i8);
        w.end();
    }
    w.end();
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// Construit un `.mca` complet.
pub fn region(t: &Terrain) -> Vec<u8> {
    region_en(t, 0, 0)
}

/// La même région, mais POSÉE quelque part dans le monde.
///
/// `region` écrit `xPos`/`zPos` comme si la région était `r.0.0` : ses chunks
/// annoncent (0, 0), (1, 0)… quel que soit le fichier où on la range. C'est
/// sans conséquence tant qu'un test n'a qu'une région, et c'est un piège dès
/// qu'il en a deux — le contenu d'un `.mca` porte ses propres coordonnées, et
/// tout ce qui les lit trouverait quatre régions empilées au même endroit.
pub fn region_en(t: &Terrain, rx: i32, rz: i32) -> Vec<u8> {
    let mut rng = Rng::new(t.seed);
    let mut locations = vec![0u8; 4096];
    let mut timestamps = vec![0u8; 4096];
    let mut body: Vec<u8> = Vec::new();
    let mut next = 2u32;

    for cz in 0..t.side {
        for cx in 0..t.side {
            let payload = zlib(&chunk_nbt(
                rx * 32 + cx as i32,
                rz * 32 + cz as i32,
                t,
                &mut rng,
            ));
            let len = payload.len() + 1;
            let total = 4 + len;
            let sectors = total.div_ceil(SECTOR);

            body.extend_from_slice(&(len as u32).to_be_bytes());
            body.push(2); // zlib
            body.extend_from_slice(&payload);
            body.resize(body.len() + (sectors * SECTOR - total), 0);

            let i = (cx + cz * 32) as usize;
            let loc = (next << 8) | sectors as u32;
            locations[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
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

/// Occupation mémoire résidente du processus, en octets.
///
/// Lue dans `/proc/self/statm` : c'est la seule mesure qui compte pour ce
/// projet — un pic de RSS est ce qui fait échouer une opération chez
/// l'utilisateur, pas la somme des allocations.
pub fn rss_bytes() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<usize>().ok())
        .map(|pages| pages * 4096)
        .unwrap_or(0)
}
