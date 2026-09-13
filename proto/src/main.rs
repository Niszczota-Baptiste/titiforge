//! Prototype de performance — il ne prouve QUE des chiffres.
//!
//! Aucune architecture n'est écrite tant que ces mesures ne tiennent pas. C'est
//! le piège n° 1 du CLAUDE.md de we-engine appliqué à la réécriture elle-même :
//! « optimiser sans mesurer ». On mesure d'abord, on conçoit ensuite.
//!
//! Trois questions, et une seule réponse acceptable pour chacune :
//!   1. Décoder une région pleine  — cible < 150 ms   (we-engine ici : 1010 ms)
//!   2. Empreinte résidente         — cible < 2 o/bloc (we-engine ici : 9,0 o/bloc)
//!   3. //replace sur la région     — cible < 20 ms    (we-engine ici : 6219 ms)
//!
//! Le fichier mesuré est celui que fabrique `bench/lib/fixtures.js` de
//! we-engine : même octets pour les deux moteurs, sinon la comparaison ne vaut
//! rien.

mod nbt;
mod section;

use nbt::*;
use rayon::prelude::*;
use section::{bits_for, Section, VOL};
use std::collections::HashMap;
use std::time::Instant;

const SECTOR: usize = 4096;

// ─────────────────────────────────────────────────────────────────────────────
// Lecture du .mca
// ─────────────────────────────────────────────────────────────────────────────

struct RawSection {
    y: i8,
    palette: Vec<String>,
    bits: u8,
    data: Box<[u64]>,
}

/// Clé d'identité d'un bloc — MÊME règle que `entryKey` de we-engine :
/// un bloc sans état rend son nom tel quel, sinon `nom|k=v,k=v` trié.
/// Deux règles différentes dédoubleraient les palettes (piège déjà payé
/// avec `grass_block[snowy=false]`).
fn state_key(name: &str, props: &mut Vec<(String, String)>) -> String {
    if props.is_empty() {
        return name.to_string();
    }
    props.sort();
    let mut s = String::with_capacity(name.len() + props.len() * 12);
    s.push_str(name);
    s.push('|');
    for (i, (k, v)) in props.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(k);
        s.push('=');
        s.push_str(v);
    }
    s
}

/// Extraction CIBLÉE : on ne descend que dans `sections[].block_states`.
/// Tout le reste du chunk (Heightmaps, block_entities, structures…) est
/// enjambé sans être matérialisé.
fn parse_chunk(bytes: &[u8]) -> R<Vec<RawSection>> {
    let mut c = Cur::new(bytes);
    if c.u8()? != COMPOUND {
        return Err(Trunc);
    }
    let _ = c.str()?; // nom de la racine

    loop {
        let t = c.u8()?;
        if t == END {
            return Ok(Vec::new()); // chunk sans `sections`
        }
        let key = c.str()?;
        if t == LIST && key == "sections" {
            let et = c.u8()?;
            let n = c.i32()?.max(0) as usize;
            if et != COMPOUND {
                return Ok(Vec::new());
            }
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                if let Some(s) = parse_section(&mut c)? {
                    out.push(s);
                }
            }
            return Ok(out);
        }
        c.skip_payload(t)?;
    }
}

fn parse_section(c: &mut Cur) -> R<Option<RawSection>> {
    let mut y: i8 = 0;
    let mut palette: Vec<String> = Vec::new();
    let mut data: Box<[u64]> = Box::new([]);
    let mut seen_states = false;

    loop {
        let t = c.u8()?;
        if t == END {
            break;
        }
        let key = c.str()?;
        match (t, key) {
            (BYTE, "Y") => y = c.u8()? as i8,
            (COMPOUND, "block_states") => {
                seen_states = true;
                parse_block_states(c, &mut palette, &mut data)?;
            }
            _ => c.skip_payload(t)?,
        }
    }
    if !seen_states || palette.is_empty() {
        return Ok(None); // section vide : elle n'occupe RIEN
    }
    let bits = bits_for(palette.len());
    Ok(Some(RawSection { y, palette, bits, data }))
}

fn parse_block_states(c: &mut Cur, palette: &mut Vec<String>, data: &mut Box<[u64]>) -> R<()> {
    loop {
        let t = c.u8()?;
        if t == END {
            return Ok(());
        }
        let key = c.str()?;
        match (t, key) {
            (LIST, "palette") => {
                let et = c.u8()?;
                let n = c.i32()?.max(0) as usize;
                if et != COMPOUND {
                    return Err(Trunc);
                }
                palette.reserve(n);
                for _ in 0..n {
                    palette.push(parse_palette_entry(c)?);
                }
            }
            (LONG_ARRAY, "data") => {
                let n = c.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    c.need_long()?;
                    v.push(c.u64_be());
                }
                *data = v.into_boxed_slice();
            }
            _ => c.skip_payload(t)?,
        }
    }
}

fn parse_palette_entry(c: &mut Cur) -> R<String> {
    let mut name = String::new();
    let mut props: Vec<(String, String)> = Vec::new();
    loop {
        let t = c.u8()?;
        if t == END {
            break;
        }
        let key = c.str()?;
        match (t, key) {
            (STRING, "Name") => name = c.str()?.to_string(),
            (COMPOUND, "Properties") => loop {
                let pt = c.u8()?;
                if pt == END {
                    break;
                }
                let pk = c.str()?.to_string();
                if pt == STRING {
                    props.push((pk, c.str()?.to_string()));
                } else {
                    c.skip_payload(pt)?;
                }
            },
            _ => c.skip_payload(t)?,
        }
    }
    Ok(state_key(&name, &mut props))
}

// ─────────────────────────────────────────────────────────────────────────────

fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = s.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    pages * 4096.0 / 1_048_576.0
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: tf-proto <r.X.Z.mca>");
    let file = std::fs::File::open(&path).expect("ouverture");
    let map = unsafe { memmap2::Mmap::map(&file) }.expect("mmap");
    let buf: &[u8] = &map;
    println!("fichier   {}  ({:.2} Mio, mmapé)", path, buf.len() as f64 / 1_048_576.0);
    println!("machine   {} cœurs\n", rayon::current_num_threads());

    // ── en-tête : 1024 entrées de localisation, aucune allocation ──────────
    let mut locs: Vec<(usize, usize, i32, i32)> = Vec::with_capacity(1024);
    for i in 0..1024usize {
        let loc = u32::from_be_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        let off = (loc >> 8) as usize;
        let cnt = (loc & 0xff) as usize;
        if off == 0 || cnt == 0 {
            continue;
        }
        locs.push((off * SECTOR, cnt * SECTOR, (i % 32) as i32, (i / 32) as i32));
    }

    // ── 1. DÉCODAGE ────────────────────────────────────────────────────────
    let t0 = Instant::now();
    let per_chunk: Vec<(i32, i32, Vec<RawSection>)> = locs
        .par_iter()
        .filter_map(|&(off, _cap, cx, cz)| {
            let len = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
            let comp = buf[off + 4];
            let payload = &buf[off + 5..off + 4 + len];
            let inflated = match comp {
                1 => {
                    let mut d = Vec::new();
                    use std::io::Read;
                    flate2::read::GzDecoder::new(payload).read_to_end(&mut d).ok()?;
                    d
                }
                2 => {
                    let mut d = Vec::new();
                    use std::io::Read;
                    flate2::read::ZlibDecoder::new(payload).read_to_end(&mut d).ok()?;
                    d
                }
                _ => payload.to_vec(),
            };
            parse_chunk(&inflated).ok().map(|s| (cx, cz, s))
        })
        .collect();
    let decode_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // ── interning : les chaînes deviennent des u32 ─────────────────────────
    let t1 = Instant::now();
    let mut interner: HashMap<String, u32> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    let mut world: Vec<i32> = vec![-1; 32 * 32 * 24];
    for (cx, cz, raws) in per_chunk {
        for r in raws {
            let pal: Vec<u32> = r
                .palette
                .into_iter()
                .map(|s| {
                    *interner.entry(s.clone()).or_insert_with(|| {
                        names.push(s);
                        (names.len() - 1) as u32
                    })
                })
                .collect();
            let yi = (r.y as i32) + 4;
            if (0..24).contains(&yi) {
                world[(yi as usize) * 1024 + (cz as usize) * 32 + (cx as usize)] = sections.len() as i32;
            }
            sections.push(Section { y: r.y, palette: pal, bits: r.bits, data: r.data, unpacked: None });
        }
    }
    let intern_ms = t1.elapsed().as_secs_f64() * 1000.0;

    let packed: usize = sections.iter().map(|s| s.packed_bytes()).sum();
    let blocks = sections.len() * VOL;
    println!("── 1 · décodage ──────────────────────────────────────────────");
    println!("  NBT ciblé + inflate (rayon)   {:>8.0} ms", decode_ms);
    println!("  interning des palettes        {:>8.0} ms", intern_ms);
    println!("  TOTAL                         {:>8.0} ms   ({} sections, {} blocs)", decode_ms + intern_ms, sections.len(), blocks);
    println!("  états distincts dans la région        {}", names.len());
    println!();
    println!("── 2 · empreinte ─────────────────────────────────────────────");
    println!("  structure packée              {:>8.1} Mo   = {:.2} o/bloc", packed as f64 / 1_048_576.0, packed as f64 / blocks as f64);
    println!("  RSS du processus              {:>8.1} Mo", rss_mb());
    println!();

    let stone = *interner.get("minecraft:stone").expect("pierre absente");
    let dirt = *interner.get("minecraft:dirt").expect("terre absente");

    // ── 3. //replace — trois stratégies sur les MÊMES données ──────────────
    println!("── 3 · //replace minecraft:stone → minecraft:dirt ────────────");

    // (a) NAÏF : boucle en coordonnées MONDE, un fil. Structure identique au
    //     chemin actuel de we-engine — c'est la comparaison honnête « même
    //     algorithme, autre langage ».
    {
        let mut secs = sections.clone();
        for s in secs.iter_mut() {
            s.ensure_unpacked();
        }
        let t = Instant::now();
        let mut changed = 0usize;
        for y in -64i32..320 {
            for z in 0i32..512 {
                for x in 0i32..512 {
                    let si = world[(((y >> 4) + 4) as usize) * 1024 + ((z >> 4) as usize) * 32 + ((x >> 4) as usize)];
                    if si < 0 {
                        continue;
                    }
                    let s = &mut secs[si as usize];
                    let li = (((y & 15) as usize) << 8) | (((z & 15) as usize) << 4) | ((x & 15) as usize);
                    let u = s.unpacked.as_mut().unwrap();
                    if s.palette[u[li] as usize] != stone {
                        continue;
                    }
                    let d = match s.palette.iter().position(|&e| e == dirt) {
                        Some(i) => i as u16,
                        None => {
                            s.palette.push(dirt);
                            (s.palette.len() - 1) as u16
                        }
                    };
                    s.unpacked.as_mut().unwrap()[li] = d;
                    changed += 1;
                }
            }
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("  (a) par bloc, coords monde, 1 fil   {:>8.1} ms   {} blocs", ms, changed);
    }

    // (b) PAR SECTION, en parallèle : on ne visite plus les sections absentes
    //     et chaque section est indépendante.
    {
        let mut secs = sections.clone();
        let t = Instant::now();
        let changed: usize = secs
            .par_iter_mut()
            .map(|s| {
                if !s.palette.iter().any(|&e| e == stone) {
                    return 0;
                }
                let mut idx = s.unpack();
                let si = s.palette.iter().position(|&e| e == stone).unwrap() as u16;
                let di = match s.palette.iter().position(|&e| e == dirt) {
                    Some(i) => i as u16,
                    None => {
                        s.palette.push(dirt);
                        (s.palette.len() - 1) as u16
                    }
                };
                let mut n = 0usize;
                for v in idx.iter_mut() {
                    if *v == si {
                        *v = di;
                        n += 1;
                    }
                }
                s.repack(&idx);
                n
            })
            .sum();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("  (b) par bloc, par section, rayon    {:>8.1} ms   {} blocs", ms, changed);
    }

    // (c) PAR PALETTE, cible DÉJÀ PRÉSENTE dans la palette. Sur du vrai
    //     terrain, une section qui contient de la pierre contient souvent de la
    //     terre : la substitution crée un doublon, donc il FAUT remapper les
    //     indices. C'est le pire cas de l'étage palette, et il faut le mesurer
    //     tel quel plutôt que d'annoncer le meilleur.
    {
        let mut fast = 0usize;
        let mut slow = 0usize;
        let ms = median(5, || {
            let mut secs = sections.clone();
            let t = Instant::now();
            let (f, sl) = secs
                .par_iter_mut()
                .map(|s| match s.replace_by_palette(stone, dirt) {
                    None => (0usize, 0usize),
                    Some(false) => (1, 0),
                    Some(true) => (0, 1),
                })
                .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            fast = f;
            slow = sl;
            t.elapsed().as_secs_f64() * 1000.0
        });
        println!("  (c) par palette — cible PRÉSENTE   {:>8.1} ms   {} sections rapides, {} remappées", ms, fast, slow);
    }

    // (d) PAR PALETTE, cible ABSENTE de la palette — le cas où l'étage donne
    //     tout ce qu'il promet : on réécrit une entrée et on ne touche AUCUN
    //     indice de bloc. C'est le cas d'un remplacement vers un bloc moddé,
    //     vers un état précis, ou vers n'importe quel bloc que la section
    //     n'utilise pas encore, ce qui est le cas courant.
    {
        let absent = names.len() as u32; // identifiant neuf, garanti absent
        let mut fast = 0usize;
        let mut slow = 0usize;
        let ms = median(5, || {
            let mut secs = sections.clone();
            let t = Instant::now();
            let (f, sl) = secs
                .par_iter_mut()
                .map(|s| match s.replace_by_palette(stone, absent) {
                    None => (0usize, 0usize),
                    Some(false) => (1, 0),
                    Some(true) => (0, 1),
                })
                .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            fast = f;
            slow = sl;
            t.elapsed().as_secs_f64() * 1000.0
        });
        println!("  (d) par palette — cible ABSENTE    {:>8.1} ms   {} sections rapides, {} remappées", ms, fast, slow);
    }

    // (f) PAR PALETTE SANS DÉDOUBLONNAGE — l'hypothèse à vérifier : si on
    //     accepte deux entrées de palette identiques, le chemin rapide
    //     s'applique TOUJOURS, y compris quand la cible est déjà présente.
    {
        let mut touched = 0usize;
        let ms = median(5, || {
            let mut secs = sections.clone();
            let t = Instant::now();
            touched = secs
                .par_iter_mut()
                .map(|s| s.replace_by_palette_nodedupe(stone, dirt) as usize)
                .sum();
            t.elapsed().as_secs_f64() * 1000.0
        });
        println!("  (f) par palette SANS dédoublonnage {:>8.2} ms   {} sections", ms, touched);
    }

    // ── vérification : les quatre stratégies doivent produire le MÊME monde.
    //    Sans ça, on mesure la vitesse d'un résultat faux.
    println!();
    println!("── 5 · vérification de correction ────────────────────────────");
    {
        let before = sections.par_iter().map(|s| s.count_of(dirt)).sum::<usize>();
        let mut b = sections.clone();
        b.par_iter_mut().for_each(|s| {
            if !s.palette.iter().any(|&e| e == stone) {
                return;
            }
            let mut idx = s.unpack();
            let si = s.palette.iter().position(|&e| e == stone).unwrap() as u16;
            let di = match s.palette.iter().position(|&e| e == dirt) {
                Some(i) => i as u16,
                None => {
                    s.palette.push(dirt);
                    (s.palette.len() - 1) as u16
                }
            };
            for v in idx.iter_mut() {
                if *v == si {
                    *v = di;
                }
            }
            s.repack(&idx);
        });
        let mut c = sections.clone();
        c.par_iter_mut().for_each(|s| {
            s.replace_by_palette(stone, dirt);
        });
        let mut f = sections.clone();
        f.par_iter_mut().for_each(|s| {
            s.replace_by_palette_nodedupe(stone, dirt);
        });
        let nb = b.par_iter().map(|s| s.count_of(dirt)).sum::<usize>();
        let nc = c.par_iter().map(|s| s.count_of(dirt)).sum::<usize>();
        let nf = f.par_iter().map(|s| s.count_of(dirt)).sum::<usize>();
        let stone_left = f.par_iter().map(|s| s.count_of(stone)).sum::<usize>();
        println!("  terre avant l'opération                {}", before);
        println!("  terre après (b) par bloc               {}", nb);
        println!("  terre après (c) palette dédoublonnée   {}", nc);
        println!("  terre après (f) palette sans dédoubl.  {}", nf);
        println!("  pierre restante après (f)              {}", stone_left);
        println!(
            "  → {}",
            if nb == nc && nc == nf && stone_left == 0 {
                "IDENTIQUES — les trois stratégies produisent le même monde"
            } else {
                "DIVERGENCE — une stratégie est fausse"
            }
        );
    }

    // (e) ÉTAGE SECTION : //set sur une sélection qui couvre la région entière.
    println!();
    println!("── 4 · //set minecraft:stone sur toute la région ─────────────");
    {
        let ms = median(5, || {
            let mut secs = sections.clone();
            let t = Instant::now();
            secs.par_iter_mut().for_each(|s| s.set_uniform(stone));
            t.elapsed().as_secs_f64() * 1000.0
        });
        println!("  (e) par section, rayon             {:>8.2} ms   {} sections", ms, sections.len());
    }
}

/// Médiane de N — la leçon du bench de we-engine : une mesure unique est du
/// bruit, `mirror-rotate` a bougé de 18 % à code identique.
fn median(n: usize, mut f: impl FnMut() -> f64) -> f64 {
    let mut v: Vec<f64> = (0..n).map(|_| f()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[n / 2]
}
