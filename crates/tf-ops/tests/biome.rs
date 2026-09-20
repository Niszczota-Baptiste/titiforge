//! `//setbiome` — la SECONDE palette, et sa grille de 4 × 4 × 4.
//!
//! Le piège central n'est pas le format, c'est l'unité : un biome se pose sur
//! une cellule de **quatre blocs par axe**, pas sur un bloc. Une sélection qui
//! ne tombe pas sur un multiple de quatre ne peut donc pas être respectée au
//! bloc près — c'est une propriété d'Anvil, et la taire ferait passer un
//! débordement de trois blocs pour un bug.

use tf_anvil::{decode_biomes, inflate, scan, Interner};
use tf_bench::{region, Terrain};
use tf_ops::edition::appliquer;
use tf_ops::PoserBiome;
use tf_world::coords::{BBox, BlockPos, RegionPos, SectionPos};
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn staging() -> Staging<MemorySource, MemorySource> {
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, region(&Terrain::avec_biomes()));
    Staging::new(m, MemorySource::new())
}

fn boite(a: (i32, i32, i32), b: (i32, i32, i32)) -> BBox {
    BBox::new(
        BlockPos {
            x: a.0,
            y: a.1,
            z: a.2,
        },
        BlockPos {
            x: b.0,
            y: b.1,
            z: b.2,
        },
    )
}

/// Le biome d'une CELLULE, relu à travers le staging.
fn biome_de<S: RegionSource, O: tf_world::staging::RegionStore>(
    st: &Staging<S, O>,
    i: &mut Interner,
    monde: BlockPos,
) -> Option<String> {
    let octets = st.read_region(&SURFACE, DOSSIER, ZERO).ok()?;
    let mut r = tf_anvil::read(&octets, 0, 0).ok()?;
    let c = r.get_mut(monde.x.div_euclid(16), monde.z.div_euclid(16))?;
    let brut = inflate(&c.payload, c.compression).ok()?;
    let s = scan(&brut).ok()?;
    let sy = monde.y.div_euclid(16) as i8;
    let sc = s.sections.iter().find(|sc| sc.y == sy)?;
    let b = decode_biomes(&brut, sc, i).ok()??;
    let (cx, cy, cz) = tf_anvil::Biomes::cellule_de_bloc(
        monde.x.rem_euclid(16) as usize,
        monde.y.rem_euclid(16) as usize,
        monde.z.rem_euclid(16) as usize,
    );
    b.get(cx, cy, cz)
        .and_then(|id| i.resolve(id).map(str::to_string))
}

#[test]
fn poser_un_biome_sur_une_section_entiere_la_rend_monobiome() {
    let st = staging();
    let mut i = Interner::new();
    let desert = i.intern("minecraft:desert");

    // Une section entière : le chemin O(1), celui qui fait disparaître le
    // tableau d'indices.
    let sel = boite((0, -16, 0), (15, -1, 15));
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(desert),
        &i,
    )
    .unwrap();
    assert_eq!(r.biomes, 1, "une section de biomes changée");
    assert!(!r.patches.is_empty());

    let octets = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let mut reg = tf_anvil::read(&octets, 0, 0).unwrap();
    let c = reg.get_mut(0, 0).unwrap();
    let brut = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&brut).unwrap();
    let sc = s.sections.iter().find(|sc| sc.y == -1).unwrap();
    let b = decode_biomes(&brut, sc, &mut i).unwrap().unwrap();
    assert_eq!(b.palette.len(), 1, "une seule entrée");
    assert!(b.data.is_empty(), "et donc AUCUN tableau d'indices");
    assert_eq!(i.resolve(b.palette[0]), Some("minecraft:desert"));
}

#[test]
fn poser_deux_fois_le_meme_biome_ne_reecrit_rien() {
    let st = staging();
    let mut i = Interner::new();
    let desert = i.intern("minecraft:desert");
    let sel = boite((0, -16, 0), (15, -1, 15));
    let op = PoserBiome::nouveau(desert);

    appliquer(&st, &SURFACE, DOSSIER, &sel, &op, &i).unwrap();
    let encore = appliquer(&st, &SURFACE, DOSSIER, &sel, &op, &i).unwrap();
    assert_eq!(encore.biomes, 0);
    assert!(
        encore.patches.is_empty(),
        "{} correctif(s) pour zéro changement",
        encore.patches.len()
    );
}

/// L'idempotence sur le chemin PARTIEL — celui qui écrit cellule par cellule.
///
/// Le test précédent ne le couvre pas : une sélection qui couvre toute la
/// section prend le chemin O(1) (`set_uniforme`), qui ne consulte jamais les
/// indices. C'est ici que vit l'invariant n° 4 pour cette seconde palette :
/// comparer l'ÉTAT de la cellule, et pas son indice.
#[test]
fn reposer_le_meme_biome_sur_une_selection_partielle_ne_reecrit_rien() {
    let st = staging();
    let mut i = Interner::new();
    let ocean = i.intern("minecraft:ocean");
    let op = PoserBiome::nouveau(ocean);
    // Un quart de section : le chemin cellule par cellule.
    let sel = boite((0, -16, 0), (3, -1, 15));

    let un = appliquer(&st, &SURFACE, DOSSIER, &sel, &op, &i).unwrap();
    assert_eq!(un.biomes, 1, "la première passe doit écrire");
    let deux = appliquer(&st, &SURFACE, DOSSIER, &sel, &op, &i).unwrap();
    assert_eq!(deux.biomes, 0, "la seconde ne doit rien trouver à faire");
    assert!(
        deux.patches.is_empty(),
        "{} correctif(s) pour zéro changement",
        deux.patches.len()
    );
}

/// **La grille du biome est de quatre blocs.** Une sélection d'un seul bloc
/// en peint quatre par axe, et c'est le format qui le veut. Le taire ferait
/// passer le débordement pour un bug.
#[test]
fn un_seul_bloc_selectionne_peint_sa_cellule_entiere() {
    let st = staging();
    let mut i = Interner::new();
    let ocean = i.intern("minecraft:ocean");

    let sel = BBox::single(BlockPos { x: 5, y: -12, z: 6 });
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(ocean),
        &i,
    )
    .unwrap();
    assert_eq!(r.biomes, 1);

    // Les 4 × 4 × 4 blocs de la cellule (1, 1, 1) de la section −1.
    for y in -12..=-9 {
        for z in 4..=7 {
            for x in 4..=7 {
                assert_eq!(
                    biome_de(&st, &mut i, BlockPos { x, y, z }).as_deref(),
                    Some("minecraft:ocean"),
                    "le bloc ({x},{y},{z}) est dans la même cellule"
                );
            }
        }
    }
    // Et la cellule d'à côté n'a pas bougé.
    assert_ne!(
        biome_de(&st, &mut i, BlockPos { x: 8, y: -12, z: 6 }).as_deref(),
        Some("minecraft:ocean"),
        "la cellule voisine ne doit pas déborder"
    );
}

#[test]
fn une_selection_partielle_ne_touche_que_ses_cellules() {
    let st = staging();
    let mut i = Interner::new();
    let marais = i.intern("minecraft:swamp");

    // Un quart de section en X, toute la hauteur et la profondeur.
    let sel = boite((0, -16, 0), (3, -1, 15));
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(marais),
        &i,
    )
    .unwrap();
    assert_eq!(r.biomes, 1);

    for z in [0, 8, 15] {
        assert_eq!(
            biome_de(&st, &mut i, BlockPos { x: 1, y: -8, z }).as_deref(),
            Some("minecraft:swamp"),
            "dans la sélection"
        );
        assert_ne!(
            biome_de(&st, &mut i, BlockPos { x: 9, y: -8, z }).as_deref(),
            Some("minecraft:swamp"),
            "hors de la sélection, à deux cellules de distance"
        );
    }
}

#[test]
fn poser_un_biome_ne_touche_pas_aux_blocs() {
    // Le rapport le dit, et la relecture le prouve : `//setbiome` n'est pas
    // une opération de blocs, et ses sections ne doivent pas être réencodées
    // pour rien.
    let st = staging();
    let mut i = Interner::new();
    let ocean = i.intern("minecraft:ocean");
    let sel = boite((0, -16, 0), (15, -1, 15));

    let avant = tf_ops::edition::copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(ocean),
        &i,
    )
    .unwrap();
    assert_eq!(r.blocs, None, "aucun bloc compté : il n'y en a pas");
    // La section traversée compte comme `Rien` — c'est exact, elle n'a rien
    // eu à faire. Ce qui compte, c'est qu'AUCUNE n'ait pris un étage qui
    // écrit : section, palette ou bloc.
    assert_eq!(
        &r.etages[1..],
        &[0, 0, 0],
        "aucune section n'est passée par un étage qui ÉCRIT des blocs : {:?}",
        r.etages
    );
    assert!(r.etages[0] > 0, "et la section a bien été traversée");
    let apres = tf_ops::edition::copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert_eq!(apres.blocs, avant.blocs, "les blocs sont intacts");
}

#[test]
fn une_section_hors_selection_garde_ses_biomes() {
    let st = staging();
    let mut i = Interner::new();
    let ocean = i.intern("minecraft:ocean");

    let temoin = BlockPos { x: 5, y: -40, z: 5 };
    let avant = biome_de(&st, &mut i, temoin);
    assert!(avant.is_some(), "le témoin doit porter un biome");

    let sel = boite((0, -16, 0), (15, -1, 15));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(ocean),
        &i,
    )
    .unwrap();
    assert_eq!(biome_de(&st, &mut i, temoin), avant);
}

#[test]
fn la_cellule_d_un_bloc_negatif_est_la_bonne() {
    // Division PLANCHER, encore : `-1 >> 2` vaut −1, mais `-1 / 4` vaut 0.
    // Les coordonnées LOCALES sont toujours positives ; c'est la conversion
    // monde → section qui doit être juste, et ce test la traverse.
    let st = staging();
    let mut i = Interner::new();
    let neige = i.intern("minecraft:snowy_taiga");
    let sel = BBox::single(BlockPos { x: 2, y: -33, z: 2 });
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(neige),
        &i,
    )
    .unwrap();
    assert_eq!(
        biome_de(&st, &mut i, BlockPos { x: 2, y: -33, z: 2 }).as_deref(),
        Some("minecraft:snowy_taiga")
    );
    // Le bloc juste au-dessus est dans une AUTRE cellule (−32 → cellule 0 de
    // la section −2 … non : −33 est en section −3, cellule y = 3 ; −32 est en
    // section −2, cellule 0).
    assert_ne!(
        biome_de(&st, &mut i, BlockPos { x: 2, y: -32, z: 2 }).as_deref(),
        Some("minecraft:snowy_taiga")
    );
}

#[test]
fn une_section_sans_biome_lisible_est_laissee_telle_quelle() {
    // La fixture par défaut n'écrit PAS de biomes : `decode_biomes` rend
    // `None`, et l'opération ne doit rien écrire plutôt que d'en inventer.
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, region(&Terrain::petite()));
    let st = Staging::new(m, MemorySource::new());
    let mut i = Interner::new();
    let ocean = i.intern("minecraft:ocean");
    let sel = boite((0, -16, 0), (15, -1, 15));
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &PoserBiome::nouveau(ocean),
        &i,
    )
    .unwrap();
    assert_eq!(r.biomes, 0);
    assert!(r.patches.is_empty(), "rien à écrire, donc rien d'écrit");
}

#[test]
fn la_section_visee_est_bien_celle_qu_on_croit() {
    // Un garde-fou sur le test lui-même : sans lui, tout ce fichier pourrait
    // viser la mauvaise section et passer en ne vérifiant rien.
    assert_eq!(SectionPos::new(0, -1, 0).min_block().y, -16);
    assert_eq!((-12i32).div_euclid(16), -1);
    assert_eq!((-12i32).rem_euclid(16), 4);
}
