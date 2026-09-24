//! Le tableau par pages : grandir sans recopier, et rester un tableau.

use tf_render::{Pages, PAGE};

/// **Grandir ne déplace rien.** C'est toute la raison d'être : une page
/// existante garde son adresse quand une autre s'ajoute. Un `Vec` qui double
/// sa capacité recopie tout, et c'est ce pic-là — 16 ms pour 2,4 millions
/// d'instances — que la structure existe pour supprimer.
#[test]
fn grandir_ne_deplace_pas_les_pages_existantes() {
    let mut p = Pages::new(0u32);
    p.resize(PAGE + 10);
    let avant: Vec<*const u32> = p.pages().map(|s| s.as_ptr()).collect();
    p.resize(10 * PAGE);
    let apres: Vec<*const u32> = p.pages().map(|s| s.as_ptr()).collect();
    assert_eq!(
        &apres[..avant.len()],
        &avant[..],
        "une page existante a bougé"
    );
}

/// Un tableau par pages se lit, s'écrit et se parcourt comme un tableau —
/// croisé avec un `Vec` sur une suite d'opérations qui traversent les
/// frontières de page, dans les deux sens.
#[test]
fn il_se_comporte_comme_un_vec() {
    let mut p = Pages::new(7u32);
    let mut v: Vec<u32> = Vec::new();
    let mut graine = 3u64;
    let mut tirer = |b: usize| {
        graine = graine.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((graine >> 33) as usize) % b
    };
    for _ in 0..200 {
        match tirer(4) {
            0 => {
                let n = tirer(3 * PAGE);
                p.resize(n);
                v.resize(n, 7);
            }
            1 if !v.is_empty() => {
                let i = tirer(v.len());
                let x = tirer(1000) as u32;
                p[i] = x;
                v[i] = x;
            }
            2 if !v.is_empty() => {
                let a = tirer(v.len());
                let b = a + tirer(v.len() - a + 1);
                p.fill(a, b, 9);
                v[a..b].fill(9);
            }
            _ => {
                let n = v.len().saturating_sub(tirer(PAGE));
                p.truncate(n);
                v.truncate(n);
            }
        }
        assert_eq!(p.len(), v.len());
    }
    assert!(
        p.iter().copied().eq(v.iter().copied()),
        "le contenu diffère"
    );
    let recolle: Vec<u32> = p.pages().flatten().copied().collect();
    assert_eq!(
        recolle, v,
        "les pages ne recouvrent pas exactement le tableau"
    );
}

/// **Une case recréée après un raccourcissement vaut `vide`**, pas ce qu'elle
/// portait. Sans ça, grandir de nouveau ferait réapparaître d'anciennes
/// instances — de la géométrie d'une section partie depuis longtemps.
#[test]
fn une_case_recreee_vaut_vide() {
    let mut p = Pages::new(0u32);
    p.resize(100);
    p.fill(0, 100, 5);
    p.truncate(40);
    p.resize(100);
    assert!(
        p.iter().skip(40).all(|&x| x == 0),
        "une case recréée garde son ancien contenu"
    );
}

/// Une plage se découpe en tranches contiguës, une par page traversée, sans
/// trou ni recouvrement.
#[test]
fn une_plage_se_decoupe_aux_frontieres_de_page() {
    let mut p = Pages::new(0u32);
    p.resize(3 * PAGE);
    for i in 0..p.len() {
        p[i] = i as u32;
    }
    let (a, b) = (PAGE - 5, 2 * PAGE + 7);
    let mut attendu = a;
    let mut morceaux = 0;
    for (d, t) in p.plage(a, b) {
        assert_eq!(d, attendu, "trou ou recouvrement");
        assert!(t.iter().enumerate().all(|(k, &x)| x as usize == d + k));
        attendu += t.len();
        morceaux += 1;
    }
    assert_eq!(attendu, b);
    assert_eq!(morceaux, 3, "trois pages traversées, trois tranches");
}

/// Raccourcir rend la mémoire des pages entièrement au-delà.
#[test]
fn raccourcir_rend_les_pages() {
    let mut p = Pages::new(0u64);
    p.resize(8 * PAGE);
    p.truncate(PAGE + 1);
    assert_eq!(p.octets_reserves(), 2 * PAGE * 8);
}
