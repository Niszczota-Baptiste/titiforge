//! Des ENTITÉS plausibles — cadres, tableaux, porte-armures, villageois —
//! construites à la volée, comme les régions.
//!
//! Écrites avec les noms et les formes du jeu (1.18), et avec ce qu'une
//! entité réelle porte AUTOUR de ce que le moteur relève : des attributs dont
//! les modificateurs ont leur propre `UUID`, un inventaire, un nom en hangeul,
//! et des données de mod qui contiennent un `{X, Y, Z}` que personne ne
//! nomme. Une fixture qui ne porterait que les champs relevés ne prouverait
//! pas qu'on laisse les autres tranquilles.
//!
//! **Ce fichier n'appelle pas le balayage de `tf-anvil`**, et calcule la
//! position d'une entité accrochée avec la formule du JEU, pas avec celle du
//! moteur : s'il partageait l'une ou l'autre, les tests diraient seulement que
//! le code est d'accord avec lui-même.

use tf_nbt::{tag, Writer};

/// Le `DataVersion` de 1.18.2 — la version du serveur Minefield.
pub const DV_1_18_2: i32 = 2975;

/// Une entité à écrire.
#[derive(Debug, Clone)]
pub struct Occupant {
    pub id: &'static str,
    pub pos: [f64; 3],
    /// `[lacet, tangage]`.
    pub rotation: [f32; 2],
    pub motion: [f64; 3],
    pub uuid: [i32; 4],
    pub traits: Vec<Trait>,
    pub passagers: Vec<Occupant>,
}

/// Ce qu'une entité porte de plus.
#[derive(Debug, Clone)]
pub enum Trait {
    /// `TileX/Y/Z`.
    Tuile([i32; 3]),
    Facing(i8),
    ItemRotation(i8),
    /// `Item: {id, Count}`.
    Objet(&'static str),
    /// `Motive` (1.18).
    Motif(&'static str),
    /// `Pose: {Head, LeftArm}`.
    Pose,
    /// `Leash: {X, Y, Z}` — noué à une clôture.
    Laisse([i32; 3]),
    /// `Leash: {UUID}` — tenu par une entité.
    LaissePar([i32; 4]),
    /// `HivePos: {X, Y, Z}`.
    Ruche([i32; 3]),
    /// `SleepingX/Y/Z` — la case du lit.
    Dort([i32; 3]),
    /// `Brain.memories.<nom>.value = {pos, dimension}`.
    Souvenir(&'static str, [i32; 3], &'static str),
    /// `AttachFace` d'un shulker.
    Attache(i8),
    /// `n` octets INCOMPRESSIBLES dans un tableau de mod : de quoi pousser un
    /// chunk au-delà du mégaoctet, là où le jeu le déporte en `.mcc`.
    Lest(usize),
}

impl Occupant {
    pub fn nouveau(id: &'static str, pos: [f64; 3], lacet: f32, uuid: [i32; 4]) -> Occupant {
        Occupant {
            id,
            pos,
            rotation: [lacet, 0.0],
            motion: [0.0, -0.0784000015258789, 0.0],
            uuid,
            traits: Vec::new(),
            passagers: Vec::new(),
        }
    }

    pub fn avec(mut self, t: Trait) -> Occupant {
        self.traits.push(t);
        self
    }

    pub fn portant(mut self, p: Occupant) -> Occupant {
        self.passagers.push(p);
        self
    }

    /// Un cadre accroché, placé comme le jeu le place : au centre de sa case,
    /// reculé de 0,46875 vers le mur qui le porte.
    ///
    /// `facing` en valeur 3D : 0 bas, 1 haut, 2 nord, 3 sud, 4 ouest, 5 est.
    pub fn cadre(
        tuile: [i32; 3],
        facing: i8,
        objet: &'static str,
        rot: i8,
        uuid: [i32; 4],
    ) -> Occupant {
        let pas = PAS_3D[facing as usize];
        let pos = [
            tuile[0] as f64 + 0.5 - pas[0] as f64 * 0.46875,
            tuile[1] as f64 + 0.5 - pas[1] as f64 * 0.46875,
            tuile[2] as f64 + 0.5 - pas[2] as f64 * 0.46875,
        ];
        // `ItemFrame.setDirection` : à l'horizontale, lacet = valeur 2D × 90 ;
        // au sol et au plafond, tangage = ∓ 90.
        let rotation = match facing {
            0 => [0.0, 90.0],
            1 => [0.0, -90.0],
            f => [VALEUR_2D_DE_3D[f as usize] as f32 * 90.0, 0.0],
        };
        let mut o = Occupant::nouveau("minecraft:item_frame", pos, rotation[0], uuid);
        o.rotation = rotation;
        o.motion = [0.0; 3];
        o.avec(Trait::Tuile(tuile))
            .avec(Trait::Facing(facing))
            .avec(Trait::Objet(objet))
            .avec(Trait::ItemRotation(rot))
    }

    /// Un tableau, placé comme `Painting.recalculateBoundingBox` le place :
    /// une largeur PAIRE décale le centre d'un demi-bloc vers la gauche de
    /// qui le regarde (le sens antihoraire de sa face).
    ///
    /// `facing` en valeur 2D : 0 sud, 1 ouest, 2 nord, 3 est.
    pub fn tableau(tuile: [i32; 3], facing: i8, motif: &'static str, uuid: [i32; 4]) -> Occupant {
        let (l, h) = taille_motif(motif);
        let pas = PAS_2D[facing as usize];
        // Le sens antihoraire de la face : sud → est, ouest → sud,
        // nord → ouest, est → nord.
        let anti = PAS_2D[((facing as usize) + 3) % 4];
        let dl = if l % 2 == 0 { 0.5 } else { 0.0 };
        let dh = if h % 2 == 0 { 0.5 } else { 0.0 };
        let pos = [
            tuile[0] as f64 + 0.5 - pas[0] as f64 * 0.46875 + dl * anti[0] as f64,
            tuile[1] as f64 + 0.5 + dh,
            tuile[2] as f64 + 0.5 - pas[2] as f64 * 0.46875 + dl * anti[2] as f64,
        ];
        let mut o = Occupant::nouveau("minecraft:painting", pos, facing as f32 * 90.0, uuid);
        o.motion = [0.0; 3];
        o.avec(Trait::Tuile(tuile))
            .avec(Trait::Facing(facing))
            .avec(Trait::Motif(motif))
    }
}

/// Le pas de chaque direction 3D, dans l'ordre du jeu : bas, haut, nord, sud,
/// ouest, est.
pub const PAS_3D: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// Le pas de chaque direction 2D : sud, ouest, nord, est.
pub const PAS_2D: [[i32; 3]; 4] = [[0, 0, 1], [-1, 0, 0], [0, 0, -1], [1, 0, 0]];

/// La valeur 2D d'une direction 3D horizontale (−1 pour haut et bas).
const VALEUR_2D_DE_3D: [i8; 6] = [-1, -1, 2, 0, 1, 3];

/// Largeur et hauteur, en blocs, de quelques motifs — une largeur de chaque
/// sorte (1, 2, 3 et 4), parce que seule la PARITÉ décide d'un décalage.
pub fn taille_motif(motif: &str) -> (i32, i32) {
    match motif {
        "minecraft:kebab" => (1, 1),
        "minecraft:pool" => (2, 1),
        "minecraft:bouquet" => (3, 3),
        "minecraft:fighters" => (4, 2),
        "minecraft:skeleton" => (4, 3),
        autre => panic!("motif absent de la fixture : {autre}"),
    }
}

fn liste_doubles(w: &mut Writer, nom: &str, v: &[f64]) {
    w.field(tag::LIST, nom).list_header(tag::DOUBLE, v.len());
    for x in v {
        w.raw(&x.to_bits().to_be_bytes());
    }
}

fn liste_flottants(w: &mut Writer, nom: &str, v: &[f32]) {
    w.field(tag::LIST, nom).list_header(tag::FLOAT, v.len());
    for x in v {
        w.raw(&x.to_bits().to_be_bytes());
    }
}

fn tableau_entiers(w: &mut Writer, nom: &str, v: &[i32]) {
    w.field(tag::INT_ARRAY, nom).i32_payload(v.len() as i32);
    for x in v {
        w.i32_payload(*x);
    }
}

fn xyz(w: &mut Writer, nom: &str, c: [i32; 3]) {
    w.field(tag::COMPOUND, nom);
    w.field(tag::INT, "X").i32_payload(c[0]);
    w.field(tag::INT, "Y").i32_payload(c[1]);
    w.field(tag::INT, "Z").i32_payload(c[2]);
    w.end();
}

/// Le compound d'une entité — ses champs puis `TAG_End`, comme un élément de
/// liste. L'ordre des champs est celui qu'écrit `Entity.saveWithoutId`, avec
/// l'`id` en tête comme le fait `saveAsPassenger`.
pub fn ecrire(o: &Occupant) -> Vec<u8> {
    let mut w = Writer::with_capacity(512);
    ecrire_dans(&mut w, o);
    w.into_bytes()
}

fn ecrire_dans(w: &mut Writer, o: &Occupant) {
    w.field(tag::STRING, "id").raw_str(o.id);
    liste_doubles(w, "Pos", &o.pos);
    liste_doubles(w, "Motion", &o.motion);
    liste_flottants(w, "Rotation", &o.rotation);
    w.field(tag::FLOAT, "FallDistance")
        .raw(&0f32.to_bits().to_be_bytes());
    w.field(tag::SHORT, "Fire").raw(&(-1i16).to_be_bytes());
    w.field(tag::SHORT, "Air").raw(&300i16.to_be_bytes());
    w.field(tag::BYTE, "OnGround").i8_payload(1);
    w.field(tag::BYTE, "Invulnerable").i8_payload(0);
    w.field(tag::INT, "PortalCooldown").i32_payload(0);
    tableau_entiers(w, "UUID", &o.uuid);
    // Un nom en hangeul : l'UTF-8 de Java doit traverser intact.
    w.field(tag::STRING, "CustomName")
        .raw_str("{\"text\":\"한국어 경비원\"}");

    // Des attributs, dont le modificateur porte SON uuid : ce n'est ni celui de
    // l'entité, ni celui de qui tient sa laisse.
    w.field(tag::LIST, "Attributes")
        .list_header(tag::COMPOUND, 1);
    w.field(tag::STRING, "Name")
        .raw_str("minecraft:generic.movement_speed");
    w.field(tag::DOUBLE, "Base")
        .raw(&0.5f64.to_bits().to_be_bytes());
    w.field(tag::LIST, "Modifiers")
        .list_header(tag::COMPOUND, 1);
    w.field(tag::STRING, "Name").raw_str("bonus");
    w.field(tag::DOUBLE, "Amount")
        .raw(&0.1f64.to_bits().to_be_bytes());
    w.field(tag::INT, "Operation").i32_payload(0);
    tableau_entiers(w, "UUID", &[11, 22, 33, 44]);
    w.end();
    w.end();

    // Les données d'un mod : un `{X, Y, Z}` que PERSONNE ne nomme. S'il
    // bougeait, le moteur aurait deviné une position sur son allure.
    w.field(tag::COMPOUND, "ForgeData");
    xyz(w, "Ancre", [7, 8, 9]);
    w.end();

    for t in &o.traits {
        match *t {
            Trait::Tuile([x, y, z]) => {
                w.field(tag::INT, "TileX").i32_payload(x);
                w.field(tag::INT, "TileY").i32_payload(y);
                w.field(tag::INT, "TileZ").i32_payload(z);
            }
            Trait::Facing(f) => {
                w.field(tag::BYTE, "Facing").i8_payload(f);
            }
            Trait::ItemRotation(r) => {
                w.field(tag::BYTE, "ItemRotation").i8_payload(r);
                w.field(tag::FLOAT, "ItemDropChance")
                    .raw(&1f32.to_bits().to_be_bytes());
            }
            Trait::Objet(id) => {
                w.field(tag::COMPOUND, "Item");
                w.field(tag::STRING, "id").raw_str(id);
                w.field(tag::BYTE, "Count").i8_payload(1);
                // L'objet a son propre `tag` : un `id` imbriqué dedans ne
                // doit pas être pris pour le sien.
                w.field(tag::COMPOUND, "tag");
                w.field(tag::STRING, "id").raw_str("leurre");
                w.end();
                w.end();
            }
            Trait::Motif(m) => {
                w.field(tag::STRING, "Motive").raw_str(m);
            }
            Trait::Pose => {
                w.field(tag::COMPOUND, "Pose");
                liste_flottants(w, "Head", &[10.0, 20.0, 0.0]);
                liste_flottants(w, "LeftArm", &[-50.0, 5.0, -30.0]);
                w.end();
            }
            Trait::Laisse(c) => xyz(w, "Leash", c),
            Trait::LaissePar(u) => {
                w.field(tag::COMPOUND, "Leash");
                tableau_entiers(w, "UUID", &u);
                w.end();
            }
            Trait::Ruche(c) => xyz(w, "HivePos", c),
            Trait::Dort([x, y, z]) => {
                w.field(tag::INT, "SleepingX").i32_payload(x);
                w.field(tag::INT, "SleepingY").i32_payload(y);
                w.field(tag::INT, "SleepingZ").i32_payload(z);
            }
            Trait::Souvenir(..) => {}
            Trait::Attache(f) => {
                w.field(tag::BYTE, "AttachFace").i8_payload(f);
            }
            Trait::Lest(n) => {
                w.field(tag::BYTE_ARRAY, "Lest").i32_payload(n as i32);
                let mut rng = crate::Rng::new(n as u32 ^ 0x5EED);
                let octets: Vec<u8> = (0..n).map(|_| rng.next_u32() as u8).collect();
                w.raw(&octets);
            }
        }
    }

    let souvenirs: Vec<_> = o
        .traits
        .iter()
        .filter_map(|t| match t {
            Trait::Souvenir(n, p, d) => Some((*n, *p, *d)),
            _ => None,
        })
        .collect();
    if !souvenirs.is_empty() {
        w.field(tag::COMPOUND, "Brain");
        w.field(tag::COMPOUND, "memories");
        for (nom, p, dim) in souvenirs {
            w.field(tag::COMPOUND, nom);
            w.field(tag::COMPOUND, "value");
            tableau_entiers(w, "pos", &p);
            w.field(tag::STRING, "dimension").raw_str(dim);
            w.end();
            w.end();
        }
        // Un souvenir qui n'est PAS une position : une date.
        w.field(tag::COMPOUND, "minecraft:last_slept");
        w.field(tag::LONG, "value").raw(&123_456i64.to_be_bytes());
        w.end();
        w.end();
        w.end();
    }

    if !o.passagers.is_empty() {
        w.field(tag::LIST, "Passengers")
            .list_header(tag::COMPOUND, o.passagers.len());
        for p in &o.passagers {
            ecrire_dans(w, p);
        }
    }
    w.end();
}

/// Un chunk d'entités complet, champs dans l'ordre inverse de celui du
/// moteur — `Entities`, `Position`, puis `DataVersion` : un compound NBT n'a
/// pas d'ordre, et un lecteur qui en supposerait un se tromperait ici.
pub fn chunk_entites(dv: i32, cx: i32, cz: i32, occupants: &[Occupant]) -> Vec<u8> {
    let mut w = Writer::with_capacity(4096);
    w.field(tag::COMPOUND, "");
    w.field(tag::LIST, "Entities");
    if occupants.is_empty() {
        w.list_header(tag::END, 0);
    } else {
        w.list_header(tag::COMPOUND, occupants.len());
        for o in occupants {
            ecrire_dans(&mut w, o);
        }
    }
    tableau_entiers(&mut w, "Position", &[cx, cz]);
    w.field(tag::INT, "DataVersion").i32_payload(dv);
    w.end();
    w.into_bytes()
}

/// Un `.mca` d'entités : un chunk par entrée `(cx, cz, dv, occupants)`, en
/// coordonnées MONDE, tous dans la région `(rx, rz)`.
pub fn region_entites(rx: i32, rz: i32, chunks: &[(i32, i32, i32, Vec<Occupant>)]) -> Vec<u8> {
    let encodes: Vec<(i32, i32, Vec<u8>)> = chunks
        .iter()
        .map(|(cx, cz, dv, occ)| (*cx, *cz, chunk_entites(*dv, *cx, *cz, occ)))
        .collect();
    crate::region_de_chunks(rx, rz, &encodes)
}
