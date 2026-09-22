//! **Remplacer des tranches d'arène plutôt que la rebâtir.**
//!
//! Rebâtir l'arène entière à chaque édition coûtait, mesuré sur une région
//! bâtie de 256 chunks, **52 ms sur les 80** d'une édition de trois blocs :
//! du travail en O(scène) pour un geste en O(édition), la même famille que le
//! `warmup(extent)` d'`ExeWorldEdit`.
//!
//! Ce qui est vérifié ici et nulle part ailleurs : **le décalage des lots**.
//! Le champ `section` d'une instance est l'indice de son lot, qui sert à
//! retrouver son origine. Les lots sont triés par adresse : une section qui
//! apparaît ou disparaît décale tout ce qui suit, et une instance recopiée
//! telle quelle se dessine à la place d'une autre.
//!
//! La coque ne sait pas produire ce cas aujourd'hui — son écrivain ne
//! supprime jamais une section, et toutes celles du monde existent déjà — et
//! c'est pour ça qu'il se teste ICI, au niveau où il se produit. Il se
//! produira tout le temps dès que la résidence sera pilotée par la caméra :
//! des cellules entreront et sortiront à chaque pas.

use tf_anvil::{bits_for, pack, Packing, Section, StateId};
use tf_mesh::{Grille, TableFormes};
use tf_render::Arene;

const AIR: StateId = 0;
const CUBE: StateId = 1;

fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    t
}

fn section(y: i8, plein: bool) -> Section {
    let id = if plein { CUBE } else { AIR };
    let mut palette: Vec<StateId> = vec![AIR, CUBE];
    let idx = vec![id as u16; 4096];
    let bits = bits_for(palette.len());
    let data = pack(&idx, bits.into(), Packing::NoStraddle);
    palette.truncate(2);
    Section {
        y,
        palette,
        bits,
        data: data.into_boxed_slice(),
        packing: Packing::NoStraddle,
    }
}

/// Chaque instance, par ce qu'elle VEUT DIRE. La couche n'entre pas : ici
/// l'apparence est constante, ce qu'on regarde est la géométrie et le LOT.
fn sens(a: &Arene) -> Vec<(u32, u32)> {
    a.instances.iter().map(|i| (i.geo, i.section)).collect()
}

fn uni(_: StateId, _: tf_mesh::forme::Face, _: StateId) -> (u32, [f32; 3]) {
    (0, [1.0; 3])
}

/// **Une section qui APPARAÎT décale les lots suivants**, et les instances
/// recopiées doivent suivre.
#[test]
fn une_section_qui_apparait_ne_decale_pas_les_instances() {
    let t = table();
    // Deux colonnes, deux sections chacune. Les lots sont triés par adresse :
    // insérer une section en y = 0 dans la colonne (0, 0) passe DEVANT tout ce
    // qui a un y plus grand.
    let mut g = Grille::new();
    g.poser(0, 0, section(1, true));
    g.poser(1, 0, section(1, true));
    g.poser(1, 0, section(2, true));
    let chantier = g.mailler(&t);
    let mut arene = Arene::depuis(&chantier, &uni);
    let lots_avant = chantier.lots.len();

    // La section neuve, en y = 0.
    g.poser(0, 0, section(0, true));
    let chantier = g.mailler(&t);
    assert!(
        chantier.lots.len() > lots_avant,
        "la prémisse : un lot doit être apparu ({lots_avant} → {})",
        chantier.lots.len()
    );

    // Seule la section neuve — et sa voisine, qui perd des faces — est visée.
    let visees = vec![(0, 0, 0), (0, 0, 1)];
    arene.remplacer(&chantier, &visees, &uni);

    let rebatie = Arene::depuis(&chantier, &uni);
    assert_eq!(
        sens(&arene),
        sens(&rebatie),
        "une instance recopiée a gardé l'indice de lot d'avant l'insertion : \
         elle se dessinerait à la place d'une autre"
    );
    assert_eq!(
        arene.origines.len(),
        rebatie.origines.len(),
        "les origines doivent suivre les lots"
    );
    assert_eq!(arene.tranches.len(), chantier.lots.len());
}

/// **Une section qui DISPARAÎT décale dans l'autre sens.** Même mécanique,
/// et c'est le cas qu'une éviction produira à chaque pas de caméra.
#[test]
fn une_section_qui_disparait_ne_decale_pas_les_instances() {
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, section(0, true));
    g.poser(0, 0, section(1, true));
    g.poser(1, 0, section(1, true));
    let chantier = g.mailler(&t);
    let mut arene = Arene::depuis(&chantier, &uni);
    let lots_avant = chantier.lots.len();

    assert!(g.retirer((0, 0, 0)), "la section devait être là");
    let chantier = g.mailler(&t);
    assert!(
        chantier.lots.len() < lots_avant,
        "la prémisse : un lot doit avoir disparu"
    );

    arene.remplacer(&chantier, &[(0, 0, 0), (0, 0, 1)], &uni);
    let rebatie = Arene::depuis(&chantier, &uni);
    assert_eq!(
        sens(&arene),
        sens(&rebatie),
        "après un retrait, les lots suivants remontent d'un cran"
    );
    assert_eq!(arene.tranches.len(), chantier.lots.len());
}

/// Sans changement de lot, le remplacement doit être identique au bit près :
/// c'est le cas courant, et il ne doit RIEN coûter de faux.
#[test]
fn sans_changement_de_lot_le_remplacement_est_exact() {
    let t = table();
    let mut g = Grille::new();
    for (cx, y) in [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0)] {
        g.poser(cx, 0, section(y, true));
    }
    let chantier = g.mailler(&t);
    let mut arene = Arene::depuis(&chantier, &uni);

    // On vide une section SANS retirer son lot : elle reste, mais sans quads.
    g.poser(1, 0, section(0, false));
    let chantier = g.mailler(&t);
    // **Les visées DÉBORDENT d'une case**, et c'est le test lui-même qui l'a
    // rappelé : première écriture, j'avais listé à la main `(1,0,0)`,
    // `(1,0,1)` et `(0,0,0)`, en oubliant `(2,0,0)`. Or vider une section
    // expose la face −X de sa VOISINE : l'arène rebâtie portait un quad de
    // plus, et l'écart accusait le remplacement d'un défaut qui venait de la
    // liste. C'est la règle du remaillage local, qu'on ne recopie donc pas à
    // la main.
    let visees = Grille::sections_autour([16, 0, 0], [31, 15, 15]);
    arene.remplacer(&chantier, &visees, &uni);

    let rebatie = Arene::depuis(&chantier, &uni);
    assert_eq!(sens(&arene), sens(&rebatie));
    assert_eq!(
        arene.instances.len(),
        rebatie.instances.len(),
        "et le même nombre d'instances"
    );
}
