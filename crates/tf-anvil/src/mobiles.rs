//! Les entités — cadres, tableaux, porte-armures, bêtes, villageois.
//!
//! Depuis 1.17 elles ont quitté le chunk de blocs pour `entities/r.X.Z.mca` :
//! même conteneur, autre contenu. Un chunk d'entités porte `DataVersion`,
//! `Position` (ses coordonnées de chunk) et `Entities`, une liste de compounds.
//! Le jeu n'écrit PAS de chunk d'entités vide — il le supprime — donc une
//! entité qui entre dans un chunk qui n'en portait aucune doit en CRÉER un.
//!
//! ## Rien n'est ré-encodé, là non plus
//!
//! Comme une block entity, une entité voyage par ses OCTETS. On relève où
//! vivent les champs qui la SITUENT — `Pos`, `Rotation`, `Motion`, `UUID`, la
//! case et l'orientation d'un cadre — et on ne réécrit que ceux-là, à taille
//! fixe et en place. L'inventaire d'un porte-armure, les échanges d'un
//! villageois, les données d'un mod passent sans être compris, **donc sans
//! pouvoir être abîmés**. C'est la propriété du splice, un cran plus bas.
//!
//! ## Ce qu'on relève est une TABLE, pas une heuristique
//!
//! Une entité peut se souvenir d'une case : le lit d'un villageois, la ruche
//! d'une abeille, la clôture où sa laisse est nouée. Chacune est nommée
//! ici, avec sa forme exacte ; rien n'est deviné sur l'allure d'un champ. Un
//! compound `{X, Y, Z}` qu'on ne nomme pas n'est pas une position qu'on
//! connaît — le traiter comme tel réécrirait les données d'un mod.
//!
//! ## Ce qu'on ne sait pas situer, on n'y touche pas
//!
//! Une entité sans `Pos` ne se déplace pas : on ne sait pas où elle est. Un
//! champ de la mauvaise forme (un `Pos` de deux doubles) n'est pas relevé, et
//! reste donc tel quel. La deviner la poserait ailleurs, ce qui est pire que
//! de ne rien faire.

use tf_nbt::{tag, Cur, Span, Trunc, Writer, R};

use crate::chunk::{trim_edit, Edit};

/// Le nom de la liste d'entités d'un chunk d'entités.
pub const CHAMP_ENTITES: &str = "Entities";

/// Au-delà, une chaîne de passagers est un fichier FORGÉ, pas un monde.
///
/// Le jeu n'en empile que quelques-uns (un squelette sur une araignée, un
/// poulet jockey). Sans plafond, cent mille passagers imbriqués feraient
/// récurser le balayage jusqu'à la mort du processus — et un débordement de
/// pile n'est pas rattrapable en Rust.
pub const MAX_PASSAGERS: u16 = 64;

/// Une valeur relevée, et où elle est écrite.
///
/// `at` désigne la PREMIÈRE charge : pour `Pos`, le premier des trois
/// doubles ; pour `UUID`, le premier des quatre entiers, compteur sauté.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Champ<T> {
    pub at: usize,
    pub v: T,
}

/// Une case MONDE portée par trois entiers, chacun à sa place.
///
/// Trois décalages et non un seul : `TileX`, `TileY`, `TileZ` sont trois
/// champs du compound, dans l'ordre qu'il plaît au jeu — et un `{X, Y, Z}`
/// aussi. Supposer qu'ils se suivent réécrirait le champ voisin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub at: [usize; 3],
    pub v: [i32; 3],
    /// La dimension d'une position GLOBALE (`GlobalPos` : les souvenirs d'un
    /// villageois). `None` pour une case qui est forcément dans la dimension
    /// de l'entité.
    pub dimension: Option<String>,
}

/// Ce qu'une entité porte de SITUÉ. Une par entité, passagers compris.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Corps {
    /// Son `id`, tel qu'écrit.
    pub id: Option<String>,
    pub pos: Option<Champ<[f64; 3]>>,
    /// `[lacet, tangage]`, en degrés. Seul le lacet tourne avec le monde.
    pub rotation: Option<Champ<[f32; 2]>>,
    pub motion: Option<Champ<[f64; 3]>>,
    pub uuid: Option<Champ<[i32; 4]>>,
    /// `TileX/Y/Z` : la case d'une entité ACCROCHÉE (cadre, tableau, nœud de
    /// laisse). Le jeu RECALCULE `Pos` depuis elle au chargement : c'est elle
    /// qui décide, pas `Pos`.
    pub tuile: Option<Case>,
    /// `Facing` (ou `facing`) : l'orientation d'un cadre — sur trois axes,
    /// 0..5 — ou d'un tableau — à l'horizontale, 0..3. Le sens dépend de
    /// l'`id`, que ce crate ne tranche pas.
    pub facing: Option<Champ<i8>>,
    /// `AttachFace` : la face à laquelle un shulker s'accroche, 0..5.
    pub attache: Option<Champ<i8>>,
    /// `ItemRotation` : la rotation de l'objet DANS un cadre, 0..7.
    pub rotation_objet: Option<Champ<i8>>,
    /// L'`id` de l'objet d'un cadre. Une carte ne tourne que par quarts de
    /// tour, les autres objets par huitièmes : la même valeur ne veut pas dire
    /// la même chose.
    pub objet: Option<String>,
    /// Le motif d'un tableau (`Motive` en 1.18, `variant` ensuite). Sa
    /// LARGEUR décide d'un décalage sous miroir.
    pub motif: Option<String>,
    /// Un porte-armure qui porte une `Pose` : ses membres ont des angles
    /// qu'un miroir devrait refléter.
    pub pose: bool,
    /// Les cases dont l'entité SE SOUVIENT : son lit, sa ruche, la clôture de
    /// sa laisse, son poste de travail.
    pub retenues: Vec<Case>,
    /// L'`UUID` de l'entité qui tient sa laisse, quand ce n'est pas une
    /// clôture.
    pub laisse_uuid: Option<Champ<[i32; 4]>>,
}

impl Corps {
    fn decaler(&mut self, d: usize) {
        fn c<T>(x: &mut Option<Champ<T>>, d: usize) {
            if let Some(x) = x {
                x.at -= d;
            }
        }
        fn k(x: &mut Case, d: usize) {
            for a in &mut x.at {
                *a -= d;
            }
        }
        c(&mut self.pos, d);
        c(&mut self.rotation, d);
        c(&mut self.motion, d);
        c(&mut self.uuid, d);
        c(&mut self.facing, d);
        c(&mut self.attache, d);
        c(&mut self.rotation_objet, d);
        c(&mut self.laisse_uuid, d);
        if let Some(t) = &mut self.tuile {
            k(t, d);
        }
        for r in &mut self.retenues {
            k(r, d);
        }
    }
}

/// Une entité repérée dans un chunk, sans rien matérialiser.
#[derive(Debug, Clone, PartialEq)]
pub struct MobileRepere {
    /// Le compound ENTIER, `TAG_End` compris.
    pub span: Span,
    /// Elle d'abord, puis ses passagers dans l'ordre de l'arbre. Décalages
    /// ABSOLUS dans le tampon inflaté.
    pub corps: Vec<Corps>,
}

impl MobileRepere {
    /// Sa position, quand elle en a une.
    pub fn pos(&self) -> Option<[f64; 3]> {
        self.corps.first()?.pos.map(|p| p.v)
    }
}

/// Ce qu'un chunk d'entités porte, repéré sans rien matérialiser.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChunkMobiles {
    pub data_version: Option<i32>,
    /// `Position` : les coordonnées de CHUNK, telles qu'écrites.
    pub position: Option<[i32; 2]>,
    /// Le CHAMP `Entities` entier — type, nom et charge. `None` s'il manque.
    pub champ: Option<Span>,
    /// Le `TAG_End` de la racine : là où insérer la liste si elle manque.
    pub inserer_a: usize,
    pub entrees: Vec<MobileRepere>,
}

/// Balaye un chunk d'ENTITÉS inflaté.
pub fn balayer_chunk(inflated: &[u8]) -> R<ChunkMobiles> {
    let mut c = Cur::new(inflated);
    c.enter_root()?;
    let mut out = ChunkMobiles::default();
    loop {
        let avant = c.pos();
        let Some((t, key)) = c.next_field()? else {
            out.inserer_a = avant;
            break;
        };
        match (t, key) {
            (tag::INT, "DataVersion") => out.data_version = Some(c.i32()?),
            (tag::INT_ARRAY, "Position") => {
                let n = c.array_len()?;
                if n == 2 {
                    out.position = Some([c.i32()?, c.i32()?]);
                } else {
                    c.skip(n.checked_mul(4).ok_or(Trunc)?)?;
                }
            }
            (tag::LIST, CHAMP_ENTITES) => {
                out.entrees = balayer_liste(&mut c)?;
                out.champ = Some(Span {
                    start: avant,
                    end: c.pos(),
                });
            }
            _ => c.skip_payload(t)?,
        }
    }
    Ok(out)
}

/// Balaye une liste d'entités, le curseur étant sur son EN-TÊTE.
///
/// Sert aussi, telle quelle, à la liste `Entities` que les chunks de BLOCS
/// portaient avant 1.17.
pub fn balayer_liste(c: &mut Cur) -> R<Vec<MobileRepere>> {
    let (et, n) = c.list_header()?;
    // Une liste vide s'écrit avec `TAG_End` pour type d'élément, ou avec
    // `TAG_Compound` et une longueur nulle : les deux sont vides.
    if et == tag::END || n == 0 {
        return Ok(Vec::new());
    }
    if et != tag::COMPOUND {
        return Err(Trunc);
    }
    // La longueur vient du FICHIER : on ne la réserve pas telle quelle.
    let mut out = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        let start = c.pos();
        let mut corps = vec![Corps::default()];
        lire_corps(c, &mut corps, 0, 0)?;
        out.push(MobileRepere {
            span: Span {
                start,
                end: c.pos(),
            },
            corps,
        });
    }
    Ok(out)
}

/// Les triplets d'entiers qu'une entité peut porter à plat, et ce qu'ils
/// désignent. `TileX/Y/Z` est à part : c'est la case de l'entité elle-même.
const TRIPLETS: [[&str; 3]; 6] = [
    // Un dormeur : la case de son LIT.
    ["SleepingX", "SleepingY", "SleepingZ"],
    // Une tortue : sa plage, et le but de son voyage.
    ["HomePosX", "HomePosY", "HomePosZ"],
    ["TravelPosX", "TravelPosY", "TravelPosZ"],
    // Un dauphin : le trésor qu'il guide.
    ["TreasurePosX", "TreasurePosY", "TreasurePosZ"],
    // Un vex : la zone où il rôde.
    ["BoundX", "BoundY", "BoundZ"],
    // Un phantom : le point autour duquel il tourne.
    ["AX", "AY", "AZ"],
];

/// Les compounds `{X, Y, Z}` qui désignent une case (jusqu'à 1.20.4).
const COMPOUNDS_CASE: [&str; 4] = ["HivePos", "FlowerPos", "WanderTarget", "BeamTarget"];

/// Les tableaux `[I; x, y, z]` qui les ont remplacés (1.20.5+).
const TABLEAUX_CASE: [&str; 9] = [
    "leash",
    "hive_pos",
    "flower_pos",
    "wander_target",
    "beam_target",
    "bound_pos",
    "home_pos",
    "travel_pos",
    "sleeping_pos",
];

/// Un triplet en cours de lecture : ses trois champs peuvent venir dans
/// n'importe quel ordre, et l'un peut manquer.
#[derive(Default, Clone, Copy)]
struct Partiel {
    at: [Option<usize>; 3],
    v: [i32; 3],
}

impl Partiel {
    fn noter(&mut self, k: usize, at: usize, v: i32) {
        self.at[k] = Some(at);
        self.v[k] = v;
    }
    fn fini(&self, dimension: Option<String>) -> Option<Case> {
        Some(Case {
            at: [self.at[0]?, self.at[1]?, self.at[2]?],
            v: self.v,
            dimension,
        })
    }
}

/// Lit les champs d'une entité, le curseur étant sur son PREMIER champ.
///
/// `rang` désigne son `Corps` dans `liste` ; ses passagers y sont AJOUTÉS,
/// dans l'ordre de l'arbre.
fn lire_corps(c: &mut Cur, liste: &mut Vec<Corps>, rang: usize, profondeur: u16) -> R<()> {
    if profondeur > MAX_PASSAGERS {
        return Err(Trunc);
    }
    let mut tuile = Partiel::default();
    let mut triplets = [Partiel::default(); TRIPLETS.len()];

    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::STRING, "id") => liste[rang].id = Some(c.str()?.to_string()),
            (tag::LIST, "Pos") => liste[rang].pos = doubles::<3>(c)?,
            (tag::LIST, "Motion") => liste[rang].motion = doubles::<3>(c)?,
            (tag::LIST, "Rotation") => liste[rang].rotation = flottants::<2>(c)?,
            (tag::INT_ARRAY, "UUID") => liste[rang].uuid = entiers::<4>(c)?,
            (tag::INT, "TileX") => tuile.noter(0, c.pos(), c.i32()?),
            (tag::INT, "TileY") => tuile.noter(1, c.pos(), c.i32()?),
            (tag::INT, "TileZ") => tuile.noter(2, c.pos(), c.i32()?),
            (tag::BYTE, "Facing" | "facing") => liste[rang].facing = Some(octet(c)?),
            (tag::BYTE, "AttachFace") => liste[rang].attache = Some(octet(c)?),
            (tag::BYTE, "ItemRotation") => liste[rang].rotation_objet = Some(octet(c)?),
            (tag::COMPOUND, "Item") => liste[rang].objet = id_de(c)?,
            (tag::STRING, "Motive" | "variant") => {
                liste[rang].motif = Some(c.str()?.to_string());
            }
            (tag::COMPOUND, "Pose") => {
                liste[rang].pose = true;
                c.skip_payload(t)?;
            }
            (tag::LIST, "Passengers") => {
                let (et, n) = c.list_header()?;
                if et != tag::COMPOUND {
                    c.skip_list_body(et, n)?;
                    continue;
                }
                for _ in 0..n {
                    liste.push(Corps::default());
                    let r = liste.len() - 1;
                    lire_corps(c, liste, r, profondeur + 1)?;
                }
            }
            (tag::COMPOUND, "Leash" | "leash") => {
                let (case, uuid) = xyz_ou_uuid(c)?;
                if let Some(case) = case {
                    liste[rang].retenues.push(case);
                }
                if uuid.is_some() {
                    liste[rang].laisse_uuid = uuid;
                }
            }
            (tag::COMPOUND, k) if COMPOUNDS_CASE.contains(&k) => {
                let (case, _) = xyz_ou_uuid(c)?;
                liste[rang].retenues.extend(case);
            }
            (tag::INT_ARRAY, k) if TABLEAUX_CASE.contains(&k) => {
                if let Some(e) = entiers::<3>(c)? {
                    liste[rang].retenues.push(Case {
                        at: [e.at, e.at + 4, e.at + 8],
                        v: e.v,
                        dimension: None,
                    });
                }
            }
            (tag::INT, k) if TRIPLETS.iter().any(|tr| tr.contains(&k)) => {
                let at = c.pos();
                let v = c.i32()?;
                for (i, tr) in TRIPLETS.iter().enumerate() {
                    if let Some(j) = tr.iter().position(|n| *n == k) {
                        triplets[i].noter(j, at, v);
                    }
                }
            }
            (tag::COMPOUND, "Brain") => souvenirs(c, &mut liste[rang].retenues)?,
            _ => c.skip_payload(t)?,
        }
    }
    liste[rang].tuile = tuile.fini(None);
    for p in triplets {
        liste[rang].retenues.extend(p.fini(None));
    }
    Ok(())
}

/// Une liste de `N` doubles, le curseur sur son en-tête. `None` si elle n'a
/// pas cette forme — elle est alors sautée, et restera telle quelle.
fn doubles<const N: usize>(c: &mut Cur) -> R<Option<Champ<[f64; N]>>> {
    let (et, n) = c.list_header()?;
    if et != tag::DOUBLE || n != N {
        c.skip_list_body(et, n)?;
        return Ok(None);
    }
    let at = c.pos();
    let mut v = [0.0; N];
    for x in &mut v {
        *x = f64::from_bits(c.u64()?);
    }
    Ok(Some(Champ { at, v }))
}

fn flottants<const N: usize>(c: &mut Cur) -> R<Option<Champ<[f32; N]>>> {
    let (et, n) = c.list_header()?;
    if et != tag::FLOAT || n != N {
        c.skip_list_body(et, n)?;
        return Ok(None);
    }
    let at = c.pos();
    let mut v = [0.0; N];
    for x in &mut v {
        *x = f32::from_bits(c.i32()? as u32);
    }
    Ok(Some(Champ { at, v }))
}

/// Un `TAG_Int_Array` de `N` entiers, le curseur sur son compteur.
fn entiers<const N: usize>(c: &mut Cur) -> R<Option<Champ<[i32; N]>>> {
    let n = c.array_len()?;
    if n != N {
        c.skip(n.checked_mul(4).ok_or(Trunc)?)?;
        return Ok(None);
    }
    let at = c.pos();
    let mut v = [0; N];
    for x in &mut v {
        *x = c.i32()?;
    }
    Ok(Some(Champ { at, v }))
}

fn octet(c: &mut Cur) -> R<Champ<i8>> {
    let at = c.pos();
    Ok(Champ { at, v: c.i8()? })
}

/// L'`id` d'un compound, le curseur sur son premier champ.
fn id_de(c: &mut Cur) -> R<Option<String>> {
    let mut id = None;
    while let Some((t, key)) = c.next_field()? {
        if t == tag::STRING && key == "id" {
            id = Some(c.str()?.to_string());
        } else {
            c.skip_payload(t)?;
        }
    }
    Ok(id)
}

/// Un compound `{X, Y, Z}` — ou `{UUID}` pour une laisse tenue par une
/// entité. Le curseur est sur son premier champ.
#[allow(clippy::type_complexity)]
fn xyz_ou_uuid(c: &mut Cur) -> R<(Option<Case>, Option<Champ<[i32; 4]>>)> {
    let mut p = Partiel::default();
    let mut uuid = None;
    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::INT, "X") => p.noter(0, c.pos(), c.i32()?),
            (tag::INT, "Y") => p.noter(1, c.pos(), c.i32()?),
            (tag::INT, "Z") => p.noter(2, c.pos(), c.i32()?),
            (tag::INT_ARRAY, "UUID") => uuid = entiers::<4>(c)?,
            _ => c.skip_payload(t)?,
        }
    }
    Ok((p.fini(None), uuid))
}

/// `Brain.memories` : les souvenirs d'une entité, dont certains sont des
/// POSITIONS GLOBALES — le lit, le poste de travail, le point de rendez-vous
/// d'un villageois. Le curseur est sur le premier champ de `Brain`.
fn souvenirs(c: &mut Cur, out: &mut Vec<Case>) -> R<()> {
    while let Some((t, key)) = c.next_field()? {
        if !(t == tag::COMPOUND && key == "memories") {
            c.skip_payload(t)?;
            continue;
        }
        // Chaque souvenir, quel que soit son nom : c'est la FORME de sa
        // valeur qui dit si c'est une position, et elle est exacte —
        // `{pos: [I; x, y, z], dimension: "…"}`.
        while let Some((t, _)) = c.next_field()? {
            if t != tag::COMPOUND {
                c.skip_payload(t)?;
                continue;
            }
            while let Some((t, key)) = c.next_field()? {
                if t == tag::COMPOUND && key == "value" {
                    out.extend(position_globale(c)?);
                } else {
                    c.skip_payload(t)?;
                }
            }
        }
    }
    Ok(())
}

/// `{pos: [I; x, y, z], dimension: "…"}`, le curseur sur son premier champ.
/// Tout autre forme n'est pas une position qu'on connaît.
fn position_globale(c: &mut Cur) -> R<Option<Case>> {
    let mut pos = None;
    let mut dimension = None;
    let mut autre = false;
    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::INT_ARRAY, "pos") => pos = entiers::<3>(c)?,
            (tag::STRING, "dimension") => dimension = Some(c.str()?.to_string()),
            _ => {
                autre = true;
                c.skip_payload(t)?;
            }
        }
    }
    Ok(match (pos, dimension, autre) {
        (Some(p), Some(d), false) => Some(Case {
            at: [p.at, p.at + 4, p.at + 8],
            v: p.v,
            dimension: Some(d),
        }),
        _ => None,
    })
}

/// Une entité détachée de son chunk, prête à voyager — passagers compris.
///
/// `nbt` sont les octets d'ORIGINE du compound. Les `Corps` disent où y
/// réécrire ce qui la situe, et ce qu'il faut y écrire : poser l'entité,
/// c'est recopier `nbt` et remplacer ces octets-là.
#[derive(Debug, Clone, PartialEq)]
pub struct Mobile {
    pub nbt: Vec<u8>,
    /// Décalages RELATIFS à `nbt`. Le repère des valeurs est celui de qui la
    /// porte : monde dans un chunk, local dans un presse-papiers.
    pub corps: Vec<Corps>,
    /// Le `DataVersion` du chunk d'où elle vient : la FORME de ses octets.
    pub data_version: Option<i32>,
}

impl Mobile {
    /// L'entité repérée, détachée du tampon.
    pub fn depuis(inflated: &[u8], r: &MobileRepere, data_version: Option<i32>) -> Mobile {
        let mut corps = r.corps.clone();
        // Absolus dans le tampon, relatifs dans `nbt` : la conversion se fait
        // ici et nulle part ailleurs.
        for k in &mut corps {
            k.decaler(r.span.start);
        }
        Mobile {
            nbt: r.span.slice(inflated).to_vec(),
            corps,
            data_version,
        }
    }

    pub fn pos(&self) -> Option<[f64; 3]> {
        self.corps.first()?.pos.map(|p| p.v)
    }

    pub fn uuid(&self) -> Option<[i32; 4]> {
        self.corps.first()?.uuid.map(|u| u.v)
    }

    pub fn id(&self) -> Option<&str> {
        self.corps.first()?.id.as_deref()
    }

    /// Ses octets, chaque champ situé réécrit à sa valeur.
    ///
    /// Une entité qu'on n'a pas touchée ressort à l'OCTET près : on relit
    /// les bits des doubles, on ne les recalcule pas.
    pub fn octets(&self) -> Vec<u8> {
        let mut out = self.nbt.clone();
        for k in &self.corps {
            if let Some(p) = k.pos {
                for (i, x) in p.v.iter().enumerate() {
                    poser(&mut out, p.at + 8 * i, &x.to_bits().to_be_bytes());
                }
            }
            if let Some(p) = k.motion {
                for (i, x) in p.v.iter().enumerate() {
                    poser(&mut out, p.at + 8 * i, &x.to_bits().to_be_bytes());
                }
            }
            if let Some(r) = k.rotation {
                for (i, x) in r.v.iter().enumerate() {
                    poser(&mut out, r.at + 4 * i, &x.to_bits().to_be_bytes());
                }
            }
            for u in [k.uuid, k.laisse_uuid].into_iter().flatten() {
                for (i, x) in u.v.iter().enumerate() {
                    poser(&mut out, u.at + 4 * i, &x.to_be_bytes());
                }
            }
            for o in [k.facing, k.attache, k.rotation_objet]
                .into_iter()
                .flatten()
            {
                poser(&mut out, o.at, &[o.v as u8]);
            }
            for c in k.tuile.iter().chain(&k.retenues) {
                for i in 0..3 {
                    poser(&mut out, c.at[i], &c.v[i].to_be_bytes());
                }
            }
        }
        out
    }
}

/// Écrit `b` à `at`, si ça tient. Un décalage hors de `nbt` désigne un champ
/// qu'on ne sait pas situer : on n'écrit rien plutôt que de paniquer sur la
/// save de quelqu'un.
fn poser(out: &mut [u8], at: usize, b: &[u8]) {
    if at.checked_add(b.len()).is_some_and(|fin| fin <= out.len()) {
        out[at..at + b.len()].copy_from_slice(b);
    }
}

/// Le CHAMP `Entities` complet — type, nom et charge.
pub fn champ_mobiles(entites: &[Vec<u8>]) -> Vec<u8> {
    let poids: usize = entites.iter().map(Vec::len).sum();
    let mut w = Writer::with_capacity(16 + poids);
    w.field(tag::LIST, CHAMP_ENTITES);
    if entites.is_empty() {
        // Ce qu'écrit le jeu pour une liste vide : `TAG_End` pour type.
        w.list_header(tag::END, 0);
    } else {
        w.list_header(tag::COMPOUND, entites.len());
        for e in entites {
            w.raw(e);
        }
    }
    w.into_bytes()
}

/// Un chunk d'entités NEUF : `DataVersion`, `Position`, `Entities`.
///
/// Le `DataVersion` est celui des entités qu'il reçoit — c'est la forme de
/// leurs octets, et le jeu les mettra à jour depuis là. En inventer un autre
/// lui ferait sauter ou rejouer des conversions.
pub fn chunk_neuf(data_version: i32, cx: i32, cz: i32, entites: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::new();
    w.raw(&[tag::COMPOUND]).raw_str("");
    w.field(tag::INT, "DataVersion").i32_payload(data_version);
    w.field(tag::INT_ARRAY, "Position")
        .i32_payload(2)
        .i32_payload(cx)
        .i32_payload(cz);
    w.raw(&champ_mobiles(entites));
    w.end();
    w.into_bytes()
}

/// L'édition qui remplace la liste d'entités d'un chunk existant.
///
/// Rend `None` quand rien ne change — ce sont les OCTETS qui décident, pas un
/// drapeau : une opération qui ne déplace aucune entité ne salit pas le chunk,
/// et ne remplit pas le journal d'entrées vides. Une liste vide qui le reste
/// ne produit rien, même écrite sous une forme que nous n'écririons pas.
pub fn edition_mobiles(inflated: &[u8], chunk: &ChunkMobiles, voulues: &[Vec<u8>]) -> Option<Edit> {
    if voulues.is_empty() && chunk.entrees.is_empty() {
        return None;
    }
    let bytes = champ_mobiles(voulues);
    let mut e = match chunk.champ {
        Some(span) => Edit { span, bytes },
        // Le chunk n'en portait pas : on insère le CHAMP entier juste avant
        // le `TAG_End` de la racine.
        None => Edit {
            span: Span {
                start: chunk.inserer_a,
                end: chunk.inserer_a,
            },
            bytes,
        },
    };
    trim_edit(inflated, &mut e).then_some(e)
}
