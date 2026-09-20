//! Repérage ciblé dans un chunk inflaté, et réécriture **par splice**.
//!
//! Le round-trip lossless de `we-engine` repose sur une promesse : un chunk
//! non modifié est réémis octet pour octet, parce qu'on garde sa charge
//! compressée brute. Mais dès qu'un chunk est modifié, il ré-encode tout son
//! arbre NBT — donc tout ce que le parseur a mal compris est perdu.
//!
//! Ici, même un chunk MODIFIÉ ne se ré-encode pas. On repère la plage d'octets
//! des champs de blocs, on remplace celles des sections qu'on a touchées, et
//! tout le reste du chunk est recopié tel quel. Heightmaps, structures, block
//! entities, données de mods inconnues : le lecteur n'y touche pas, **donc il
//! ne peut pas les abîmer**. C'est une propriété structurelle, pas une
//! précaution.
//!
//! Deux dispositions cohabitent dans les saves réelles, et elles sont
//! détectées sur la STRUCTURE, jamais sur le `DataVersion` — voir `format.rs`.

use tf_nbt::{tag, Cur, Span, Trunc, R};

use crate::entites::{balayer_liste, champ_entites, nom_du_champ, Entite, ListeEntites};
use crate::format::{detect_packing, packing_de_repli, Layout, Packing};
use crate::section::{bits_for, Section, MAX_PALETTE, VOL};
use crate::state::{split_key, state_key, Interner, StateId};

/// Ce qu'un balayage a repéré, sans rien matérialiser.
#[derive(Debug, Clone)]
pub struct ChunkScan {
    pub data_version: i32,
    pub layout: Layout,
    /// Coordonnées MONDE du chunk, telles qu'écrites dans son contenu.
    ///
    /// Un nom de fichier est une métadonnée qui peut mentir — Windows renomme
    /// un téléchargement en double `r.0.0 (16).mca`. Le contenu, lui, ne ment
    /// pas.
    pub x_pos: Option<i32>,
    pub z_pos: Option<i32>,
    pub sections: Vec<ScannedSection>,
    /// Les block entities du chunk — le contenu des coffres, le texte des
    /// panneaux. Elles ne sont PAS dans la grille de blocs et ne suivent donc
    /// aucune opération toutes seules : voir `entites.rs`.
    pub entites: ListeEntites,
    /// Packing du CHUNK, déduit de la première section qui porte des indices.
    ///
    /// C'est une propriété du chunk et non de la section : une section
    /// homogène n'a aucun tableau d'indices, donc rien à mesurer. Lui donner
    /// le packing moderne par défaut écrirait du 1.16+ dans un fichier 1.15 le
    /// jour où elle cesse d'être homogène — une corruption silencieuse, et
    /// dans le seul cas que personne ne pense à tester.
    pub packing: Packing,
}

impl Default for ChunkScan {
    fn default() -> Self {
        ChunkScan {
            data_version: 0,
            layout: Layout::Flat,
            x_pos: None,
            z_pos: None,
            sections: Vec::new(),
            entites: ListeEntites::default(),
            packing: Packing::NoStraddle,
        }
    }
}

/// Où vivent les octets de blocs d'une section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionSpans {
    /// 1.18+ : `palette` et `data` sont deux champs d'un compound
    /// `block_states`.
    ///
    /// Les deux sont relevés SÉPARÉMENT, et ce n'est pas du détail : un
    /// `//replace` ne change que des noms. Réécrire le compound entier
    /// traînerait les 4096 indices derrière chaque changement de palette —
    /// mesuré sur un chunk réel, 21 922 octets réécrits au lieu de 1 536, et
    /// autant à stocker dans le journal d'annulation.
    ///
    /// `data` est la plage du CHAMP entier — type, nom et charge — pour qu'une
    /// section qui redevient homogène puisse le faire disparaître : 1.18+
    /// n'écrit pas de `data` pour une palette d'une entrée, et en laisser un
    /// de la mauvaise longueur casse le chargement du chunk.
    Flat {
        palette: Span,
        data: Option<Span>,
        insert_at: usize,
    },
    /// 1.13 – 1.17 : deux champs FRÈRES du compound de section. `blocks` est
    /// absent quand la section est homogène — il faut alors l'insérer, d'où
    /// `insert_at`, qui pointe sur le `TAG_End` de la section.
    Legacy {
        palette: Span,
        blocks: Option<Span>,
        insert_at: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScannedSection {
    pub y: i8,
    /// `None` pour une section sans champs de blocs : elles existent (sections
    /// purement d'éclairage aux bords du monde) et il ne faut ni les décoder
    /// ni les réécrire.
    pub spans: Option<SectionSpans>,
    /// Nombre d'entrées de palette, relevé au vol pendant le balayage.
    pub palette_len: usize,
    /// Nombre de longs d'indices, relevé au vol. Zéro pour une section
    /// homogène.
    pub data_len: usize,
}

/// Balaye un chunk inflaté. Ne descend que dans les champs de blocs ; tout le
/// reste est enjambé sans être matérialisé.
pub fn scan(inflated: &[u8]) -> R<ChunkScan> {
    let mut c = Cur::new(inflated);
    c.enter_root()?;
    let mut out = ChunkScan::default();
    let mut level: Option<Span> = None;
    let racine_fin;

    loop {
        // La position AVANT l'en-tête : c'est le début du CHAMP, et si c'est
        // le `TAG_End` de la racine, c'est là qu'il faudra insérer une liste
        // de block entities absente.
        let avant = c.pos();
        let Some((t, key)) = c.next_field()? else {
            racine_fin = avant;
            break;
        };
        match (t, key) {
            (tag::INT, "DataVersion") => out.data_version = c.i32()?,
            (tag::INT, "xPos") => out.x_pos = Some(c.i32()?),
            (tag::INT, "zPos") => out.z_pos = Some(c.i32()?),
            (tag::LIST, "sections") => {
                out.layout = Layout::Flat;
                out.sections = scan_section_list(&mut c, Layout::Flat)?;
            }
            (tag::LIST, "block_entities") => {
                out.entites.entrees = balayer_liste(&mut c)?;
                out.entites.champ = Some(Span {
                    start: avant,
                    end: c.pos(),
                });
            }
            // 1.13 – 1.17 : tout est sous `Level`. On note sa plage et on la
            // traite APRÈS, parce que `sections` à la racine doit gagner si les
            // deux existent (un monde converti peut porter les deux le temps
            // d'une migration).
            (tag::COMPOUND, "Level") => level = Some(c.span_of_payload(t)?),
            _ => c.skip_payload(t)?,
        }
    }
    out.entites.inserer_a = racine_fin;

    if out.sections.is_empty() {
        if let Some(span) = level {
            out.layout = Layout::Legacy;
            let mut lc = Cur::at(inflated, span.start);
            // La liste de 1.13–1.17 vit sous `Level` : c'est aussi là qu'il
            // faut l'insérer si elle manque, et pas à la racine.
            out.entites = ListeEntites::default();
            loop {
                let avant = lc.pos();
                let Some((t, key)) = lc.next_field()? else {
                    out.entites.inserer_a = avant;
                    break;
                };
                match (t, key) {
                    (tag::INT, "xPos") => out.x_pos = Some(lc.i32()?),
                    (tag::INT, "zPos") => out.z_pos = Some(lc.i32()?),
                    (tag::LIST, "Sections") => {
                        out.sections = scan_section_list(&mut lc, Layout::Legacy)?;
                    }
                    (tag::LIST, "TileEntities") => {
                        out.entites.entrees = balayer_liste(&mut lc)?;
                        out.entites.champ = Some(Span {
                            start: avant,
                            end: lc.pos(),
                        });
                    }
                    _ => lc.skip_payload(t)?,
                }
            }
        }
    }
    out.packing = deduce_packing(&out.sections, out.data_version);
    Ok(out)
}

/// Packing du chunk : la première section qui porte assez d'indices pour
/// trancher l'emporte, et les sections muettes (homogènes) en héritent.
///
/// Toutes les sections d'un chunk viennent de la même version du jeu — c'est
/// le jeu qui réécrit un chunk entier ou pas du tout. Une seule mesure suffit
/// donc, et c'est plus sûr que de croire le `DataVersion`.
///
/// Le `DataVersion` ne sert qu'au cas où AUCUNE section ne porte d'indices :
/// un chunk entièrement homogène ne contient aucune preuve de son packing.
fn deduce_packing(sections: &[ScannedSection], data_version: i32) -> Packing {
    for s in sections {
        if s.data_len == 0 || s.palette_len <= 1 {
            continue;
        }
        let bits = bits_for(s.palette_len) as usize;
        // On ignore les largeurs ambiguës (4 et 8 bits) : elles ne tranchent
        // rien, puisque les deux dispositions y produisent les mêmes octets.
        if 64 % bits == 0 {
            continue;
        }
        if let Some(p) = detect_packing(VOL, bits, s.data_len) {
            return p;
        }
    }
    // Aucune section ne porte d'indices : il n'y a rien à mesurer. C'est le
    // seul endroit du crate où le DataVersion décide de quelque chose, et sa
    // portée est bornée — voir `packing_de_repli`.
    packing_de_repli(data_version)
}

fn scan_section_list(c: &mut Cur, layout: Layout) -> R<Vec<ScannedSection>> {
    let (et, n) = c.list_header()?;
    if et == tag::END {
        return Ok(Vec::new()); // liste vide
    }
    if et != tag::COMPOUND {
        return Err(Trunc);
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(match layout {
            Layout::Flat => scan_flat_section(c)?,
            Layout::Legacy => scan_legacy_section(c)?,
        });
    }
    Ok(out)
}

fn scan_flat_section(c: &mut Cur) -> R<ScannedSection> {
    let mut y: i8 = 0;
    let mut spans = None;
    let mut palette_len = 0;
    let mut data_len = 0;
    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::BYTE, "Y") => y = c.i8()?,
            (tag::COMPOUND, "block_states") => {
                let bloc = c.span_of_payload(t)?;
                // Relecture des deux SEULS en-têtes, sans rien matérialiser :
                // le curseur est déjà sorti du compound, on y repasse au vol.
                let mut p = Cur::at(c.buf(), bloc.start);
                let mut palette = None;
                let mut data = None;
                let fin;
                loop {
                    // La position AVANT l'en-tête : si c'est le `TAG_End` du
                    // compound, c'est là qu'il faudra insérer un `data` absent.
                    let avant = p.pos();
                    let Some((ft, fk)) = p.next_field()? else {
                        fin = avant;
                        break;
                    };
                    match (ft, fk) {
                        (tag::LIST, "palette") => {
                            let debut = p.pos();
                            let (_, n) = p.list_header()?;
                            palette_len = n;
                            p.skip_list_body(tag::COMPOUND, n)?;
                            palette = Some(Span {
                                start: debut,
                                end: p.pos(),
                            });
                        }
                        (tag::LONG_ARRAY, "data") => {
                            data_len = p.array_len()?;
                            p.skip(data_len * 8)?;
                            // Le CHAMP, pas la charge : `avant` pointe sur
                            // l'octet de type.
                            data = Some(Span {
                                start: avant,
                                end: p.pos(),
                            });
                        }
                        _ => p.skip_payload(ft)?,
                    }
                }
                // Un `block_states` sans `palette` n'est pas une section de
                // blocs : ne rien réécrire vaut mieux qu'écrire à l'aveugle.
                spans = palette.map(|palette| SectionSpans::Flat {
                    palette,
                    data,
                    insert_at: fin,
                });
            }
            _ => c.skip_payload(t)?,
        }
    }
    Ok(ScannedSection {
        y,
        spans,
        palette_len,
        data_len,
    })
}

fn scan_legacy_section(c: &mut Cur) -> R<ScannedSection> {
    let mut y: i8 = 0;
    let mut palette = None;
    let mut blocks = None;
    let mut palette_len = 0;
    let mut data_len = 0;
    let end_pos;
    loop {
        // La position AVANT de lire le prochain en-tête : si c'est le `TAG_End`
        // de la section, c'est là qu'il faudra insérer un `BlockStates` absent.
        let before = c.pos();
        let Some((t, key)) = c.next_field()? else {
            end_pos = before;
            break;
        };
        match (t, key) {
            (tag::BYTE, "Y") => y = c.i8()?,
            (tag::LIST, "Palette") => {
                let start = c.pos();
                let (et, n) = c.list_header()?;
                palette_len = n;
                c.skip_list_body(et, n)?;
                palette = Some(Span {
                    start,
                    end: c.pos(),
                });
            }
            (tag::LONG_ARRAY, "BlockStates") => {
                let start = c.pos();
                data_len = c.array_len()?;
                c.skip(data_len * 8)?;
                blocks = Some(Span {
                    start,
                    end: c.pos(),
                });
            }
            _ => c.skip_payload(t)?,
        }
    }
    Ok(ScannedSection {
        y,
        spans: palette.map(|palette| SectionSpans::Legacy {
            palette,
            blocks,
            insert_at: end_pos,
        }),
        palette_len,
        data_len,
    })
}

/// Matérialise une section repérée. C'est le seul endroit qui alloue.
pub fn decode_section(
    inflated: &[u8],
    chunk: &ChunkScan,
    scanned: &ScannedSection,
    interner: &mut Interner,
) -> R<Option<Section>> {
    let Some(spans) = scanned.spans else {
        return Ok(None);
    };
    // Les deux dispositions se lisent pareil : la palette est une liste de
    // compounds, les indices un tableau de longs. Seul l'emplacement change, et
    // le balayage l'a déjà relevé.
    let (palette, data) = match spans {
        SectionSpans::Flat { palette, data, .. } => {
            let pal = read_palette_list(&mut Cur::at(inflated, palette.start), interner)?;
            // `data` est le CHAMP : on repasse son en-tête pour atteindre la
            // charge.
            let idx = match data {
                Some(f) => {
                    let mut c = Cur::at(inflated, f.start);
                    c.next_field()?.ok_or(Trunc)?;
                    c.long_array()?
                }
                None => Vec::new(),
            };
            (pal, idx)
        }
        SectionSpans::Legacy {
            palette, blocks, ..
        } => {
            let pal = read_palette_list(&mut Cur::at(inflated, palette.start), interner)?;
            let idx = match blocks {
                Some(b) => Cur::at(inflated, b.start).long_array()?,
                None => Vec::new(),
            };
            (pal, idx)
        }
    };

    if palette.is_empty() {
        return Ok(None); // section sans palette : rien à éditer
    }
    let bits = bits_for(palette.len());
    // Une palette d'une entrée n'a pas de `data`, et un `data` présent avec une
    // palette d'une entrée est du bruit qu'on ignore plutôt que de le propager.
    let (data, packing) = if palette.len() <= 1 || data.is_empty() {
        // Aucun indice à mesurer : la section hérite du packing de son CHUNK.
        // Lui donner le moderne par défaut écrirait du 1.16+ dans un fichier
        // 1.15 le jour où elle cesse d'être homogène.
        (Vec::new(), chunk.packing)
    } else {
        // Le packing se DÉDUIT de la longueur du tableau, jamais du
        // DataVersion. Une longueur qui ne correspond à aucune des deux formes
        // est un tableau tronqué : on refuse plutôt que de deviner, parce que
        // deviner écrirait des blocs faux dans une save.
        let Some(p) = detect_packing(VOL, bits as usize, data.len()) else {
            return Err(Trunc);
        };
        (data, p)
    };

    Ok(Some(Section {
        y: scanned.y,
        palette,
        bits,
        data: data.into_boxed_slice(),
        packing,
    }))
}

/// Lit une liste de compounds de palette, le curseur étant sur son EN-TÊTE.
fn read_palette_list(c: &mut Cur, interner: &mut Interner) -> R<Vec<StateId>> {
    let (et, n) = c.list_header()?;
    if et == tag::END {
        return Ok(Vec::new());
    }
    if et != tag::COMPOUND {
        return Err(Trunc);
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read_palette_entry(c, interner)?);
    }
    Ok(out)
}

fn read_palette_entry(c: &mut Cur, interner: &mut Interner) -> R<StateId> {
    let mut name = "";
    let mut props: Vec<(String, String)> = Vec::new();
    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::STRING, "Name") => name = c.str()?,
            (tag::COMPOUND, "Properties") => {
                while let Some((pt, pk)) = c.next_field()? {
                    if pt == tag::STRING {
                        props.push((pk.to_string(), c.str()?.to_string()));
                    } else {
                        c.skip_payload(pt)?;
                    }
                }
            }
            _ => c.skip_payload(t)?,
        }
    }
    if name.is_empty() {
        return Err(Trunc);
    }
    Ok(interner.intern(&state_key(name, &mut props)))
}

/// Remplacement d'une plage d'octets du chunk inflaté.
///
/// Une plage de longueur nulle est une INSERTION à cette position — c'est ce
/// qui permet d'ajouter un `BlockStates` à une section 1.13–1.17 qui était
/// homogène et ne devient plus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub span: Span,
    pub bytes: Vec<u8>,
}

/// Pourquoi une section refuse d'être écrite.
///
/// Deux causes distinctes, et les confondre dans un `None` les rendait
/// indiscernables — alors qu'elles demandent des corrections opposées.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// Un état de la palette ne se résout pas dans l'interner fourni. C'est un
    /// mélange d'interners : les identifiants d'une table ne veulent rien dire
    /// dans une autre, et les écrire poserait les mauvais blocs.
    UnknownState(StateId),
    /// La palette dépasse ce qu'une section peut porter. Un `repack` compacte
    /// automatiquement ; y arriver signifie qu'on écrit une section qui n'a
    /// jamais été repackée depuis qu'on a grossi sa palette.
    PaletteTooLarge(usize),
    /// La section n'a aucun champ de blocs à remplacer (section d'éclairage).
    NoBlockFields,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::UnknownState(id) => write!(
                f,
                "l'état {id} est introuvable dans l'interner fourni : deux tables ont été mélangées"
            ),
            EncodeError::PaletteTooLarge(n) => write!(
                f,
                "palette de {n} entrées, maximum {MAX_PALETTE} : la section n'a pas été repackée"
            ),
            EncodeError::NoBlockFields => write!(f, "section sans champ de blocs"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Compose les éditions qui réécrivent une section, dans sa disposition
/// d'origine.
///
/// Une édition dont les octets sont DÉJÀ ceux du chunk n'est pas produite. Ce
/// n'est pas une micro-optimisation : c'est ce qui fait qu'une opération de
/// palette ne traîne pas les 4 096 indices derrière elle, et qu'une opération
/// qui ne change rien ne salit pas le chunk — donc ne remplit pas le journal
/// d'annulation d'entrées vides. La comparaison porte sur les OCTETS, pas sur
/// un drapeau : un drapeau finit par mentir.
pub fn section_edits(
    inflated: &[u8],
    section: &Section,
    scanned: &ScannedSection,
    interner: &Interner,
) -> Result<Vec<Edit>, EncodeError> {
    let spans = scanned.spans.ok_or(EncodeError::NoBlockFields)?;
    if section.palette.len() > MAX_PALETTE {
        return Err(EncodeError::PaletteTooLarge(section.palette.len()));
    }
    let entries = palette_entries(section, interner)?;
    let refs: Vec<tf_nbt::PaletteEntryRef> = entries
        .iter()
        .map(|(n, p)| tf_nbt::PaletteEntryRef { name: n, props: p })
        .collect();

    let mut out = match spans {
        SectionSpans::Flat {
            palette,
            data,
            insert_at,
        } => {
            let mut out = vec![Edit {
                span: palette,
                bytes: tf_nbt::palette_list_payload(&refs),
            }];
            // 1.18+ n'écrit pas de `data` pour une palette d'une entrée.
            let voulu = if section.palette.len() > 1 && !section.data.is_empty() {
                Some(tf_nbt::named_long_array("data", &section.data))
            } else {
                None
            };
            match (data, voulu) {
                (Some(span), Some(bytes)) => out.push(Edit { span, bytes }),
                // La section redevient homogène : le champ DISPARAÎT. En
                // laisser un de la mauvaise longueur casse le chargement.
                (Some(span), None) => out.push(Edit {
                    span,
                    bytes: Vec::new(),
                }),
                (None, Some(bytes)) => out.push(Edit {
                    span: Span {
                        start: insert_at,
                        end: insert_at,
                    },
                    bytes,
                }),
                (None, None) => {}
            }
            out
        }
        SectionSpans::Legacy {
            palette,
            blocks,
            insert_at,
        } => {
            let mut out = vec![Edit {
                span: palette,
                bytes: tf_nbt::palette_list_payload(&refs),
            }];
            match blocks {
                Some(span) => out.push(Edit {
                    span,
                    bytes: tf_nbt::long_array_payload(&section.data),
                }),
                // La section n'avait pas de `BlockStates` et en a besoin :
                // on insère le champ ENTIER (type + nom + charge) juste avant
                // le `TAG_End` de la section.
                None if !section.data.is_empty() => out.push(Edit {
                    span: Span {
                        start: insert_at,
                        end: insert_at,
                    },
                    bytes: tf_nbt::named_long_array("BlockStates", &section.data),
                }),
                None => {}
            }
            out
        }
    };
    // Chaque édition est RESSERRÉE sur ce qui change vraiment, et celles qui
    // ne changent rien disparaissent.
    out.retain_mut(|e| trim_edit(inflated, e));
    Ok(out)
}

/// Resserre une édition sur la partie qui diffère réellement. Rend `false` si
/// elle ne change rien.
///
/// C'est du diff d'octets, sans une ligne de connaissance NBT — et c'est ce qui
/// le rend juste partout. Un `//replace pierre terre` ne change qu'un nom au
/// milieu d'une liste de palette : le préfixe commun s'arrête à ce nom, et le
/// SUFFIXE commun se compare depuis la fin des deux tampons, donc il tient même
/// si le nouveau nom est plus court et décale tout ce qui suit. Mesuré sur un
/// chunk réel à grosse palette : 11 840 octets réécrits sans, 180 avec.
///
/// L'enjeu n'est pas le splice, qui recopie de toute façon : c'est le JOURNAL.
/// Il stocke les octets d'avant ET d'après ; les resserrer divise sa taille par
/// le même facteur, et une annulation se paie alors pour ce qu'une opération a
/// écrit, pas pour ce qu'elle a survolé.
fn trim_edit(inflated: &[u8], e: &mut Edit) -> bool {
    let ancien = &inflated[e.span.start..e.span.end];
    if ancien == e.bytes.as_slice() {
        return false;
    }
    let plafond = ancien.len().min(e.bytes.len());

    let mut p = 0;
    while p < plafond && ancien[p] == e.bytes[p] {
        p += 1;
    }
    let mut s = 0;
    while s < plafond - p && ancien[ancien.len() - 1 - s] == e.bytes[e.bytes.len() - 1 - s] {
        s += 1;
    }

    e.bytes.truncate(e.bytes.len() - s);
    e.bytes.drain(..p);
    e.span = Span {
        start: e.span.start + p,
        end: e.span.end - s,
    };
    true
}

/// L'édition qui remplace la liste de block entities du chunk.
///
/// Rend `None` quand rien ne change — même critère que `section_edits`, et
/// pour la même raison : ce sont les OCTETS qui décident, pas un drapeau. Une
/// opération qui ne déplace aucun coffre ne doit pas salir le chunk, sinon
/// elle remplit le journal d'annulation d'entrées vides.
///
/// **Une liste vide qui le reste ne produit rien**, même si le fichier l'écrit
/// sous une forme que nous n'écririons pas (type d'élément `TAG_Compound` avec
/// une longueur nulle). Normaliser au passage changerait les octets d'un chunk
/// que l'opération n'a pas touché — c'est exactement la faute que le collage a
/// payée sur un vrai monde.
pub fn edition_entites(
    inflated: &[u8],
    liste: &ListeEntites,
    layout: Layout,
    voulues: &[Entite],
) -> Option<Edit> {
    if voulues.is_empty() && liste.est_vide() {
        return None;
    }
    let bytes = champ_entites(nom_du_champ(layout), voulues);
    let mut e = match liste.champ {
        Some(span) => Edit { span, bytes },
        // Le chunk n'en portait pas et en reçoit : on insère le CHAMP entier
        // juste avant le `TAG_End` de son compound hôte.
        None => Edit {
            span: Span {
                start: liste.inserer_a,
                end: liste.inserer_a,
            },
            bytes,
        },
    };
    trim_edit(inflated, &mut e).then_some(e)
}

/// Recompose la charge d'un `block_states` (1.18+) depuis une section.
///
/// Conservé pour l'usage direct ; `section_edits` est le chemin qui gère les
/// deux dispositions.
pub fn encode_section(section: &Section, interner: &Interner) -> Result<Vec<u8>, EncodeError> {
    if section.palette.len() > MAX_PALETTE {
        return Err(EncodeError::PaletteTooLarge(section.palette.len()));
    }
    let entries = palette_entries(section, interner)?;
    let refs: Vec<tf_nbt::PaletteEntryRef> = entries
        .iter()
        .map(|(n, p)| tf_nbt::PaletteEntryRef { name: n, props: p })
        .collect();
    Ok(tf_nbt::block_states_payload(&refs, &section.data))
}

type Entries = Vec<(String, Vec<(String, String)>)>;

fn palette_entries(section: &Section, interner: &Interner) -> Result<Entries, EncodeError> {
    let mut out = Vec::with_capacity(section.palette.len());
    for &id in &section.palette {
        let key = interner.resolve(id).ok_or(EncodeError::UnknownState(id))?;
        let (name, props) = split_key(key);
        out.push((name.to_string(), props));
    }
    Ok(out)
}

/// Remplace des plages d'octets et recopie tout le reste **tel quel**.
///
/// Les plages doivent être disjointes. Deux plages qui se chevauchent sont un
/// bug d'appelant, pas une entrée à assainir : on refuse plutôt que d'écrire
/// une sortie plausible mais fausse dans la save de quelqu'un.
pub fn splice(inflated: &[u8], edits: &mut [Edit]) -> Result<Vec<u8>, SpliceError> {
    if edits.is_empty() {
        return Ok(inflated.to_vec());
    }
    // Trier sur le SEUL début rendait le résultat dépendant de l'ordre du
    // vecteur : une insertion en `p` et un remplacement commençant en `p`
    // passaient ou rendaient `Overlap` selon lequel arrivait en premier.
    // Deux appels au même ensemble d'éditions doivent faire la même chose.
    // Les insertions (longueur nulle) viennent avant : elles s'écrivent
    // AVANT le texte remplacé, ce qui est la lecture naturelle.
    edits.sort_by_key(|e| (e.span.start, e.span.len()));

    let mut prev_end = 0usize;
    for e in edits.iter() {
        if e.span.start < prev_end {
            return Err(SpliceError::Overlap);
        }
        if e.span.end > inflated.len() || e.span.start > e.span.end {
            return Err(SpliceError::OutOfBounds);
        }
        prev_end = e.span.end;
    }

    let grown: usize = edits.iter().map(|e| e.bytes.len()).sum();
    let shrunk: usize = edits.iter().map(|e| e.span.len()).sum();
    let mut out = Vec::with_capacity(inflated.len() + grown - shrunk.min(inflated.len()));

    let mut cursor = 0usize;
    for e in edits.iter() {
        out.extend_from_slice(&inflated[cursor..e.span.start]);
        out.extend_from_slice(&e.bytes);
        cursor = e.span.end;
    }
    out.extend_from_slice(&inflated[cursor..]);
    Ok(out)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SpliceError {
    /// Deux plages se chevauchent.
    Overlap,
    /// Une plage sort du tampon.
    OutOfBounds,
}

impl std::fmt::Display for SpliceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpliceError::Overlap => write!(f, "deux plages de réécriture se chevauchent"),
            SpliceError::OutOfBounds => write!(f, "une plage de réécriture sort du chunk"),
        }
    }
}

impl std::error::Error for SpliceError {}

/// Les éditions qui ANNULENT `edits`.
///
/// C'est ce qui rend l'annulation proportionnelle à ce qu'une opération a
/// vraiment écrit, et non à ce qu'elle a survolé. Un `//replace pierre terre`
/// sur 100 millions de blocs ne réécrit que des entrées de palette : son
/// annulation pèse quelques centaines d'octets par chunk, pas 26 Mo. Copier
/// le chunk d'avant marcherait aussi et coûterait 260 fois plus.
///
/// `avant` est l'état sur lequel `edits` a été appliqué ; le résultat
/// s'applique au RÉSULTAT de ce splice. Les plages se décalent donc du cumul
/// des changements de longueur qui les précèdent — les oublier viserait le
/// milieu du chunk et le corromprait sans rien signaler.
///
/// `edits` doit avoir été trié par `splice`, qui le fait en place : les passer
/// dans le désordre décrirait un autre résultat. Le tri est donc refait ici,
/// sur une copie, pour que la fonction reste juste quel que soit l'appelant.
pub fn inverse_edits(avant: &[u8], edits: &[Edit]) -> Result<Vec<Edit>, SpliceError> {
    let mut tries: Vec<&Edit> = edits.iter().collect();
    tries.sort_by_key(|e| (e.span.start, e.span.len()));

    let mut prev_end = 0usize;
    for e in tries.iter() {
        if e.span.start < prev_end {
            return Err(SpliceError::Overlap);
        }
        if e.span.end > avant.len() || e.span.start > e.span.end {
            return Err(SpliceError::OutOfBounds);
        }
        prev_end = e.span.end;
    }

    let mut out = Vec::with_capacity(tries.len());
    let mut decalage: isize = 0;
    for e in tries {
        let debut = (e.span.start as isize + decalage) as usize;
        out.push(Edit {
            span: Span {
                start: debut,
                end: debut + e.bytes.len(),
            },
            bytes: avant[e.span.start..e.span.end].to_vec(),
        });
        decalage += e.bytes.len() as isize - e.span.len() as isize;
    }
    Ok(out)
}
