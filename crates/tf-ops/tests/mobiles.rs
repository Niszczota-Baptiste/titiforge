//! **Un build déplacé emporte ses entités** — cadres, tableaux, porte-armures,
//! bêtes, villageois.
//!
//! Depuis 1.17 elles vivent dans `entities/`, un dossier que rien ne relie aux
//! blocs. Ce fichier tient la jonction : la copie ramasse en MONDE, le
//! presse-papiers range en LOCAL, la pose écrit en MONDE — et ce qu'elle écrit
//! est relu par le décodeur GELÉ de `tf-anvil`, pas par le balayage du moteur.
//! Un test qui relirait avec le code qu'il vérifie ne prouverait que leur
//! accord.

#[path = "../../tf-anvil/tests/common/frozen.rs"]
mod frozen;

use std::collections::{BTreeMap, BTreeSet};

use frozen::Tag;
use tf_anvil::mobiles::{balayer_chunk, Mobile};
use tf_anvil::Interner;
use tf_bench::mobiles::{chunk_entites, region_entites, taille_motif, Occupant, Trait, DV_1_18_2};
use tf_bench::{region, Terrain};
use tf_blocks::Transfo;
use tf_ops::catalogue::{construire, Params, Valeur};
use tf_ops::edition::{copier, rejouer, Sens};
use tf_ops::executer::{executer, CompteRendu, Options};
use tf_ops::mobiles::{
    face2_apres, face3_apres, lacet_apres, position_apres, rotation_objet_apres,
    transformer_mobile, uuid_derive, vecteur_apres, PAS_2D, PAS_3D,
};
use tf_ops::TransfoBoite;
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

const STAND: [i32; 4] = [1, 2, 3, 4];
const CADRE_MUR: [i32; 4] = [5, 6, 7, 8];
const CADRE_SOL: [i32; 4] = [9, 10, 11, 12];
const TABLEAU: [i32; 4] = [13, 14, 15, 16];
const VILLAGEOIS: [i32; 4] = [17, 18, 19, 20];
const COCHON: [i32; 4] = [21, 22, 23, 24];
const POULET: [i32; 4] = [25, 26, 27, 28];
const ZOMBIE: [i32; 4] = [29, 30, 31, 32];
const CADRE_VOISIN: [i32; 4] = [33, 34, 35, 36];
const CADRE_FACADE: [i32; 4] = [37, 38, 39, 40];
const MARCHAND: [i32; 4] = [41, 42, 43, 44];
const LAMA: [i32; 4] = [45, 46, 47, 48];
const VACHE: [i32; 4] = [49, 50, 51, 52];
const CHAUVE_SOURIS: [i32; 4] = [53, 54, 55, 56];

/// Le chunk (0, 0) : ce que la sélection doit emporter, plus un piège.
fn chunk_zero() -> Vec<Occupant> {
    vec![
        Occupant::nouveau("minecraft:armor_stand", [5.5, 64.0, 5.5], 30.0, STAND).avec(Trait::Pose),
        // Au mur SUD d'un bloc : accroché à (8, 65, 2).
        Occupant::cadre([8, 65, 3], 3, "minecraft:filled_map", 3, CADRE_MUR),
        // Au sol : posé sur (9, 63, 9).
        Occupant::cadre([9, 64, 9], 1, "minecraft:diamond", 1, CADRE_SOL),
        // Deux blocs de large : son centre est décalé d'un demi-bloc.
        Occupant::tableau([12, 66, 4], 0, "minecraft:pool", TABLEAU),
        Occupant::nouveau("minecraft:villager", [3.5, 64.0, 10.5], -90.0, VILLAGEOIS)
            .avec(Trait::Dort([3, 64, 11]))
            .avec(Trait::Souvenir(
                "minecraft:home",
                [3, 64, 11],
                "minecraft:overworld",
            ))
            // Son poste de travail est dans un AUTRE bâtiment.
            .avec(Trait::Souvenir(
                "minecraft:job_site",
                [200, 64, 200],
                "minecraft:overworld",
            ))
            // Et un souvenir d'une autre dimension, aux mêmes nombres qu'une
            // case de la sélection.
            .avec(Trait::Souvenir(
                "minecraft:meeting_point",
                [4, 64, 4],
                "minecraft:the_nether",
            )),
        Occupant {
            // Une vitesse HORIZONTALE : une rotation qui l'oublierait ne se
            // verrait sur aucune autre entité, toutes au repos.
            motion: [0.125, 0.0, 0.25],
            ..Occupant::nouveau("minecraft:pig", [6.5, 64.0, 12.5], 0.0, COCHON)
        }
        .portant(
            Occupant::nouveau("minecraft:chicken", [6.5, 64.9, 12.5], 0.0, POULET).portant(
                Occupant::nouveau("minecraft:zombie", [6.5, 65.5, 12.5], 0.0, ZOMBIE),
            ),
        ),
        // LE PIÈGE : sa case est dans la sélection (x = 15), mais il est
        // accroché au mur OUEST de (16, 65, 7), qui n'y est pas.
        Occupant::cadre([15, 65, 7], 4, "minecraft:clock", 0, CADRE_VOISIN),
    ]
}

/// Le chunk (1, 0).
fn chunk_un() -> Vec<Occupant> {
    vec![
        // Accroché à la face EST du dernier mur de la sélection (15, 65, 5) :
        // sa case est hors de la sélection, et même dans le chunk d'à côté.
        Occupant::cadre([16, 65, 5], 5, "minecraft:clock", 0, CADRE_FACADE),
        Occupant::nouveau(
            "minecraft:wandering_trader",
            [20.5, 64.0, 6.5],
            0.0,
            MARCHAND,
        ),
        Occupant::nouveau("minecraft:trader_llama", [22.5, 64.0, 6.5], 0.0, LAMA)
            .avec(Trait::LaissePar(MARCHAND)),
    ]
}

fn monde() -> Staging<MemorySource, MemorySource> {
    let m = MemorySource::new();
    m.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    m.put_region(
        SURFACE,
        Folder::Entities,
        ZERO,
        region_entites(
            0,
            0,
            &[
                (0, 0, DV_1_18_2, chunk_zero()),
                (1, 0, DV_1_18_2, chunk_un()),
                (
                    3,
                    3,
                    DV_1_18_2,
                    vec![Occupant::nouveau(
                        "minecraft:cow",
                        [55.5, 64.0, 55.5],
                        0.0,
                        VACHE,
                    )],
                ),
                // Un chunk écrit par une AUTRE version du jeu (1.20.1).
                (
                    5,
                    0,
                    3465,
                    vec![Occupant::nouveau(
                        "minecraft:bat",
                        [85.5, 70.0, 5.5],
                        0.0,
                        CHAUVE_SOURIS,
                    )],
                ),
            ],
        ),
    );
    Staging::new(m, MemorySource::new())
}

/// Le chunk (0, 0), sur la hauteur des entités.
fn sel() -> BBox {
    BBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 80, 15))
}

/// Ce que la sélection doit emporter — et seulement ça.
fn emportees() -> BTreeSet<[i32; 4]> {
    [
        STAND,
        CADRE_MUR,
        CADRE_SOL,
        TABLEAU,
        VILLAGEOIS,
        COCHON,
        CADRE_FACADE,
    ]
    .into_iter()
    .collect()
}

// ── relire, avec le décodeur GELÉ ───────────────────────────────────────────

/// Une entité telle que le décodeur gelé la relit.
#[derive(Debug, Clone)]
struct Vue {
    chunk: (u32, u32),
    tag: Tag,
}

impl Vue {
    fn uuid(&self) -> [i32; 4] {
        match self.tag.get("UUID") {
            Some(Tag::IntArray(v)) if v.len() == 4 => [v[0], v[1], v[2], v[3]],
            autre => panic!("UUID illisible : {autre:?}"),
        }
    }
    fn pos(&self) -> [f64; 3] {
        doubles(self.tag.get("Pos").expect("Pos"))
    }
    fn id(&self) -> &str {
        self.tag.get("id").and_then(Tag::as_str).expect("id")
    }
}

fn doubles(t: &Tag) -> [f64; 3] {
    match t.as_list().map(|l| l.as_slice()) {
        Some([Tag::Double(a), Tag::Double(b), Tag::Double(c)]) => [*a, *b, *c],
        autre => panic!("trois doubles attendus : {autre:?}"),
    }
}

fn entier(t: &Tag, k: &str) -> i32 {
    t.get(k)
        .and_then(Tag::as_i32)
        .unwrap_or_else(|| panic!("{k} absent"))
}

fn octet(t: &Tag, k: &str) -> i8 {
    t.get(k)
        .and_then(Tag::as_i8)
        .unwrap_or_else(|| panic!("{k} absent"))
}

fn lacet(t: &Tag) -> f32 {
    match t
        .get("Rotation")
        .and_then(Tag::as_list)
        .map(|l| l.as_slice())
    {
        Some([Tag::Float(l), Tag::Float(_)]) => *l,
        autre => panic!("Rotation illisible : {autre:?}"),
    }
}

/// Toutes les entités RACINES du dossier `entities/`, chunk par chunk, dans
/// l'ordre des listes.
fn entites(st: &Staging<MemorySource, MemorySource>) -> Vec<Vue> {
    let octets = st.read_region(&SURFACE, Folder::Entities, ZERO).unwrap();
    let mut out = Vec::new();
    for ((lx, lz), c) in frozen::decode_region(&octets) {
        let liste = c
            .root
            .get("Entities")
            .and_then(Tag::as_list)
            .cloned()
            .unwrap_or_default();
        for tag in liste {
            out.push(Vue {
                chunk: (lx, lz),
                tag,
            });
        }
    }
    out
}

fn par_uuid(v: &[Vue]) -> BTreeMap<[i32; 4], Vue> {
    v.iter().map(|e| (e.uuid(), e.clone())).collect()
}

/// Les octets inflatés de chaque chunk d'entités — la vérité qu'on compare.
fn contenu(st: &Staging<MemorySource, MemorySource>) -> BTreeMap<(u32, u32), Tag> {
    let octets = st.read_region(&SURFACE, Folder::Entities, ZERO).unwrap();
    frozen::decode_region(&octets)
        .into_iter()
        .map(|(k, c)| (k, c.root))
        .collect()
}

/// Un tag sans ce qui SITUE l'entité : ce qui reste doit voyager intact.
fn sans_situation(t: &Tag) -> Tag {
    const SITUE: [&str; 13] = [
        "Pos",
        "Rotation",
        "Motion",
        "UUID",
        "TileX",
        "TileY",
        "TileZ",
        "Facing",
        "ItemRotation",
        "SleepingX",
        "SleepingY",
        "SleepingZ",
        "Brain",
    ];
    match t {
        Tag::Compound(v) => Tag::Compound(
            v.iter()
                .filter(|(k, _)| !SITUE.contains(&k.as_str()))
                .map(|(k, x)| (k.clone(), sans_situation(x)))
                .collect(),
        ),
        Tag::List(v) => Tag::List(v.iter().map(sans_situation).collect()),
        x => x.clone(),
    }
}

// ── exécuter comme un hôte ──────────────────────────────────────────────────

fn lancer(st: &Staging<MemorySource, MemorySource>, op: &str, p: &Params, s: &BBox) -> CompteRendu {
    let mut i = Interner::new();
    let t = construire(op, p, &mut i).unwrap();
    executer(
        &t,
        st,
        &SURFACE,
        Folder::Region,
        s,
        &mut i,
        &Options::default(),
    )
    .unwrap()
}

fn copier_vers(d: [i32; 3], t: Option<Transfo>) -> Params {
    let mut p = Params::new();
    p.poser("decalage", Valeur::Vecteur(d));
    p.poser("transformation", Valeur::Transformation(t));
    p
}

fn deplacer(d: [i32; 3]) -> Params {
    let mut p = Params::new();
    p.poser("decalage", Valeur::Vecteur(d));
    p.poser("remplir", Valeur::texte("minecraft:air"));
    p
}

fn correctifs_d_entites(cr: &CompteRendu) -> usize {
    cr.rapport
        .patches
        .iter()
        .filter(|p| p.cible.folder == Folder::Entities)
        .count()
}

// ── le balayage ─────────────────────────────────────────────────────────────

#[test]
fn le_balayage_releve_ce_qui_situe_une_entite_et_rien_d_autre() {
    let nbt = chunk_entites(DV_1_18_2, 0, 0, &chunk_zero());
    let ch = balayer_chunk(&nbt).unwrap();
    assert_eq!(ch.data_version, Some(DV_1_18_2));
    assert_eq!(ch.position, Some([0, 0]));
    assert_eq!(ch.entrees.len(), chunk_zero().len());

    let a = &ch.entrees[0].corps[0];
    assert_eq!(a.id.as_deref(), Some("minecraft:armor_stand"));
    assert_eq!(a.pos.unwrap().v, [5.5, 64.0, 5.5]);
    assert_eq!(a.rotation.unwrap().v, [30.0, 0.0]);
    assert_eq!(
        a.uuid.unwrap().v,
        STAND,
        "l'UUID de l'entité, pas celui du modificateur d'attribut"
    );
    assert!(a.pose);
    assert!(
        a.retenues.is_empty(),
        "le {{X, Y, Z}} d'un mod n'est pas une position qu'on connaît"
    );
    assert!(a.laisse_uuid.is_none());

    let f = &ch.entrees[1].corps[0];
    assert_eq!(f.tuile.as_ref().unwrap().v, [8, 65, 3]);
    assert_eq!(f.facing.unwrap().v, 3);
    assert_eq!(f.rotation_objet.unwrap().v, 3);
    assert_eq!(
        f.objet.as_deref(),
        Some("minecraft:filled_map"),
        "l'id de l'OBJET, pas celui de son tag"
    );

    assert_eq!(
        ch.entrees[3].corps[0].motif.as_deref(),
        Some("minecraft:pool")
    );

    let v = &ch.entrees[4].corps[0];
    let mut r: Vec<([i32; 3], Option<String>)> = v
        .retenues
        .iter()
        .map(|c| (c.v, c.dimension.clone()))
        .collect();
    r.sort();
    let mut voulu = vec![
        ([3, 64, 11], None),
        ([3, 64, 11], Some("minecraft:overworld".to_string())),
        ([200, 64, 200], Some("minecraft:overworld".to_string())),
        ([4, 64, 4], Some("minecraft:the_nether".to_string())),
    ];
    voulu.sort();
    assert_eq!(
        r, voulu,
        "le lit, et les trois souvenirs qui sont des POSITIONS"
    );

    let ids: Vec<_> = ch.entrees[5]
        .corps
        .iter()
        .map(|k| k.id.clone().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["minecraft:pig", "minecraft:chicken", "minecraft:zombie"],
        "une monture, puis ses passagers dans l'ordre de l'arbre"
    );
    assert_eq!(ch.entrees[5].corps[2].uuid.unwrap().v, ZOMBIE);

    let l = balayer_chunk(&chunk_entites(DV_1_18_2, 1, 0, &chunk_un())).unwrap();
    assert_eq!(l.entrees[2].corps[0].laisse_uuid.unwrap().v, MARCHAND);
}

#[test]
fn une_entite_qu_on_ne_touche_pas_ressort_a_l_octet_pres() {
    for (cx, occ) in [(0, chunk_zero()), (1, chunk_un())] {
        let nbt = chunk_entites(DV_1_18_2, cx, 0, &occ);
        let ch = balayer_chunk(&nbt).unwrap();
        for e in &ch.entrees {
            let m = Mobile::depuis(&nbt, e, ch.data_version);
            assert_eq!(m.octets(), e.span.slice(&nbt), "{:?}", m.id());
        }
    }
}

// ── copier ──────────────────────────────────────────────────────────────────

#[test]
fn copier_ramasse_les_entites_de_la_selection_en_local() {
    let st = monde();
    let mut i = Interner::new();
    let p = copier(&st, &SURFACE, Folder::Region, &sel(), &mut i).unwrap();
    let pris: BTreeSet<_> = p.mobiles.iter().map(|m| m.uuid().unwrap()).collect();
    assert_eq!(
        pris,
        emportees(),
        "le cadre de façade (porté par un mur de la sélection) vient ; le cadre \
         voisin (porté par un mur d'à côté) reste, même si sa case est dedans"
    );
    let stand = p.mobiles.iter().find(|m| m.uuid() == Some(STAND)).unwrap();
    assert_eq!(
        stand.pos(),
        Some([5.5, 4.0, 5.5]),
        "en LOCAL, au coin de la sélection"
    );
    assert!(st.is_clean(), "`//copy` n'écrit rien");
}

/// Une position illisible n'est NULLE PART. `NaN as i32` vaut 0 : sans garde,
/// l'entité serait prise dans toute sélection qui contient l'origine — et
/// déplacée avec une position qui ne veut rien dire.
#[test]
fn une_entite_sans_position_lisible_n_est_prise_nulle_part() {
    let m = MemorySource::new();
    m.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    let perdue = Occupant::nouveau("minecraft:armor_stand", [f64::NAN, 0.0, 0.0], 0.0, STAND);
    let saine = Occupant::nouveau("minecraft:armor_stand", [0.5, 0.0, 0.5], 0.0, CADRE_MUR);
    m.put_region(
        SURFACE,
        Folder::Entities,
        ZERO,
        region_entites(0, 0, &[(0, 0, DV_1_18_2, vec![perdue, saine])]),
    );
    let st = Staging::new(m, MemorySource::new());
    let mut i = Interner::new();
    let s = BBox::new(BlockPos::new(0, -10, 0), BlockPos::new(15, 10, 15));
    let p = copier(&st, &SURFACE, Folder::Region, &s, &mut i).unwrap();
    let pris: Vec<_> = p.mobiles.iter().map(|m| m.uuid().unwrap()).collect();
    assert_eq!(pris, [CADRE_MUR]);
}

// ── coller ──────────────────────────────────────────────────────────────────

#[test]
fn coller_pose_des_copies_neuves_et_laisse_les_originaux() {
    let st = monde();
    let avant = par_uuid(&entites(&st));
    let cr = lancer(&st, "copier-vers", &copier_vers([32, 0, 0], None), &sel());
    assert_eq!(cr.mobiles_copies, 7);
    assert_eq!(cr.rapport.mobiles_poses, 7);
    assert_eq!(cr.rapport.mobiles_retires, 0);

    let apres = entites(&st);
    let index = par_uuid(&apres);
    // Les originaux n'ont pas bougé d'un octet.
    for (u, v) in &avant {
        assert_eq!(index[u].tag, v.tag, "l'original {u:?} doit rester tel quel");
    }
    // Les copies : sept entités NEUVES, décalées de 32 blocs, au contenu intact.
    let neuves: Vec<&Vue> = apres
        .iter()
        .filter(|v| !avant.contains_key(&v.uuid()))
        .collect();
    assert_eq!(neuves.len(), 7);
    for n in &neuves {
        let u = n.uuid();
        assert_eq!(
            (u[1] >> 12) & 0xF,
            4,
            "un UUID de version 4, comme le jeu en tire"
        );
        let o = avant
            .values()
            .find(|o| {
                let (a, b) = (o.pos(), n.pos());
                o.id() == n.id() && a[0] + 32.0 == b[0] && a[1] == b[1] && a[2] == b[2]
            })
            .unwrap_or_else(|| panic!("aucun original pour la copie {:?}", n.id()));
        assert_eq!(
            sans_situation(&n.tag),
            sans_situation(&o.tag),
            "hors ce qui la situe, une copie porte les MÊMES octets — nom en \
             hangeul, attributs, données de mod"
        );
    }
}

#[test]
fn coller_deux_fois_au_meme_endroit_ne_double_rien() {
    let st = monde();
    lancer(&st, "copier-vers", &copier_vers([32, 0, 0], None), &sel());
    let une = contenu(&st);
    let cr = lancer(&st, "copier-vers", &copier_vers([32, 0, 0], None), &sel());
    assert_eq!(
        correctifs_d_entites(&cr),
        0,
        "les mêmes UUID reviennent, et la pose REMPLACE : rien ne change"
    );
    assert_eq!(contenu(&st), une);
}

#[test]
fn une_laisse_tenue_par_une_entite_copiee_suit_la_copie() {
    let st = monde();
    let s = BBox::new(BlockPos::new(16, 60, 0), BlockPos::new(31, 80, 15));
    let avant = par_uuid(&entites(&st));
    lancer(&st, "copier-vers", &copier_vers([32, 0, 0], None), &s);
    let apres = entites(&st);
    let neuf = |id: &str| {
        apres
            .iter()
            .find(|v| v.id() == id && !avant.contains_key(&v.uuid()))
            .unwrap()
            .clone()
    };
    let (marchand, lama) = (
        neuf("minecraft:wandering_trader"),
        neuf("minecraft:trader_llama"),
    );
    let laisse = match lama.tag.get("Leash").and_then(|l| l.get("UUID")) {
        Some(Tag::IntArray(v)) => [v[0], v[1], v[2], v[3]],
        autre => panic!("laisse illisible : {autre:?}"),
    };
    assert_eq!(
        laisse,
        marchand.uuid(),
        "le lama copié est tenu par le marchand COPIÉ, pas par l'original"
    );
}

// ── déplacer ────────────────────────────────────────────────────────────────

#[test]
fn deplacer_emporte_les_entites_et_garde_leur_uuid() {
    let st = monde();
    let avant = par_uuid(&entites(&st));
    let cr = lancer(&st, "deplacer", &deplacer([32, 0, 0]), &sel());
    assert_eq!(cr.rapport.mobiles_poses, 7);
    assert_eq!(cr.rapport.mobiles_retires, 7);

    let vues = entites(&st);
    let apres = par_uuid(&vues);
    // Sur les VUES et pas sur la table : indexée par UUID, elle avalerait un
    // original resté derrière sa copie — c'est comme ça qu'une mutation « le
    // retrait est ignoré » passait ce test.
    assert_eq!(
        vues.len(),
        avant.len(),
        "aucune entité perdue, aucune doublée"
    );
    assert_eq!(
        apres.len(),
        vues.len(),
        "deux entités ne partagent jamais un UUID"
    );
    for u in emportees() {
        let (a, b) = (avant[&u].pos(), apres[&u].pos());
        assert_eq!([a[0] + 32.0, a[1], a[2]], b, "{:?}", apres[&u].id());
        assert!(apres[&u].chunk.0 >= 2, "partie de son ancien chunk");
    }
    for u in [CADRE_VOISIN, MARCHAND, LAMA, VACHE, CHAUVE_SOURIS] {
        assert_eq!(
            apres[&u].tag, avant[&u].tag,
            "hors sélection, rien ne bouge"
        );
    }
    // Ce qui la situe a suivi : la case du cadre, le lit et la maison du
    // villageois — pas son poste de travail, resté dans l'autre bâtiment, ni
    // un souvenir d'une autre dimension.
    let cadre = &apres[&CADRE_MUR].tag;
    assert_eq!(
        [
            entier(cadre, "TileX"),
            entier(cadre, "TileY"),
            entier(cadre, "TileZ")
        ],
        [40, 65, 3]
    );
    let v = &apres[&VILLAGEOIS].tag;
    assert_eq!(entier(v, "SleepingX"), 35);
    let memoire = |nom: &str| -> [i32; 3] {
        let m = v
            .get("Brain")
            .and_then(|b| b.get("memories"))
            .and_then(|m| m.get(nom))
            .and_then(|m| m.get("value"))
            .and_then(|m| m.get("pos"));
        match m {
            Some(Tag::IntArray(p)) => [p[0], p[1], p[2]],
            autre => panic!("{nom} illisible : {autre:?}"),
        }
    };
    assert_eq!(memoire("minecraft:home"), [35, 64, 11]);
    assert_eq!(memoire("minecraft:job_site"), [200, 64, 200]);
    assert_eq!(memoire("minecraft:meeting_point"), [4, 64, 4]);
}

#[test]
fn deplacer_de_rien_ne_touche_aucun_chunk_d_entites() {
    let st = monde();
    let avant = contenu(&st);
    let cr = lancer(&st, "deplacer", &deplacer([0, 0, 0]), &sel());
    assert_eq!(cr.rapport.mobiles_poses, 7);
    assert_eq!(correctifs_d_entites(&cr), 0, "même place, mêmes octets");
    assert_eq!(contenu(&st), avant);
}

#[test]
fn une_entite_qui_reste_dans_son_chunk_garde_sa_place_dans_la_liste() {
    let st = monde();
    let ids = |v: &[Vue]| -> Vec<[i32; 4]> {
        v.iter()
            .filter(|e| e.chunk == (0, 0))
            .map(Vue::uuid)
            .collect()
    };
    let avant = ids(&entites(&st));
    lancer(&st, "deplacer", &deplacer([1, 0, 0]), &sel());
    assert_eq!(
        ids(&entites(&st)),
        avant,
        "déplacées d'un bloc sans changer de chunk : même ordre, donc un \
         correctif resserré sur les seules coordonnées"
    );
}

#[test]
fn sans_terrain_a_l_arrivee_une_entite_reste_a_sa_place() {
    let st = monde();
    let avant = par_uuid(&entites(&st));
    // La fixture de terrain ne va que jusqu'au chunk 15 : +320 tombe au-delà.
    let cr = lancer(&st, "deplacer", &deplacer([320, 0, 0]), &sel());
    assert_eq!(cr.rapport.mobiles_sans_terrain, 7);
    assert_eq!(cr.rapport.mobiles_poses, 0);
    assert_eq!(
        cr.rapport.mobiles_retires, 0,
        "rien n'est retiré sans arriver"
    );
    assert_eq!(
        par_uuid(&entites(&st))
            .into_iter()
            .map(|(u, v)| (u, v.tag))
            .collect::<Vec<_>>(),
        avant
            .into_iter()
            .map(|(u, v)| (u, v.tag))
            .collect::<Vec<_>>()
    );
}

#[test]
fn une_entite_n_est_pas_melee_a_un_chunk_d_une_autre_version() {
    let st = monde();
    let avant = par_uuid(&entites(&st));
    // +80 : six entités tombent dans le chunk 5, écrit par 1.20.1 ; le cadre
    // de façade tombe dans le chunk 6, qui n'existe pas et naît en 1.18.2.
    let cr = lancer(&st, "deplacer", &deplacer([80, 0, 0]), &sel());
    assert_eq!(cr.rapport.mobiles_autre_version, 6);
    assert_eq!(cr.rapport.mobiles_poses, 1);
    let apres = par_uuid(&entites(&st));
    for u in emportees() {
        if u == CADRE_FACADE {
            assert_eq!(apres[&u].chunk, (6, 0));
        } else {
            assert_eq!(apres[&u].tag, avant[&u].tag, "restée à sa place");
        }
    }
    let neuf = contenu(&st)[&(6, 0)].clone();
    assert_eq!(entier(&neuf, "DataVersion"), DV_1_18_2);
}

// ── annuler, refaire ────────────────────────────────────────────────────────

#[test]
fn annuler_rend_les_chunks_d_entites_d_origine_et_efface_ceux_qu_on_a_crees() {
    let st = monde();
    let avant = contenu(&st);
    let cr = lancer(&st, "deplacer", &deplacer([32, 0, 0]), &sel());
    let apres = contenu(&st);
    assert!(
        apres.contains_key(&(2, 0)) && !avant.contains_key(&(2, 0)),
        "le test ne prouve rien sans chunk CRÉÉ"
    );

    let mut journal = Journal::new();
    assert!(cr
        .rapport
        .journaliser(&mut journal, "Déplacer", "deplacer", Vec::new(), 0));
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert_eq!(
        contenu(&st),
        avant,
        "annuler rend EXACTEMENT les chunks d'avant — et un chunk créé disparaît, \
         il ne reste pas une coquille vide"
    );
    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert_eq!(contenu(&st), apres);
}

// ── empiler ─────────────────────────────────────────────────────────────────

#[test]
fn chaque_copie_d_un_empilement_a_ses_propres_uuid() {
    let st = monde();
    let n = entites(&st).len();
    let mut p = Params::new();
    p.poser("fois", Valeur::Entier(2));
    p.poser(
        "direction",
        Valeur::Direction(tf_world::selection::Direction::PlusX),
    );
    let cr = lancer(&st, "empiler", &p, &sel());
    assert_eq!(cr.rapport.mobiles_poses, 14);
    let tous = entites(&st);
    let uuids: BTreeSet<_> = tous.iter().map(Vue::uuid).collect();
    assert_eq!(tous.len(), n + 14);
    assert_eq!(
        uuids.len(),
        tous.len(),
        "deux entités ne partagent jamais un UUID"
    );
}

// ── tourner, refléter ───────────────────────────────────────────────────────

#[test]
fn tourner_un_extrait_tourne_ses_entites() {
    let st = monde();
    let avant = par_uuid(&entites(&st));
    let cr = lancer(
        &st,
        "copier-vers",
        &copier_vers([32, 0, 0], Some(Transfo::Rot90)),
        &sel(),
    );
    assert_eq!(cr.rapport.mobiles_poses, 7);
    let apres = entites(&st);
    let copie = |id: &str| {
        apres
            .iter()
            .filter(|v| v.id() == id && !avant.contains_key(&v.uuid()))
            .map(|v| v.tag.clone())
            .collect::<Vec<_>>()
    };
    // La boîte fait 16 de large : un quart de tour envoie (x, z) sur
    // (16 − z, x) en LOCAL, puis le coin la pose à (32, 60, 0).
    let stand = &copie("minecraft:armor_stand")[0];
    assert_eq!(doubles(stand.get("Pos").unwrap()), [42.5, 64.0, 5.5]);
    assert_eq!(lacet(stand), 120.0);

    let cadres = copie("minecraft:item_frame");
    let portant = |objet: &str| {
        cadres
            .iter()
            .find(|c| {
                c.get("Item")
                    .and_then(|i| i.get("id"))
                    .and_then(Tag::as_str)
                    == Some(objet)
            })
            .unwrap()
            .clone()
    };
    let mur = portant("minecraft:filled_map");
    assert_eq!(
        [
            entier(&mur, "TileX"),
            entier(&mur, "TileY"),
            entier(&mur, "TileZ")
        ],
        [44, 65, 8]
    );
    assert_eq!(
        octet(&mur, "Facing"),
        4,
        "accroché au sud, il l'est à l'ouest"
    );
    assert_eq!(
        octet(&mur, "ItemRotation"),
        3,
        "au mur, la carte tourne AVEC le cadre"
    );
    let sol = portant("minecraft:diamond");
    assert_eq!(octet(&sol, "Facing"), 1);
    assert_eq!(
        octet(&sol, "ItemRotation"),
        3,
        "au sol, l'objet tourne d'un quart : +2 crans"
    );

    let tableau = &copie("minecraft:painting")[0];
    assert_eq!(octet(tableau, "Facing"), 1, "sud → ouest");
    assert_eq!(
        [
            entier(tableau, "TileX"),
            entier(tableau, "TileY"),
            entier(tableau, "TileZ")
        ],
        [43, 66, 12]
    );

    // La vitesse tourne comme un vecteur : (0,125, 0,25) → (−0,25, 0,125).
    let cochon = &copie("minecraft:pig")[0];
    assert_eq!(doubles(cochon.get("Motion").unwrap()), [-0.25, 0.0, 0.125]);
    // Le lit du villageois est dans la boîte : il tourne avec elle.
    let v = &copie("minecraft:villager")[0];
    assert_eq!(
        [
            entier(v, "SleepingX"),
            entier(v, "SleepingY"),
            entier(v, "SleepingZ")
        ],
        [36, 64, 3]
    );
}

#[test]
fn un_shulker_s_accroche_a_la_face_tournee() {
    let occ = Occupant::nouveau("minecraft:shulker", [2.5, 64.0, 2.5], 0.0, STAND)
        .avec(Trait::Attache(5));
    let nbt = chunk_entites(DV_1_18_2, 0, 0, &[occ]);
    let ch = balayer_chunk(&nbt).unwrap();
    let m = Mobile::depuis(&nbt, &ch.entrees[0], ch.data_version);
    let mut a = Vec::new();
    for t in TOUTES {
        let n = transformer_mobile(&m, t, [16, 21, 16], &mut a);
        assert_eq!(n.corps[0].attache.unwrap().v, face3_apres(t, 5).unwrap());
    }
    // Accroché à l'EST, un quart de tour l'accroche au SUD.
    let n = transformer_mobile(&m, Transfo::Rot90, [16, 21, 16], &mut a);
    assert_eq!(n.corps[0].attache.unwrap().v, 3);
    assert!(a.is_empty());
}

/// **Annuler et refaire traversent un chunk DÉPORTÉ.** Au-delà d'un mégaoctet,
/// le jeu range la charge d'un chunk dans un `c.X.Z.mcc` et ne laisse qu'un
/// talon dans la région — une ferme à objets y suffit. Sans résoudre ce
/// talon, l'annulation lisait une charge vide et refusait de se faire.
#[test]
fn annuler_et_refaire_traversent_un_chunk_deporte() {
    use tf_anvil::region::{write, Compression, RawChunk, Region};
    let m = MemorySource::new();
    m.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    let lourd = chunk_entites(
        DV_1_18_2,
        0,
        0,
        &[
            Occupant::nouveau("minecraft:armor_stand", [5.5, 64.0, 5.5], 0.0, STAND),
            Occupant::nouveau("minecraft:item", [10.5, 64.0, 10.5], 0.0, VACHE)
                .avec(Trait::Lest(1_300_000)),
        ],
    );
    let mut r = Region::vide(0, 0);
    r.slots[0] = Some(RawChunk {
        index: 0,
        timestamp: 0,
        compression: Compression::Zlib,
        payload: std::borrow::Cow::Owned(tf_anvil::deflate(&lourd, Compression::Zlib).unwrap()),
        external: false,
    });
    let out = write(&r).unwrap();
    assert_eq!(
        out.external.len(),
        1,
        "le test ne prouve rien sans chunk déporté"
    );
    m.put_region(SURFACE, Folder::Entities, ZERO, out.region);
    for f in out.external {
        m.put_external(SURFACE, Folder::Entities, &f.name, f.bytes);
    }
    let st = Staging::new(m, MemorySource::new());

    // Le contenu, talons résolus.
    let lire = |st: &Staging<MemorySource, MemorySource>| -> BTreeMap<u16, Vec<u8>> {
        let octets = st.read_region(&SURFACE, Folder::Entities, ZERO).unwrap();
        let mut region = tf_anvil::read(&octets, 0, 0).unwrap();
        let mut out = BTreeMap::new();
        for i in 0..1024u16 {
            let Some(c) = region.slots[i as usize].as_mut() else {
                continue;
            };
            if c.needs_external() {
                let nom = tf_anvil::external_file_name(i as i32 % 32, i as i32 / 32);
                c.resolve_external(st.read_external(&SURFACE, Folder::Entities, &nom).unwrap());
            }
            out.insert(i, tf_anvil::inflate(&c.payload, c.compression).unwrap());
        }
        out
    };
    let avant = lire(&st);
    let s = BBox::new(BlockPos::new(5, 60, 5), BlockPos::new(5, 70, 5));
    let cr = lancer(&st, "deplacer", &deplacer([32, 0, 0]), &s);
    assert_eq!(cr.rapport.mobiles_poses, 1);
    let apres = lire(&st);
    assert_ne!(apres, avant);

    let mut journal = Journal::new();
    assert!(cr
        .rapport
        .journaliser(&mut journal, "Déplacer", "deplacer", Vec::new(), 0));
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert_eq!(lire(&st), avant);
    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert_eq!(lire(&st), apres);
}

#[test]
fn un_miroir_recale_un_tableau_de_largeur_paire() {
    let st = monde();
    let avant = par_uuid(&entites(&st));
    lancer(
        &st,
        "copier-vers",
        &copier_vers([32, 0, 0], Some(Transfo::MiroirX)),
        &sel(),
    );
    let tableau = entites(&st)
        .into_iter()
        .find(|v| v.id() == "minecraft:painting" && !avant.contains_key(&v.uuid()))
        .unwrap()
        .tag;
    // Il couvrait x = 12 et 13 (sa gauche est l'est). Reflété dans une boîte
    // de 16 : x = 3 et 2. Face au sud, sa gauche est toujours l'est, donc son
    // ancre est la case 2 — pas 3, où l'aurait posé la seule formule des cases.
    assert_eq!(octet(&tableau, "Facing"), 0);
    assert_eq!(entier(&tableau, "TileX"), 32 + 2);
}

/// **Quatre quarts de tour ramènent chaque entité à ses OCTETS** — `−0,0`
/// compris, qui n'est pas `0,0` pour qui compare des octets.
#[test]
fn quatre_quarts_de_tour_ramenent_une_entite_a_ses_octets() {
    let nbt = chunk_entites(DV_1_18_2, 0, 0, &chunk_zero());
    let ch = balayer_chunk(&nbt).unwrap();
    for e in &ch.entrees {
        let m = Mobile::depuis(&nbt, e, ch.data_version);
        for (t, fois) in [
            (Transfo::Rot90, 4),
            (Transfo::MiroirX, 2),
            (Transfo::MiroirZ, 2),
        ] {
            let mut n = m.clone();
            let mut approches = Vec::new();
            for _ in 0..fois {
                n = transformer_mobile(&n, t, [16, 21, 16], &mut approches);
            }
            assert_eq!(n.octets(), m.octets(), "{:?} sous {t:?}", m.id());
        }
    }
}

#[test]
fn ce_qu_un_miroir_ne_sait_pas_refleter_est_nomme() {
    let nbt = chunk_entites(DV_1_18_2, 0, 0, &chunk_zero());
    let ch = balayer_chunk(&nbt).unwrap();
    let stand = Mobile::depuis(&nbt, &ch.entrees[0], ch.data_version);
    let mut a = Vec::new();
    transformer_mobile(&stand, Transfo::Rot90, [16, 21, 16], &mut a);
    assert!(a.is_empty(), "une rotation emporte la pose avec le lacet");
    transformer_mobile(&stand, Transfo::MiroirX, [16, 21, 16], &mut a);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].id, "minecraft:armor_stand");
}

// ── //hollow ────────────────────────────────────────────────────────────────

#[test]
fn creuser_ne_duplique_aucune_entite() {
    let st = monde();
    let avant = contenu(&st);
    let s = BBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 70, 15));
    let cr = lancer(&st, "creuser", &Params::new(), &s);
    assert_eq!(correctifs_d_entites(&cr), 0);
    assert_eq!(contenu(&st), avant);
}

// ── les règles, pures ───────────────────────────────────────────────────────

const TOUTES: [Transfo; 5] = [
    Transfo::Rot90,
    Transfo::Rot180,
    Transfo::Rot270,
    Transfo::MiroirX,
    Transfo::MiroirZ,
];

/// Le centre de chaque CASE va au centre de la case transformée : la formule
/// continue et celle des blocs sont la même, prise sur les bords.
#[test]
fn une_position_suit_la_formule_des_cases() {
    let taille = [3, 2, 5];
    for t in TOUTES {
        for x in 0..3u32 {
            for z in 0..5u32 {
                let (ax, az) = t.case_apres((x, z), (3, 5));
                let p = position_apres(t, [x as f64 + 0.5, 1.25, z as f64 + 0.5], taille);
                assert_eq!(
                    p,
                    [ax as f64 + 0.5, 1.25, az as f64 + 0.5],
                    "{t:?} ({x}, {z})"
                );
            }
        }
        // Et un vecteur est la différence de deux points.
        let (a, v) = ([1.0, 0.0, 2.0], [0.25, -1.0, 0.75]);
        let b = [a[0] + v[0], a[1] + v[1], a[2] + v[2]];
        let (pa, pb) = (position_apres(t, a, taille), position_apres(t, b, taille));
        assert_eq!(
            vecteur_apres(t, v),
            [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]]
        );
    }
}

/// Le lacet se vérifie par la DIRECTION qu'il désigne — `(−sin θ, cos θ)`,
/// lacet 0 vers le sud — transformée comme un vecteur.
#[test]
fn un_lacet_suit_la_direction_qu_il_designe() {
    let dir = |l: f32| {
        let r = (l as f64).to_radians();
        [-r.sin(), 0.0, r.cos()]
    };
    for t in TOUTES {
        for l in [
            -180.0f32, -135.0, -90.0, -30.0, 0.0, 12.5, 45.0, 90.0, 179.0,
        ] {
            let voulu = vecteur_apres(t, dir(l));
            let n = lacet_apres(t, l);
            assert!((-180.0..180.0).contains(&n), "{n} hors de [−180, 180)");
            let eu = dir(n);
            for k in 0..3 {
                assert!((eu[k] - voulu[k]).abs() < 1e-5, "{t:?} {l} → {n}");
            }
        }
    }
}

/// **Un cadre reste accroché au bloc qui le porte.** Le bloc porteur suit la
/// formule des CASES ; le cadre, celle des faces : les deux doivent se
/// retrouver, pour chaque face et chaque transformation.
#[test]
fn un_cadre_reste_accroche_au_bloc_qui_le_porte() {
    let taille = [7, 4, 9];
    for t in TOUTES {
        for f in 0..6i8 {
            let tuile = [3, 2, 4];
            let pas = PAS_3D[f as usize];
            let porteur = [tuile[0] - pas[0], tuile[1] - pas[1], tuile[2] - pas[2]];
            let f2 = face3_apres(t, f).unwrap();
            let t2 = t.point_apres(tuile, taille);
            let p2 = PAS_3D[f2 as usize];
            assert_eq!(
                t.point_apres(porteur, taille),
                [t2[0] - p2[0], t2[1] - p2[1], t2[2] - p2[2]],
                "{t:?}, face {f}"
            );
        }
        assert_eq!(face3_apres(t, 6), None);
        assert_eq!(face3_apres(t, -1), None);
    }
}

/// Les cases qu'un tableau COUVRE, calculées comme le jeu les calcule
/// (`HangingEntity.recalculateBoundingBox`) : centre de la case d'ancre,
/// décalé d'un demi-bloc vers sa gauche si la largeur est paire, et vers le
/// haut si la hauteur l'est.
fn couvertes(tuile: [i32; 3], f: i8, l: i32, h: i32) -> BTreeSet<[i32; 3]> {
    let gauche = PAS_2D[((f as usize) + 3) % 4];
    let dl = if l % 2 == 0 { 0.5 } else { 0.0 };
    let dh = if h % 2 == 0 { 0.5 } else { 0.0 };
    let centre = [
        tuile[0] as f64 + 0.5 + dl * gauche[0] as f64,
        tuile[1] as f64 + 0.5 + dh,
        tuile[2] as f64 + 0.5 + dl * gauche[2] as f64,
    ];
    let mut out = BTreeSet::new();
    for i in 0..l {
        for j in 0..h {
            let a = i as f64 - (l - 1) as f64 / 2.0;
            let b = j as f64 - (h - 1) as f64 / 2.0;
            out.insert([
                (centre[0] + a * gauche[0] as f64).floor() as i32,
                (centre[1] + b).floor() as i32,
                (centre[2] + a * gauche[2] as f64).floor() as i32,
            ]);
        }
    }
    out
}

/// **Un tableau couvre les MÊMES cases après transformation** — pour chaque
/// largeur (seule la parité décide), chaque face, chaque transformation.
/// Vérifié sur l'entité entière (`transformer_mobile`), pas sur une formule
/// isolée : c'est le décalage de miroir qu'on veut voir.
#[test]
fn un_tableau_couvre_les_memes_cases_apres_transformation() {
    let taille = [16, 8, 16];
    for motif in [
        "minecraft:kebab",
        "minecraft:pool",
        "minecraft:bouquet",
        "minecraft:fighters",
    ] {
        let (l, h) = taille_motif(motif);
        for f in 0..4i8 {
            let tuile = [7, 3, 6];
            let nbt = chunk_entites(
                DV_1_18_2,
                0,
                0,
                &[Occupant::tableau(tuile, f, motif, TABLEAU)],
            );
            let ch = balayer_chunk(&nbt).unwrap();
            let m = Mobile::depuis(&nbt, &ch.entrees[0], ch.data_version);
            for t in TOUTES {
                let mut a = Vec::new();
                let n = transformer_mobile(&m, t, taille, &mut a);
                assert!(a.is_empty(), "{motif} est connu");
                let k = &n.corps[0];
                let voulues: BTreeSet<_> = couvertes(tuile, f, l, h)
                    .into_iter()
                    .map(|c| t.point_apres(c, taille))
                    .collect();
                assert_eq!(
                    couvertes(k.tuile.as_ref().unwrap().v, k.facing.unwrap().v, l, h),
                    voulues,
                    "{motif} face {f} sous {t:?}"
                );
                assert_eq!(k.facing.unwrap().v, face2_apres(t, f).unwrap());
            }
        }
    }
}

// La rotation d'un objet dans son cadre, vérifiée contre le DESSIN du jeu.
//
// `ItemFrameRenderer` compose : tangage autour de X, puis `180 − lacet`
// autour de Y, puis l'angle de l'objet autour de Z. On recalcule ici ces
// matrices telles quelles — un chemin de calcul qui ne partage rien avec
// les règles fermées de `rotation_objet_apres` — et on exige qu'une
// rotation du monde appliquée au cadre DESSINÉ donne le cadre dessiné à
// partir de ce que la règle écrit. Pour un miroir, l'image elle-même est
// reflétée, ce qu'aucun angle ne rend : on exige alors que le HAUT de
// l'objet aille au bon endroit.

type M3 = [[f64; 3]; 3];

fn mul(a: M3, b: M3) -> M3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    c
}

fn rx(d: f64) -> M3 {
    let (s, c) = d.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

fn ry(d: f64) -> M3 {
    let (s, c) = d.to_radians().sin_cos();
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

fn rz(d: f64) -> M3 {
    let (s, c) = d.to_radians().sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// Le cadre DESSINÉ : `ItemFrame.setDirection` pour les angles, puis la
/// composition du rendu.
fn dessin(face: i8, r: i8, carte: bool) -> M3 {
    let (tangage, lacet) = match face {
        0 => (90.0, 0.0),
        1 => (-90.0, 0.0),
        // Valeur 2D × 90 : nord 2, sud 0, ouest 1, est 3.
        2 => (0.0, 180.0),
        3 => (0.0, 0.0),
        4 => (0.0, 90.0),
        _ => (0.0, 270.0),
    };
    let angle = if carte {
        (r as i32 % 4) as f64 * 90.0
    } else {
        r as f64 * 45.0
    };
    mul(mul(rx(tangage), ry(180.0 - lacet)), rz(angle))
}

/// La transformation du MONDE, en matrice : un quart de tour envoie +X sur
/// +Z, c'est-à-dire −90° autour de +Y dans un repère direct.
fn monde_de(t: Transfo) -> M3 {
    match t {
        Transfo::Rot90 => ry(-90.0),
        Transfo::Rot180 => ry(180.0),
        Transfo::Rot270 => ry(90.0),
        Transfo::MiroirX => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        Transfo::MiroirZ => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
    }
}

fn proches(a: M3, b: M3) -> bool {
    (0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() < 1e-9))
}

#[test]
fn la_rotation_d_un_objet_suit_le_dessin_du_jeu() {
    for carte in [false, true] {
        for face in 0..6i8 {
            for r in 0..8i8 {
                for t in TOUTES {
                    // Le monde tourne AVEC la matrice de base : on vérifie
                    // d'abord que la table des faces est cohérente avec lui.
                    let v = monde_de(t);
                    let f2 = face3_apres(t, face).unwrap();
                    let r2 = rotation_objet_apres(t, r, face, carte);
                    assert!((0..8).contains(&r2));
                    let avant = mul(v, dessin(face, r, carte));
                    let apres = dessin(f2, r2, carte);
                    if t.est_miroir() {
                        // Le haut de l'objet : la deuxième colonne.
                        let haut = |m: M3| [m[0][1], m[1][1], m[2][1]];
                        let (a, b) = (haut(avant), haut(apres));
                        assert!(
                            (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9),
                            "{t:?} face {face} r {r} carte {carte} : haut {a:?} ≠ {b:?}"
                        );
                    } else {
                        assert!(
                            proches(avant, apres),
                            "{t:?} face {face} r {r} carte {carte}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn un_uuid_derive_est_de_version_4_et_depend_de_la_position() {
    let a = uuid_derive(STAND, [1.5, 64.0, 2.5]);
    assert_eq!((a[1] >> 12) & 0xF, 4, "version 4");
    assert_eq!((a[2] as u32) >> 30, 0b10, "variante IETF");
    assert_eq!(a, uuid_derive(STAND, [1.5, 64.0, 2.5]), "rejouable");
    assert_ne!(
        a,
        uuid_derive(STAND, [1.5, 64.0, 3.5]),
        "une autre place, une autre entité"
    );
    assert_ne!(a, uuid_derive(CADRE_MUR, [1.5, 64.0, 2.5]));
    assert_ne!(a, STAND);
}
