//! Écriture des seuls tags qu'Anvil demande.
//!
//! Volontairement minuscule : on ne réécrit JAMAIS un chunk entier. Seule la
//! charge d'un `block_states` modifié est reconstruite, puis splicée dans le
//! chunk inflaté d'origine. Un écrivain NBT complet serait à la fois inutile
//! et dangereux — il donnerait la possibilité de ré-émettre des champs qu'on
//! ne comprend pas, donc de les abîmer.

use crate::tag;

#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    pub fn with_capacity(n: usize) -> Self {
        Writer {
            buf: Vec::with_capacity(n),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// En-tête d'un champ nommé : type puis nom.
    pub fn field(&mut self, t: u8, name: &str) -> &mut Self {
        self.buf.push(t);
        self.raw_str(name);
        self
    }

    /// Ferme un compound.
    pub fn end(&mut self) -> &mut Self {
        self.buf.push(tag::END);
        self
    }

    /// Chaîne nue : longueur `u16` BE puis les octets. Une chaîne de plus de
    /// 65 535 octets est tronquée par le format lui-même ; on refuse plutôt
    /// que d'écrire une longueur fausse, parce qu'un nom de bloc tronqué
    /// donnerait un état valide mais FAUX — le pire des deux mondes.
    pub fn raw_str(&mut self, s: &str) -> &mut Self {
        let n = u16::try_from(s.len()).expect("chaîne NBT > 65535 octets");
        self.buf.extend_from_slice(&n.to_be_bytes());
        self.buf.extend_from_slice(s.as_bytes());
        self
    }

    pub fn i32_payload(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn i8_payload(&mut self, v: i8) -> &mut Self {
        self.buf.push(v as u8);
        self
    }

    /// En-tête de liste : type d'élément puis longueur `i32` BE.
    pub fn list_header(&mut self, et: u8, n: usize) -> &mut Self {
        self.buf.push(et);
        self.i32_payload(i32::try_from(n).expect("liste NBT > i32::MAX"));
        self
    }

    /// `TAG_Long_Array` : longueur puis les longs en BIG-endian.
    pub fn long_array_payload(&mut self, v: &[u64]) -> &mut Self {
        self.i32_payload(i32::try_from(v.len()).expect("long array > i32::MAX"));
        self.buf.reserve(v.len() * 8);
        for &w in v {
            self.buf.extend_from_slice(&w.to_be_bytes());
        }
        self
    }

    /// Recopie des octets déjà formés — sert à replacer une entrée de palette
    /// dans ses octets d'origine plutôt que de la reconstruire.
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }
}

/// Une entrée de palette, dans la forme où Anvil l'écrit.
///
/// `props` est **trié par clé** à l'écriture. NBT ne donne aucun sens à
/// l'ordre des champs d'un compound, mais deux ordres donnent deux suites
/// d'octets : trier rend l'écriture déterministe, donc les tests comparables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntryRef<'a> {
    pub name: &'a str,
    pub props: &'a [(String, String)],
}

fn write_palette_entry(w: &mut Writer, e: &PaletteEntryRef<'_>) {
    w.field(tag::STRING, "Name").raw_str(e.name);
    if !e.props.is_empty() {
        w.field(tag::COMPOUND, "Properties");
        let mut sorted: Vec<&(String, String)> = e.props.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in sorted {
            w.field(tag::STRING, k).raw_str(v);
        }
        w.end();
    }
    w.end();
}

/// Compose la CHARGE d'un compound `block_states` — sans son octet de type ni
/// son nom, puisqu'on la splice à la place de l'ancienne.
///
/// Palette d'une entrée → **pas de `data`**, exactement comme Minecraft écrit
/// une section homogène. Émettre un tableau d'indices tous nuls serait valide
/// mais ferait grossir le fichier sans raison, et surtout ferait diverger nos
/// octets de ceux du jeu.
pub fn block_states_payload(palette: &[PaletteEntryRef<'_>], data: &[u64]) -> Vec<u8> {
    let mut w = Writer::with_capacity(64 + palette.len() * 48 + data.len() * 8);
    w.field(tag::LIST, "palette");
    w.list_header(tag::COMPOUND, palette.len());
    for e in palette {
        write_palette_entry(&mut w, e);
    }
    if palette.len() > 1 && !data.is_empty() {
        w.field(tag::LONG_ARRAY, "data");
        w.long_array_payload(data);
    }
    w.end();
    w.into_bytes()
}

/// Charge d'une liste de palette 1.13–1.17 (`Palette`), sans son nom.
/// Même contenu que la `palette` de 1.18+ — seul le champ hôte diffère.
pub fn palette_list_payload(palette: &[PaletteEntryRef<'_>]) -> Vec<u8> {
    let mut w = Writer::with_capacity(16 + palette.len() * 48);
    w.list_header(tag::COMPOUND, palette.len());
    for e in palette {
        write_palette_entry(&mut w, e);
    }
    w.into_bytes()
}

/// Charge d'une liste de CHAÎNES, sans son nom.
///
/// C'est la palette des biomes : un biome n'a pas d'état, donc pas de
/// compound `{ Name, Properties }` — juste un nom. Réutiliser l'écrivain de
/// palette de blocs produirait des compounds que le jeu ne sait pas relire à
/// cet endroit.
pub fn string_list_payload(noms: &[&str]) -> Vec<u8> {
    let poids: usize = noms.iter().map(|n| n.len() + 2).sum();
    let mut w = Writer::with_capacity(8 + poids);
    w.list_header(tag::STRING, noms.len());
    for n in noms {
        w.raw_str(n);
    }
    w.into_bytes()
}

/// Compose la CHARGE d'un compound `biomes`, palette et indices.
///
/// Palette d'une entrée → **pas de `data`**, exactement comme pour les blocs :
/// c'est ce que le jeu écrit, et en laisser un de la mauvaise longueur casse
/// le chargement du chunk.
pub fn biomes_payload(noms: &[&str], data: &[u64]) -> Vec<u8> {
    let mut w = Writer::with_capacity(32 + noms.len() * 24 + data.len() * 8);
    w.field(tag::LIST, "palette");
    w.list_header(tag::STRING, noms.len());
    for n in noms {
        w.raw_str(n);
    }
    if noms.len() > 1 && !data.is_empty() {
        w.field(tag::LONG_ARRAY, "data");
        w.long_array_payload(data);
    }
    w.end();
    w.into_bytes()
}

/// Charge d'un `TAG_Long_Array`, sans son nom.
pub fn long_array_payload(data: &[u64]) -> Vec<u8> {
    let mut w = Writer::with_capacity(4 + data.len() * 8);
    w.long_array_payload(data);
    w.into_bytes()
}

/// Champ `TAG_Long_Array` COMPLET — type, nom et charge. Sert à insérer un
/// champ qui n'existait pas dans le chunk d'origine.
pub fn named_long_array(name: &str, data: &[u64]) -> Vec<u8> {
    let mut w = Writer::with_capacity(8 + name.len() + data.len() * 8);
    w.field(tag::LONG_ARRAY, name);
    w.long_array_payload(data);
    w.into_bytes()
}
