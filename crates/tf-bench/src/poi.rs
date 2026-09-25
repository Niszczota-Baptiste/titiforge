//! Des POINTS D'INTÉRÊT plausibles — `poi/r.X.Z.mca` — construits à la volée.
//!
//! Écrits comme le jeu (1.18) : `{Sections: {"<y>": {Records, Valid}},
//! DataVersion}`. Un enregistrement porte un `Valid` imbriqué nulle part —
//! mais un MOD pourrait : la fixture en glisse un dans un enregistrement, pour
//! qu'une invalidation qui descendrait trop loin se voie.

use tf_nbt::{tag, Writer};

/// Une section de points d'intérêt.
#[derive(Debug, Clone)]
pub struct SectionPoi {
    /// L'index de section, tel qu'écrit en CLÉ (« -4 », « 0 »…).
    pub y: i8,
    /// `None` : pas de champ `Valid` du tout (le jeu le lit comme faux).
    pub valid: Option<bool>,
    /// Des lits, à ces cases.
    pub lits: Vec<[i32; 3]>,
}

pub fn chunk_poi(dv: i32, sections: &[SectionPoi]) -> Vec<u8> {
    let mut w = Writer::with_capacity(1024);
    w.field(tag::COMPOUND, "");
    w.field(tag::COMPOUND, "Sections");
    for s in sections {
        w.field(tag::COMPOUND, &s.y.to_string());
        w.field(tag::LIST, "Records");
        if s.lits.is_empty() {
            w.list_header(tag::END, 0);
        } else {
            w.list_header(tag::COMPOUND, s.lits.len());
            for p in &s.lits {
                w.field(tag::INT_ARRAY, "pos").i32_payload(3);
                for x in p {
                    w.i32_payload(*x);
                }
                w.field(tag::STRING, "type").raw_str("minecraft:home");
                w.field(tag::INT, "free_tickets").i32_payload(1);
                // Le piège : un `Valid` qui n'est PAS celui de la section.
                w.field(tag::COMPOUND, "mod:extra");
                w.field(tag::BYTE, "Valid").i8_payload(1);
                w.end();
                w.end();
            }
        }
        if let Some(v) = s.valid {
            w.field(tag::BYTE, "Valid").i8_payload(v as i8);
        }
        w.end();
    }
    w.end();
    w.field(tag::INT, "DataVersion").i32_payload(dv);
    w.end();
    w.into_bytes()
}

/// Un `.mca` de points d'intérêt : un chunk par entrée `(cx, cz, sections)`.
pub fn region_poi(rx: i32, rz: i32, chunks: &[(i32, i32, Vec<SectionPoi>)]) -> Vec<u8> {
    let encodes: Vec<(i32, i32, Vec<u8>)> = chunks
        .iter()
        .map(|(cx, cz, s)| (*cx, *cz, chunk_poi(crate::mobiles::DV_1_18_2, s)))
        .collect();
    crate::region_de_chunks(rx, rz, &encodes)
}
