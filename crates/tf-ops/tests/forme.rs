//! Les formes — et surtout le verdict qu'elles rendent PAR SECTION.
//!
//! Une forme a deux chemins : `contient`, qui répond case par case, et
//! `couverture`, qui répond pour les 4 096 cases d'une section d'un seul
//! coup. Le second existe pour éviter le premier, et c'est précisément pour
//! ça qu'il est dangereux : un « Dedans » faux écrit hors de la forme, un
//! « Dehors » faux y laisse un trou, et ni l'un ni l'autre ne se voit sur une
//! capture d'écran.
//!
//! Le test qui décide compare donc le verdict rapide au parcours LENT, case
//! par case, sur toutes les sections de la boîte englobante — l'invariant du
//! dépôt : *toute stratégie rapide se compare au résultat de la stratégie
//! lente, et le compte doit être exact au bloc près.*

use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, copier};
use tf_ops::forme::Couverture;
use tf_ops::plan::Plan;
use tf_ops::{Forme, Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos, SectionPos};
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn formes() -> Vec<(&'static str, Forme)> {
    vec![
        ("sphère 12", Forme::sphere([40, -30, 40], 12.0)),
        ("sphère 1", Forme::sphere([8, -50, 8], 1.0)),
        ("sphère 0", Forme::sphere([8, -50, 8], 0.0)),
        (
            "ellipsoïde plat",
            Forme::ellipsoide([40, -30, 40], [20.0, 3.0, 9.0]),
        ),
        ("cylindre", Forme::cylindre([40, -30, 40], 10.0, 24.0)),
        (
            "pyramide",
            Forme::pyramide([40, -50, 40], 12.0, 13.0, false),
        ),
        (
            "pyramide renversée",
            Forme::pyramide([40, -20, 40], 12.0, 13.0, true),
        ),
        (
            "sphère creuse",
            Forme::sphere([40, -30, 40], 14.0).creuse(2.0),
        ),
        (
            "cylindre creux",
            Forme::cylindre([40, -30, 40], 10.0, 24.0).creuse(1.0),
        ),
        (
            "pyramide creuse",
            Forme::pyramide([40, -50, 40], 12.0, 13.0, false).creuse(2.0),
        ),
        ("pavé", Forme::pave(pave((10, -40, 10), (53, -9, 61)))),
        ("murs", Forme::murs(pave((10, -40, 10), (53, -9, 61)), 2.0)),
        (
            "faces",
            Forme::faces(pave((10, -40, 10), (53, -9, 61)), 1.0),
        ),
    ]
}

fn pave(a: (i32, i32, i32), b: (i32, i32, i32)) -> BBox {
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

/// Les cases d'une boîte. Écrite ici et pas sur `BBox` : tout ce dépôt
/// existe pour NE PAS itérer les blocs, et offrir l'itérateur inviterait à le
/// faire. Un test, lui, a le droit d'être lent.
fn cases(b: &BBox) -> impl Iterator<Item = BlockPos> + '_ {
    (b.min.y..=b.max.y).flat_map(move |y| {
        (b.min.z..=b.max.z)
            .flat_map(move |z| (b.min.x..=b.max.x).map(move |x| BlockPos { x, y, z }))
    })
}

/// Les rangs de `RapportRegion::etages` : rien, section, palette, bloc.
const RIEN: usize = 0;
const SECTION: usize = 1;
const PALETTE: usize = 2;

/// Les sections à visiter : celles de la boîte, plus une couronne autour,
/// pour que « Dehors » soit exercé et pas seulement supposé.
fn sections_autour(b: &BBox) -> Vec<SectionPos> {
    let large = BBox::new(
        BlockPos {
            x: b.min.x - 16,
            y: b.min.y - 16,
            z: b.min.z - 16,
        },
        BlockPos {
            x: b.max.x + 16,
            y: b.max.y + 16,
            z: b.max.z + 16,
        },
    );
    large.sections().collect()
}

#[test]
fn le_verdict_par_section_est_exact_sur_les_4096_cases() {
    for (nom, f) in formes() {
        let b = f.bornes().expect("une forme bornée");
        let mut par_verdict = [0usize; 3];
        for s in sections_autour(&b) {
            let o = s.min_block();
            let mut dedans = 0usize;
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        if f.contient(o.x + x, o.y + y, o.z + z) {
                            dedans += 1;
                        }
                    }
                }
            }
            match f.couverture(s) {
                Couverture::Dedans => {
                    par_verdict[0] += 1;
                    assert_eq!(
                        dedans, 4096,
                        "{nom} : « Dedans » à {s:?} en contient {dedans}"
                    );
                }
                Couverture::Dehors => {
                    par_verdict[1] += 1;
                    assert_eq!(dedans, 0, "{nom} : « Dehors » à {s:?} en contient {dedans}");
                }
                Couverture::Partielle => par_verdict[2] += 1,
            }
        }
        // Et le verdict doit VRAIMENT trancher : un `Partielle` partout serait
        // juste et sans intérêt — c'est l'étage bloc pour tout le volume.
        assert!(
            par_verdict[1] > 0,
            "{nom} : aucune section écartée, le chemin rapide ne sert à rien"
        );
        assert!(
            par_verdict[0] > 0 || par_verdict[2] > 0,
            "{nom} : la forme est vide"
        );
    }
}

/// **Le cœur d'une forme creuse doit être ÉCARTÉ**, pas parcouru.
///
/// C'est la moitié de l'intérêt d'une coque : une sphère creuse de rayon 30
/// a un cœur de 100 000 cases qu'il ne faut ni décoder ni visiter. Un
/// `Partielle` y serait juste et coûterait tout le gain — et le test
/// précédent ne le verrait pas, puisqu'il ne vérifie que l'exactitude.
#[test]
fn le_creux_d_une_coque_est_ecarte_section_par_section() {
    // La propriété : une section entièrement dans le TROU doit être écartée
    // par la coque, pas parcourue. C'est la moitié de l'intérêt d'une forme
    // creuse — une sphère creuse de rayon 30 a un cœur de 100 000 cases qu'il
    // ne faut ni décoder ni visiter.
    //
    // Le trou est reconstruit ICI, par les constructeurs publics, au lieu
    // d'être demandé à la coque : c'est une seconde écriture de la règle de
    // rétrécissement, indépendante de celle qu'on teste. Si l'une change sans
    // l'autre, ce test le dit.
    //
    // Attention à ce qu'on ne peut PAS écrire : « la section est dans la
    // forme pleine » n'implique pas « elle est dans le trou ». Une section
    // entièrement dans la sphère extérieure peut chevaucher la coque, et
    // `Partielle` y est alors la bonne réponse — c'est ce que la première
    // écriture de ce test affirmait à tort.
    let c = [40, -32, 40];
    let cas: Vec<(&str, Forme, Forme)> = vec![
        (
            "sphère 30, coque 2",
            Forme::sphere(c, 30.0).creuse(2.0),
            Forme::sphere(c, 28.0),
        ),
        (
            "cylindre 30 × 64, coque 2",
            Forme::cylindre(c, 30.0, 64.0).creuse(2.0),
            Forme::cylindre(c, 28.0, 60.0),
        ),
        (
            "pyramide 40, coque 3",
            Forme::pyramide([40, -60, 40], 40.0, 40.0, false).creuse(3.0),
            Forme::pyramide([40, -57, 40], 37.0, 37.0, false),
        ),
    ];
    for (nom, creuse, trou) in cas {
        let b = creuse.bornes().expect("bornée");
        let mut coeurs = 0usize;
        for s in b.sections() {
            if trou.couverture(s) != Couverture::Dedans {
                continue;
            }
            coeurs += 1;
            assert_eq!(
                creuse.couverture(s),
                Couverture::Dehors,
                "{nom} : la section {s:?} est dans le trou, la coque doit l'écarter"
            );
        }
        assert!(
            coeurs > 0,
            "{nom} : aucune section de cœur, le test est vide"
        );
    }
}

#[test]
fn une_sphere_a_le_diametre_qu_on_lui_demande() {
    // `//sphere 5` fait ONZE blocs de large, pas dix : c'est la convention de
    // WorldEdit, et un même chiffre doit donner le même bâtiment.
    let f = Forme::sphere([0, 0, 0], 5.0);
    let large = (-8..=8).filter(|&x| f.contient(x, 0, 0)).count();
    assert_eq!(large, 11);
    assert!(f.contient(5, 0, 0) && !f.contient(6, 0, 0));
    // Et pas de coin : une sphère n'est pas un cube.
    assert!(!f.contient(5, 5, 5));

    // Le VOLUME, parce que le diamètre sur un axe ne suffit pas à fixer la
    // convention : sans le demi-rayon, la sphère fait toujours onze blocs de
    // large et n'en contient plus que 515. C'est la même sphère vue de face
    // et une autre sphère en vrai.
    let n = cases(&f.bornes().unwrap())
        .filter(|p| f.contient(p.x, p.y, p.z))
        .count();
    assert_eq!(n, 739, "la sphère de WorldEdit, rayon 5");
}

#[test]
fn un_rayon_nul_ne_garde_que_le_centre() {
    // Le cas qui rend `NaN` si on divise sans y penser — et une comparaison
    // avec `NaN` est fausse dans les DEUX sens, donc la forme serait à la fois
    // vide et pleine.
    let f = Forme::sphere([3, 4, 5], 0.0);
    assert!(f.contient(3, 4, 5));
    assert!(!f.contient(4, 4, 5));
    assert_eq!(f.bornes().unwrap().volume(), 1);
}

#[test]
fn les_bornes_contiennent_tout_ce_que_la_forme_contient() {
    for (nom, f) in formes() {
        let b = f.bornes().expect("bornée");
        for s in sections_autour(&b) {
            let o = s.min_block();
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let p = BlockPos {
                            x: o.x + x,
                            y: o.y + y,
                            z: o.z + z,
                        };
                        if f.contient(p.x, p.y, p.z) {
                            assert!(
                                b.contains(p),
                                "{nom} : {p:?} est dedans mais hors des bornes"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn une_coque_est_creuse_et_son_epaisseur_tient() {
    let pleine = Forme::sphere([0, 0, 0], 10.0);
    let coque = pleine.clone().creuse(2.0);
    // Le cœur est vide, la surface est pleine.
    assert!(pleine.contient(0, 0, 0) && !coque.contient(0, 0, 0));
    assert!(coque.contient(10, 0, 0), "le bord extérieur reste");
    // L'épaisseur se mesure sur un axe : deux blocs, plus la tolérance du
    // demi-rayon effectif.
    let epaisseur = (0..=10).filter(|&x| coque.contient(x, 0, 0)).count();
    assert!(
        (2..=3).contains(&epaisseur),
        "coque de {epaisseur} blocs, attendu 2 à 3"
    );
    // Et elle tient aussi en diagonale : une coque dont le fond serait plus
    // épais que les flancs est un défaut classique.
    let diag = (0..=10).filter(|&d| coque.contient(d, d, 0)).count();
    assert!((1..=4).contains(&diag), "coque diagonale de {diag} blocs");
}

// ── branchée au plan ────────────────────────────────────────────────────────

fn staging() -> Staging<MemorySource, MemorySource> {
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, region(&Terrain::petite()));
    Staging::new(m, MemorySource::new())
}

#[test]
fn une_forme_ne_paie_pas_la_selection_ou_on_la_pose() {
    // Le piège `warmup(extent)` d'`ExeWorldEdit` : une sphère de soixante-deux
    // blocs posée dans une sélection « tout le build » y prenait 5,2 s.
    let plan =
        Plan::nouveau(Masque::Tout, Motif::Bloc(0)).dans(Forme::sphere([100, -30, 100], 6.0));
    let enorme = BBox::new(
        BlockPos {
            x: -10_000,
            y: -64,
            z: -10_000,
        },
        BlockPos {
            x: 10_000,
            y: 320,
            z: 10_000,
        },
    );
    let p = plan.portee(&enorme);
    assert_eq!(p.volume(), 13 * 13 * 13, "la portée est celle de la sphère");
    assert_eq!(
        p.min,
        BlockPos {
            x: 94,
            y: -36,
            z: 94
        }
    );
}

#[test]
fn un_plan_dans_une_sphere_n_ecrit_que_dedans() {
    let st = staging();
    let mut i = Interner::new();
    let marque = i.intern("minecraft:bedrock");
    let centre = [40, -30, 40];
    let forme = Forme::sphere(centre, 9.0);

    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(marque))
        .dans(forme.clone())
        .en_comptant();
    let portee = plan.portee(&BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 255,
            y: 127,
            z: 255,
        },
    ));
    let r = appliquer(&st, &SURFACE, DOSSIER, &portee, &plan, &i).unwrap();

    // Le compte exact : autant de blocs que la forme contient de cases.
    let mut attendu = 0u64;
    for p in cases(&portee) {
        if forme.contient(p.x, p.y, p.z) {
            attendu += 1;
        }
    }
    assert_eq!(r.blocs, Some(attendu), "ni un bloc de plus, ni un de moins");

    // Et on RELIT, parce qu'un compte juste sur une écriture fausse est
    // exactement ce qu'on ne veut pas croire.
    let relu = copier(&st, &SURFACE, DOSSIER, &portee, &mut i).unwrap();
    for p in cases(&portee) {
        let (x, y, z) = (
            (p.x - portee.min.x) as u32,
            (p.y - portee.min.y) as u32,
            (p.z - portee.min.z) as u32,
        );
        let vu = relu.get(x, y, z) == Some(marque);
        assert_eq!(
            vu,
            forme.contient(p.x, p.y, p.z),
            "la case {p:?} est du mauvais côté de la sphère"
        );
    }
}

#[test]
fn le_coeur_d_une_grosse_sphere_garde_l_etage_palette() {
    // Ce qui fait qu'une forme n'est pas « l'étage bloc pour tout le volume ».
    let st = staging();
    let mut i = Interner::new();
    let marque = i.intern("minecraft:bedrock");
    let plan =
        Plan::nouveau(Masque::Tout, Motif::Bloc(marque)).dans(Forme::sphere([64, -32, 64], 40.0));
    let portee = plan.portee(&BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 255,
            y: 127,
            z: 255,
        },
    ));
    let r = appliquer(&st, &SURFACE, DOSSIER, &portee, &plan, &i).unwrap();
    let rapides = r.etages[SECTION] + r.etages[PALETTE];
    assert!(
        rapides > 0,
        "aucune section du cœur n'a pris le chemin rapide : {:?}",
        r.etages
    );
    assert!(
        r.etages[RIEN] > 0,
        "aucune section écartée : {:?}",
        r.etages
    );
}

// ── murs et faces ───────────────────────────────────────────────────────────

/// **Un mur n'a pas de toit**, et c'est le sens qu'on veut : on entoure une
/// cour, on ne l'enferme pas. `faces` ferme les six côtés.
#[test]
fn un_mur_est_ouvert_en_haut_et_en_bas_les_faces_non() {
    let b = pave((0, 0, 0), (9, 9, 9));
    let murs = Forme::murs(b, 1.0);
    let faces = Forme::faces(b, 1.0);

    // Le centre du plafond : dans les faces, pas dans les murs.
    assert!(!murs.contient(5, 9, 5), "un mur n'a pas de toit");
    assert!(faces.contient(5, 9, 5), "les faces en ont un");
    assert!(!murs.contient(5, 0, 5), "ni de plancher");
    assert!(faces.contient(5, 0, 5));

    // La paroi verticale, elle, est dans les deux.
    for f in [&murs, &faces] {
        assert!(f.contient(0, 5, 5));
        assert!(f.contient(9, 5, 5));
        assert!(f.contient(5, 5, 0));
        assert!(f.contient(5, 5, 9));
        assert!(!f.contient(5, 5, 5), "le cœur est vide");
    }
}

#[test]
fn le_compte_d_un_mur_est_celui_qu_on_calcule_a_la_main() {
    // Une boîte 10 × 10 × 10, paroi d'un bloc : le pourtour fait
    // 10² − 8² = 36 cases par tranche, sur les dix tranches.
    let murs = Forme::murs(pave((0, 0, 0), (9, 9, 9)), 1.0);
    let n = cases(&murs.bornes().unwrap())
        .filter(|p| murs.contient(p.x, p.y, p.z))
        .count();
    assert_eq!(n, 36 * 10);

    // Et les six faces : le cube entier moins son intérieur 8³.
    let faces = Forme::faces(pave((0, 0, 0), (9, 9, 9)), 1.0);
    let n = cases(&faces.bornes().unwrap())
        .filter(|p| faces.contient(p.x, p.y, p.z))
        .count();
    assert_eq!(n, 1000 - 512);
}

/// Une épaisseur plus grande que la boîte ne doit pas rendre un creux
/// INVERSÉ — `min > max` — qui contiendrait tout, ou rien, selon le sens de
/// la comparaison.
#[test]
fn une_epaisseur_plus_grande_que_la_boite_donne_un_bloc_plein() {
    let b = pave((0, 0, 0), (4, 4, 4));
    let murs = Forme::murs(b, 10.0);
    let n = cases(&b).filter(|p| murs.contient(p.x, p.y, p.z)).count();
    assert_eq!(n, 125, "tout est paroi, rien n'est creux");
}

// ── `Volume` : ce qu'un hôte demande ────────────────────────────────────────

/// `TOUS` doit être complet : c'est elle qu'une interface parcourt pour
/// proposer les formes. Incomplète, elle rend une forme INATTEIGNABLE — le
/// piège « déclaré, branché, testé, et personne ne le propose ».
#[test]
fn toutes_les_formes_demandables_sont_enumerees() {
    use tf_ops::Volume;
    let mut rangs: Vec<usize> = Volume::TOUS.iter().map(|v| v.rang()).collect();
    rangs.sort_unstable();
    assert_eq!(rangs, (0..Volume::TOUS.len()).collect::<Vec<_>>());
    for v in Volume::TOUS {
        assert!(!v.nom().is_empty());
    }
}

/// **Un volume se pose sur la SÉLECTION**, et chacun à sa manière : une sphère
/// sur le centre, une pyramide sur le BAS, une enveloppe sur les parois. Se
/// tromper d'ancrage donne une pyramide flottante ou des murs décalés — et
/// les deux sont des images parfaitement plausibles.
#[test]
fn chaque_volume_se_pose_ou_il_doit() {
    use tf_ops::{Forme, Volume};
    use tf_world::coords::{BBox, BlockPos};

    let sel = BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(20, 20, 20));

    assert!(matches!(Volume::Aucun.forme(&sel, None), Forme::Boite));

    // La sphère est CENTRÉE, et ses bornes tiennent dans la sélection.
    let b = Volume::Sphere { rayon: 5.0 }
        .forme(&sel, None)
        .bornes()
        .expect("une sphère est bornée");
    assert_eq!(b.min.x, 5);
    assert_eq!(b.max.x, 15);

    // La pyramide part du BAS : sa base est à y = 0, pas au centre.
    let b = Volume::Pyramide {
        demi_base: 6.0,
        hauteur: 8.0,
        renversee: false,
    }
    .forme(&sel, None)
    .bornes()
    .expect("une pyramide est bornée");
    assert_eq!(b.min.y, 0, "la pyramide flotte");

    // Renversée, elle pend du HAUT.
    let b = Volume::Pyramide {
        demi_base: 6.0,
        hauteur: 8.0,
        renversee: true,
    }
    .forme(&sel, None)
    .bornes()
    .unwrap();
    assert_eq!(b.max.y, 20);

    // Une enveloppe est PRISE sur la sélection : elle en couvre les bornes.
    let b = Volume::Murs { epaisseur: 1.0 }
        .forme(&sel, None)
        .bornes()
        .unwrap();
    assert_eq!((b.min.x, b.max.x), (0, 20));
    assert!(Volume::Murs { epaisseur: 1.0 }.enveloppe());
    assert!(!Volume::Sphere { rayon: 1.0 }.enveloppe());
}

/// **Un volume négatif se prend en division PLANCHER.** Le bloc −1 est dans la
/// région −1 : tronquer vers zéro décale le centre d'un bloc d'un seul côté de
/// l'origine, ce qui se lit « la sphère n'est pas au milieu », et seulement
/// pour les builds à coordonnées négatives.
#[test]
fn le_centre_d_un_volume_se_prend_en_division_plancher() {
    use tf_ops::Volume;
    use tf_world::coords::{BBox, BlockPos};

    let sel = BBox::new(BlockPos::new(-9, -9, -9), BlockPos::new(0, 0, 0));
    let b = Volume::Sphere { rayon: 2.0 }
        .forme(&sel, None)
        .bornes()
        .unwrap();
    // (−9 + 0) / 2 vaut −5 en plancher, −4 en troncature.
    assert_eq!(b.min.x, -7);
    assert_eq!(b.max.x, -3);
}

/// Creuser une forme la vide GÉOMÉTRIQUEMENT — il retire le centre du volume
/// qu'on vient de poser, là où `//hollow` INSPECTE ce qui est relié au dehors.
#[test]
fn un_volume_creux_est_une_coque() {
    use tf_ops::{Couverture, Forme, Volume};
    use tf_world::coords::{BBox, BlockPos};

    let sel = BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(20, 20, 20));
    let pleine = Volume::Sphere { rayon: 8.0 }.forme(&sel, None);
    let creuse = Volume::Sphere { rayon: 8.0 }.forme(&sel, Some(2.0));
    assert!(matches!(creuse, Forme::Coque { .. }));

    // Le CENTRE est dans la pleine et hors de la creuse. C'est toute la
    // différence, et elle se dit sur un point, pas sur un type.
    let c = [10, 10, 10];
    assert!(pleine.contient(c[0], c[1], c[2]));
    assert!(!creuse.contient(c[0], c[1], c[2]));
    // Et la paroi est dans les deux.
    assert!(pleine.contient(17, 10, 10));
    assert!(creuse.contient(17, 10, 10));
    let _ = Couverture::Dedans;
}
