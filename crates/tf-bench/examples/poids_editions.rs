//! Combien d'octets une opération fait-elle vraiment réécrire ?
//!
//! Criterion mesure du temps ; c'est le POIDS qui décide de la taille du
//! journal d'annulation. Une mesure, pas une estimation.

use tf_anvil::{decode_section, inflate, read, scan, section_edits, Interner, SectionSpans};
use tf_bench::{region, Terrain};

fn main() {
    let t = Terrain::region_pleine();
    let bytes = region(&t);
    let r = read(&bytes, 0, 0).unwrap();

    let mut total_chunk = 0usize;
    let mut total_edits = 0usize;
    let mut sections = 0usize;
    let mut touchees = 0usize;
    let mut naif = 0usize;

    for lz in 0..32 {
        for lx in 0..32 {
            let Some(raw) = r.get(lx, lz) else { continue };
            let inflated = inflate(&raw.payload, raw.compression).unwrap();
            total_chunk += inflated.len();
            let sc = scan(&inflated).unwrap();
            let mut interner = Interner::new();
            for s in &sc.sections {
                let Some(mut sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap()
                else {
                    continue;
                };
                sections += 1;
                let Some(de) = interner.get("minecraft:stone") else {
                    continue;
                };
                let vers = interner.intern("minecraft:deepslate");
                if sec.replace_state(de, vers) == 0 {
                    continue;
                }
                touchees += 1;
                // Ce que coûterait la réécriture du bloc de champs ENTIER —
                // ce que faisait le code avant qu'une édition soit resserrée.
                if let Some(sp) = s.spans {
                    naif += match sp {
                        SectionSpans::Flat { palette, data, .. } => {
                            palette.len() + data.map_or(0, |d| d.len())
                        }
                        SectionSpans::Legacy {
                            palette, blocks, ..
                        } => palette.len() + blocks.map_or(0, |b| b.len()),
                    };
                }
                for e in section_edits(&inflated, &sec, s, &interner).unwrap() {
                    total_edits += e.bytes.len();
                }
            }
        }
    }

    println!("région    : {} blocs", t.blocs());
    println!("chunks    : {total_chunk} octets inflatés au total");
    println!("sections  : {sections} dont {touchees} touchées");
    println!("bloc entier : {naif} octets (ce que coûtait la réécriture complète)");
    println!(
        "réécrit   : {total_edits} octets, soit × {:.0} moins",
        naif as f64 / total_edits as f64
    );
    println!(
        "rapport   : {:.4} % du chunk — le journal stocke ça DEUX fois (avant, après)",
        100.0 * total_edits as f64 / total_chunk as f64
    );
}
