//! Viser : quel bloc, et quelle FACE.
//!
//! Tout ce qui se fait à la souris en dépend — poser un coin, poser un bloc,
//! en casser un, pousser-tirer une face. Et tout s'y trompe de la même
//! façon : d'une case, ou d'un côté.

use tf_mesh::forme::Face;
use tf_render::viser::{rayon_ecran, viser, PAS_MAX};
use tf_render::Camera;

/// Un monde d'essai : un unique bloc solide.
fn bloc(c: [i32; 3]) -> impl Fn([i32; 3]) -> bool {
    move |p| p == c
}

/// Un plancher plein à y < 0 — le cas le plus courant de tous.
fn plancher(p: [i32; 3]) -> bool {
    p[1] < 0
}

fn vide(_: [i32; 3]) -> bool {
    false
}

// ── la case et la face ──────────────────────────────────────────────────────

/// **La face traversée est celle du côté d'où l'on vient.**
///
/// En avançant vers +X on entre par la face −X. L'inverser ferait poser les
/// blocs de l'autre côté du mur : ça se lit « l'outil vise à côté » et ça ne
/// désigne pas la cause. Les six directions, une par une, parce qu'une seule
/// prouverait seulement qu'un axe est juste.
#[test]
fn on_entre_par_la_face_opposee_au_sens_de_marche() {
    let cible = [5, 0, 0];
    let cas: [([f32; 3], [f32; 3], Face); 6] = [
        ([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], Face::MoinsX),
        ([10.5, 0.5, 0.5], [-1.0, 0.0, 0.0], Face::PlusX),
        ([5.5, -6.5, 0.5], [0.0, 1.0, 0.0], Face::MoinsY),
        ([5.5, 6.5, 0.5], [0.0, -1.0, 0.0], Face::PlusY),
        ([5.5, 0.5, -6.5], [0.0, 0.0, 1.0], Face::MoinsZ),
        ([5.5, 0.5, 6.5], [0.0, 0.0, -1.0], Face::PlusZ),
    ];
    for (o, d, attendue) in cas {
        let t = viser(o, d, 64.0, &bloc(cible)).expect("le rayon doit toucher");
        assert_eq!(t.case, cible, "depuis {o:?} vers {d:?}");
        assert_eq!(t.face, Some(attendue), "depuis {o:?} vers {d:?}");
    }
}

/// **Poser et effacer ne visent pas la même case.**
///
/// Piège payé dans `ExeWorldEdit` : un rayon touche une FACE, donc un plan
/// entre deux cases. `case` est celle qu'on casse, `avant` celle où l'on
/// pose — et `avant` est toujours la voisine par la face traversée. Laisser
/// l'appelant refaire le pas de son côté donne un outil qui pose un bloc DANS
/// le mur une fois sur deux.
#[test]
fn la_case_d_avant_est_la_voisine_par_la_face_touchee() {
    let t = viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 64.0, &bloc([5, 0, 0])).unwrap();
    assert_eq!(t.case, [5, 0, 0], "on casse celle-là");
    assert_eq!(t.avant, Some([4, 0, 0]), "on pose dans celle-là");

    // Et la relation tient : avant = case + le pas de la face traversée.
    let p = t.face.unwrap().pas();
    let c = t.case;
    assert_eq!(t.avant, Some([c[0] + p[0], c[1] + p[1], c[2] + p[2]]));
}

/// Un rayon qui part DANS un solide n'a ni face ni case d'avant. Inventer
/// l'une des deux ferait poser un bloc à un endroit arbitraire — et la caméra
/// dans un mur n'est pas un cas rare quand on vole.
#[test]
fn partir_dans_un_solide_ne_rend_ni_face_ni_case_d_avant() {
    let t = viser([5.5, 0.5, 0.5], [1.0, 0.0, 0.0], 64.0, &bloc([5, 0, 0])).unwrap();
    assert_eq!(t.case, [5, 0, 0]);
    assert_eq!(t.avant, None);
    assert_eq!(t.face, None);
    assert_eq!(t.distance, 0.0);
}

// ── les coordonnées négatives ───────────────────────────────────────────────

/// **Le bloc −0,5 est dans la case −1, pas la case 0.**
///
/// L'invariant du dépôt, ici aussi : une division qui tronque vers zéro fait
/// viser la case d'à côté, et seulement du côté négatif. Un défaut qui ne se
/// voit que dans un quart du monde est un défaut qu'on met longtemps à
/// reproduire.
#[test]
fn les_coordonnees_negatives_utilisent_la_division_plancher() {
    // Départ à x = −0,5, donc dans la case −1.
    let t = viser([-0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 64.0, &bloc([-1, 0, 0])).unwrap();
    assert_eq!(t.case, [-1, 0, 0], "on part DANS la case −1");
    assert_eq!(t.avant, None);

    // Et en marchant vers les négatifs.
    let t = viser([2.5, 0.5, 0.5], [-1.0, 0.0, 0.0], 64.0, &bloc([-3, 0, 0])).unwrap();
    assert_eq!(t.case, [-3, 0, 0]);
    assert_eq!(t.avant, Some([-2, 0, 0]));
    assert_eq!(t.face, Some(Face::PlusX));
}

// ── la distance et la portée ────────────────────────────────────────────────

/// La distance est celle du rayon, dans l'unité de la direction. Avec une
/// direction normalisée, elle est en blocs — et c'est ce dont un outil a
/// besoin pour dire « à 42 blocs ».
#[test]
fn la_distance_est_celle_du_rayon() {
    let t = viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 64.0, &bloc([5, 0, 0])).unwrap();
    // De x = 0,5 au plan x = 5 : 4,5.
    assert!((t.distance - 4.5).abs() < 1e-4, "distance {}", t.distance);
}

/// **La portée BORNE vraiment.** Un rayon qui va plus loin que la portée ne
/// touche rien : une sélection ne doit pas accrocher un bloc à trois cents
/// mètres parce qu'il se trouvait dans l'axe.
#[test]
fn au_dela_de_la_portee_on_ne_touche_rien() {
    let cible = [100, 0, 0];
    assert!(viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0, &bloc(cible)).is_none());
    assert!(viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 200.0, &bloc(cible)).is_some());
}

/// Un monde vide ne rend rien, et la boucle s'arrête bien — c'est la portée
/// qui la termine, pas le garde-fou.
#[test]
fn un_monde_vide_ne_touche_rien() {
    assert!(viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 500.0, &vide).is_none());
}

// ── les cas dégénérés ───────────────────────────────────────────────────────

/// **Une entrée dégénérée est refusée TOUT DE SUITE, pas au bout de seize
/// mille pas.**
///
/// Première écriture du test : je vérifiais seulement que le résultat est
/// `None`. Il l'est déjà SANS la garde — `INFINITY > portee` finit par
/// couper, ou le garde-fou. La mutation « retirer la garde » passait donc
/// tous les tests, et c'est le genre de vert qui ne prouve rien.
///
/// Ce que la garde achète n'est pas le résultat, c'est le COÛT : `arrete` est
/// une lecture du MONDE, qui décode des chunks. L'appeler des milliers de
/// fois par image sur un rayon qui ne vise rien est exactement le défaut
/// qu'on ne voit pas avant de profiler. Et avec une origine `NaN`, aucune
/// comparaison n'est vraie — la marche va jusqu'au garde-fou à chaque
/// image.
///
/// On compte donc les appels.
#[test]
fn une_entree_degeneree_ne_touche_pas_le_monde() {
    use std::cell::Cell;
    let cas: [([f32; 3], [f32; 3], f32); 6] = [
        ([0.5, 0.5, 0.5], [0.0, 0.0, 0.0], 64.0),
        ([0.5, 0.5, 0.5], [f32::NAN, 0.0, 1.0], 64.0),
        ([0.5, 0.5, 0.5], [f32::INFINITY, 0.0, 0.0], 64.0),
        ([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], 64.0),
        ([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], -1.0),
        ([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], f32::NAN),
    ];
    for (o, d, p) in cas {
        let appels = Cell::new(0u32);
        let t = viser(o, d, p, &|c| {
            appels.set(appels.get() + 1);
            plancher(c)
        });
        assert!(t.is_none(), "origine {o:?} direction {d:?} portée {p}");
        assert!(
            appels.get() <= 1,
            "origine {o:?} direction {d:?} portée {p} : {} lectures du monde \
             pour un rayon qui ne vise rien",
            appels.get()
        );
    }
}

/// Le même, sur le seul RÉSULTAT — ce qu'un appelant observe.
#[test]
fn une_direction_degeneree_ne_vise_rien() {
    for d in [
        [0.0, 0.0, 0.0],
        [f32::NAN, 0.0, 1.0],
        [f32::INFINITY, 0.0, 0.0],
    ] {
        assert!(
            viser([0.5, 0.5, 0.5], d, 64.0, &plancher).is_none(),
            "direction {d:?}"
        );
    }
    // Une origine non finie non plus, et une portée absurde non plus.
    assert!(viser([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], 64.0, &plancher).is_none());
    assert!(viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], -1.0, &plancher).is_none());
    assert!(viser([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], f32::NAN, &plancher).is_none());
}

/// **Un rayon parfaitement axial ne demande aucun cas particulier.**
///
/// Une composante exactement nulle laisse sa distance à `INFINITY` : cet axe
/// n'est jamais le minimum, donc on ne franchit jamais un de ses plans. C'est
/// exactement le comportement voulu, et c'est le cas le plus COURANT — un
/// utilisateur qui regarde droit devant lui.
#[test]
fn un_rayon_axial_marche_sans_dévier() {
    let t = viser([0.5, 7.5, 0.5], [0.0, -1.0, 0.0], 64.0, &plancher).unwrap();
    assert_eq!(t.case, [0, -1, 0], "le premier bloc du plancher");
    assert_eq!(t.avant, Some([0, 0, 0]));
    assert_eq!(t.face, Some(Face::PlusY), "on arrive par le DESSUS");
}

/// Le garde-fou existe et il est atteignable : une direction minuscule fait
/// des pas minuscules, et il vaut mieux rendre « rien » qu'une case tirée au
/// sort après seize mille pas — ou ne jamais rendre du tout.
#[test]
fn le_garde_fou_termine_une_marche_interminable() {
    // Des pas de 1/10 000 de bloc : la portée seule n'arrêterait la boucle
    // qu'après des millions d'itérations.
    let d = [1e-4f32, 0.0, 0.0];
    let t = viser([0.5, 0.5, 0.5], d, f32::MAX / 2.0, &bloc([9_999_999, 0, 0]));
    assert!(t.is_none(), "le garde-fou doit couper");
    // Et il reste généreux pour l'usage réel : une portée de joueur fait
    // quelques dizaines de blocs, soit au plus quelques centaines de pas.
    const _: () = assert!(PAS_MAX >= 1024);
}

/// Sur un plancher, une portée réaliste de joueur touche en quelques pas
/// quelle que soit l'inclinaison — le cas nominal, balayé.
#[test]
fn un_plancher_se_touche_sous_tous_les_angles() {
    for a in 0..32 {
        let t = (a as f32) / 32.0 * std::f32::consts::TAU;
        let d = [t.cos() * 0.6, -0.5, t.sin() * 0.6];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let d = [d[0] / n, d[1] / n, d[2] / n];
        let t = viser([0.5, 5.5, 0.5], d, 64.0, &plancher)
            .unwrap_or_else(|| panic!("angle {a} : rien touché"));
        assert!(t.case[1] < 0, "on touche le plancher");
        assert_eq!(t.avant.map(|c| c[1]), Some(0), "et on pose juste au-dessus");
        assert_eq!(t.face, Some(Face::PlusY));
    }
}

// ── le rayon de l'écran ─────────────────────────────────────────────────────

fn cam(oeil: [f32; 3], cible: [f32; 3]) -> Camera {
    Camera {
        oeil,
        cible,
        fov: 50f32.to_radians(),
        proche: 0.1,
        loin: 1000.0,
    }
}

/// Le centre de l'écran vise exactement devant — c'est là qu'est le réticule.
#[test]
fn le_centre_de_l_ecran_vise_droit_devant() {
    let c = cam([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let d = rayon_ecran(&c, [0.0, 0.0], 16.0 / 9.0);
    assert!(
        (0..3).all(|k| (d[k] - [1.0, 0.0, 0.0][k]).abs() < 1e-5),
        "{d:?}"
    );
}

/// **Les signes de l'écran se disent, ils ne se devinent pas.**
///
/// +1 en x vise à DROITE, +1 en y vise en HAUT. Un axe inversé donne un outil
/// qui vise symétriquement — parfaitement plausible, et faux. C'est à la
/// coque de convertir les pixels d'une souris, comptés depuis le haut ; ici
/// la convention est fixée et vérifiée.
#[test]
fn les_bords_de_l_ecran_visent_du_bon_cote() {
    // On regarde vers l'EST (+X). La droite de l'écran est alors le SUD (+Z),
    // par la règle du repère : +X = Est, +Z = Sud, +Y = Haut.
    let c = cam([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let droite = rayon_ecran(&c, [1.0, 0.0], 1.0);
    assert!(
        droite[2] > 0.1,
        "la droite de l'écran va au Sud : {droite:?}"
    );
    let gauche = rayon_ecran(&c, [-1.0, 0.0], 1.0);
    assert!(gauche[2] < -0.1, "et la gauche au Nord : {gauche:?}");

    let haut = rayon_ecran(&c, [0.0, 1.0], 1.0);
    assert!(haut[1] > 0.1, "le haut de l'écran monte : {haut:?}");
    let bas = rayon_ecran(&c, [0.0, -1.0], 1.0);
    assert!(bas[1] < -0.1, "et le bas descend : {bas:?}");
}

/// **L'aspect élargit le champ HORIZONTAL, pas le vertical.**
///
/// C'est la convention de la matrice de projection (`fov` est vertical) : s'en
/// écarter ici ferait viser à côté du curseur, de plus en plus loin du centre.
/// Le défaut se lit « ma souris est décalée » et on cherche dans la fenêtre.
#[test]
fn l_aspect_elargit_l_horizontale_et_laisse_la_verticale() {
    let c = cam([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let large = rayon_ecran(&c, [1.0, 0.0], 2.0);
    let carre = rayon_ecran(&c, [1.0, 0.0], 1.0);
    assert!(
        large[2] > carre[2],
        "un écran large vise plus loin sur les côtés : {large:?} vs {carre:?}"
    );
    let h2 = rayon_ecran(&c, [0.0, 1.0], 2.0);
    let h1 = rayon_ecran(&c, [0.0, 1.0], 1.0);
    assert!(
        (h2[1] - h1[1]).abs() < 1e-6,
        "la verticale ne doit PAS dépendre de l'aspect"
    );
}

/// La chaîne complète : une caméra, un curseur, un monde — et le bloc visé.
/// C'est la jonction, et c'est là que ça casse.
#[test]
fn la_chaine_camera_vers_bloc_tient_de_bout_en_bout() {
    // L'œil à 5 de haut, regardant vers le bas et l'Est.
    let c = cam([0.5, 5.0, 0.5], [10.5, 0.0, 0.5]);
    let d = rayon_ecran(&c, [0.0, 0.0], 16.0 / 9.0);
    let t = viser(c.oeil, d, 64.0, &plancher).expect("le réticule doit toucher le sol");
    assert!(t.case[1] < 0, "sous le niveau zéro");
    assert_eq!(t.face, Some(Face::PlusY), "par le dessus");
    assert_eq!(t.avant.map(|p| p[1]), Some(0));
    // Et on va bien vers l'Est : la case touchée est devant, pas derrière.
    assert!(t.case[0] > 0, "vers l'Est : {:?}", t.case);
}

// ── les deux tables de directions ───────────────────────────────────────────

/// **`tf_mesh::forme::Face` et `tf_world::Direction` disent la même chose, et
/// on le MESURE.**
///
/// Les deux décrivent les six directions du repère Minecraft. Elles ne
/// peuvent pas partager un type : ni `tf-world` ni `tf-mesh` ne dépend de
/// l'autre, et les faire dépendre mettrait le mailleur sous l'éditeur ou
/// l'inverse. Ce dépôt a payé quatre fois le piège des deux tables qui
/// divergent — la rotation des variantes, les touches de `we-engine`, la
/// table de `tf-bench`, les deux miroirs. La parade n'est pas d'espérer
/// qu'elles restent d'accord.
///
/// Le croisement porte sur ce qu'elles VEULENT DIRE — le pas vers le voisin —
/// et pas sur leur rang, qui serait une coïncidence d'écriture.
/// `viser` rend une `Face`, `Selection::agrandir` prend une `Direction` : un
/// désaccord ferait tirer la paroi opposée à celle qu'on a visée.
#[test]
fn les_deux_tables_de_directions_disent_la_meme_chose() {
    use tf_mesh::forme::FACES;
    use tf_world::selection::DIRECTIONS;

    assert_eq!(FACES.len(), DIRECTIONS.len());
    for (f, d) in FACES.iter().zip(DIRECTIONS.iter()) {
        assert_eq!(f.pas(), d.pas(), "{f:?} et {d:?} ne pointent pas pareil");
        assert_eq!(f.axe(), d.axe(), "{f:?} et {d:?} : axes différents");
        assert_eq!(
            f.positif(),
            d.positif(),
            "{f:?} et {d:?} : signes différents"
        );
        assert_eq!(
            f.opposee().pas(),
            d.opposee().pas(),
            "{f:?} et {d:?} : opposées différentes"
        );
    }
    // Et les six sont bien distinctes des deux côtés — une table qui
    // dupliquerait une direction passerait la boucle ci-dessus.
    let pas: std::collections::BTreeSet<[i32; 3]> = DIRECTIONS.iter().map(|d| d.pas()).collect();
    assert_eq!(pas.len(), 6);
}

/// La chaîne que le croisement protège : ce que `viser` rend doit pouvoir
/// tirer la BONNE paroi d'une sélection.
#[test]
fn la_face_visee_dans_le_monde_tire_la_bonne_paroi() {
    use tf_mesh::forme::FACES;
    use tf_world::coords::BlockPos;
    use tf_world::selection::{Selection, DIRECTIONS};

    // Un bloc solide isolé, visé depuis six côtés.
    let cible = [5, 0, 0];
    let depuis: [([f32; 3], [f32; 3]); 6] = [
        ([0.5, 0.5, 0.5], [1.0, 0.0, 0.0]),
        ([10.5, 0.5, 0.5], [-1.0, 0.0, 0.0]),
        ([5.5, -6.5, 0.5], [0.0, 1.0, 0.0]),
        ([5.5, 6.5, 0.5], [0.0, -1.0, 0.0]),
        ([5.5, 0.5, -6.5], [0.0, 0.0, 1.0]),
        ([5.5, 0.5, 6.5], [0.0, 0.0, -1.0]),
    ];
    for (o, d) in depuis {
        let t = viser(o, d, 64.0, &bloc(cible)).expect("touche");
        let face = t.face.expect("une face");
        // Le rang de la face dans FACES désigne la direction de même rang.
        let rang = FACES.iter().position(|f| *f == face).unwrap();
        let dir = DIRECTIONS[rang];

        let mut s = Selection::nouvelle();
        s.poser_coin1(BlockPos::new(cible[0], cible[1], cible[2]));
        s.poser_coin2(BlockPos::new(cible[0], cible[1], cible[2]));
        let avant = s.boite().unwrap();
        s.agrandir(dir, 2);
        let apres = s.boite().unwrap();

        // La face qui a bougé doit être celle du côté d'où l'on vient.
        let pas = dir.pas();
        let k = dir.axe();
        let (a0, a1) = (
            [avant.min.x, avant.min.y, avant.min.z][k],
            [avant.max.x, avant.max.y, avant.max.z][k],
        );
        let (b0, b1) = (
            [apres.min.x, apres.min.y, apres.min.z][k],
            [apres.max.x, apres.max.y, apres.max.z][k],
        );
        if pas[k] > 0 {
            assert_eq!((b0, b1), (a0, a1 + 2), "depuis {o:?} vers {d:?}");
        } else {
            assert_eq!((b0, b1), (a0 - 2, a1), "depuis {o:?} vers {d:?}");
        }
        // Et la sélection s'est étendue VERS l'observateur, jamais au travers.
        let vers_nous = (o[k] - cible[k] as f32).signum() as i32;
        assert_eq!(pas[k].signum(), vers_nous, "depuis {o:?} : mauvais côté");
    }
}
