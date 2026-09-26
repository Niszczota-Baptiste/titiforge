//! Ce que les trois formats partagent : la compression, la lecture d'un
//! compound champ par champ, les noms d'états, et la chirurgie d'octets qui
//! fait voyager une block entity ou une entité SANS la ré-encoder.

use std::borrow::Cow;
use std::io::{Read, Write};

use tf_anvil::state_key;
use tf_nbt::{tag, Cur, Span, Writer};

use crate::Erreur;

// ── compression ─────────────────────────────────────────────────────────────

/// Le plus gros NBT DÉCOMPRESSÉ qu'on accepte de lire.
///
/// Le même ordre de grandeur que le plafond d'un presse-papiers
/// (`MAX_OCTETS_MATERIALISES`) : un fichier plus gros décrirait un extrait
/// que le moteur refuserait de toute façon. Sans plafond, quelques kilo-octets
/// de gzip forgé se décompressent en dizaines de gigaoctets.
pub const MAX_OCTETS_NBT: u64 = 2_000_000_000;

/// Les octets NBT d'un fichier — décompressés s'il le faut.
///
/// **Le format se reconnaît à ses OCTETS**, jamais à l'extension : gzip
/// (`1f 8b`) comme l'écrivent les trois outils, zlib (`78`) pour qui s'en
/// écarte, et un NBT nu (`0a`, un compound racine) tel quel.
pub(crate) fn decompresser(octets: &[u8]) -> Result<Cow<'_, [u8]>, Erreur> {
    decompresser_jusqu_a(octets, MAX_OCTETS_NBT)
}

fn decompresser_jusqu_a(octets: &[u8], plafond: u64) -> Result<Cow<'_, [u8]>, Erreur> {
    let tout_lire = |r: &mut dyn Read| -> Result<Vec<u8>, Erreur> {
        let mut out = Vec::new();
        r.take(plafond + 1)
            .read_to_end(&mut out)
            .map_err(|_| Erreur::Illisible)?;
        if out.len() as u64 > plafond {
            return Err(Erreur::TropGros {
                octets: out.len() as u64,
                plafond,
            });
        }
        Ok(out)
    };
    match octets {
        [0x1f, 0x8b, ..] => Ok(Cow::Owned(tout_lire(
            &mut flate2::read::MultiGzDecoder::new(octets),
        )?)),
        [0x78, ..] => Ok(Cow::Owned(tout_lire(&mut flate2::read::ZlibDecoder::new(
            octets,
        ))?)),
        [tag::COMPOUND, ..] => Ok(Cow::Borrowed(octets)),
        _ => Err(Erreur::Illisible),
    }
}

/// Un fichier gzip, comme l'écrivent WorldEdit, Litematica et le jeu.
///
/// L'en-tête ne porte AUCUNE date (`mtime` à zéro) : le même extrait donne les
/// mêmes octets, ce qui rend l'écriture comparable dans un test — et un
/// fichier qui ne change pas ne change pas.
pub(crate) fn compresser(nbt: &[u8]) -> Vec<u8> {
    let mut e = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::default());
    e.write_all(nbt)
        .expect("écrire en mémoire ne peut pas échouer");
    e.finish().expect("écrire en mémoire ne peut pas échouer")
}

// ── lire un compound ────────────────────────────────────────────────────────

/// Un champ d'un compound : son type, son nom, où commence son EN-TÊTE, et
/// où est sa charge.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Champ<'a> {
    pub t: u8,
    pub nom: &'a str,
    pub entete: usize,
    pub charge: Span,
}

/// Un compound vu champ par champ, sans rien matérialiser : les charges
/// restent dans le tampon, repérées par leur plage.
#[derive(Debug, Clone)]
pub(crate) struct Compound<'a> {
    pub buf: &'a [u8],
    pub champs: Vec<Champ<'a>>,
    /// Sa charge ENTIÈRE, `TAG_End` compris.
    pub span: Span,
}

impl<'a> Compound<'a> {
    /// Le compound dont la charge commence à `pos`.
    pub fn lire(buf: &'a [u8], pos: usize) -> Result<Compound<'a>, Erreur> {
        let mut c = Cur::at(buf, pos);
        let mut champs = Vec::new();
        loop {
            let entete = c.pos();
            let Some((t, nom)) = c.next_field().map_err(|_| Erreur::Illisible)? else {
                break;
            };
            let charge = c.span_of_payload(t).map_err(|_| Erreur::Illisible)?;
            champs.push(Champ {
                t,
                nom,
                entete,
                charge,
            });
        }
        Ok(Compound {
            buf,
            champs,
            span: Span {
                start: pos,
                end: c.pos(),
            },
        })
    }

    /// La racine d'un fichier NBT : son nom, et le compound.
    pub fn racine(buf: &'a [u8]) -> Result<(&'a str, Compound<'a>), Erreur> {
        let mut c = Cur::new(buf);
        let nom = c.enter_root().map_err(|_| Erreur::Illisible)?;
        let racine = Compound::lire(buf, c.pos())?;
        Ok((nom, racine))
    }

    /// Le champ de ce nom. En double — un NBT invalide, mais qui existe — le
    /// DERNIER gagne, comme dans la table de hachage du jeu.
    pub fn champ(&self, nom: &str) -> Option<Champ<'a>> {
        self.champs.iter().rev().find(|c| c.nom == nom).copied()
    }

    fn cur(&self, c: Champ<'a>) -> Cur<'a> {
        Cur::at(self.buf, c.charge.start)
    }

    /// Un entier, quelle que soit sa LARGEUR : un fichier écrit à la main ou
    /// par un autre outil met `Version` en short ou en long. Un entier n'est
    /// pas un flottant : un `Width` en double est refusé.
    pub fn entier(&self, nom: &str) -> Option<i64> {
        let c = self.champ(nom)?;
        let mut k = self.cur(c);
        match c.t {
            tag::BYTE => k.i8().ok().map(i64::from),
            tag::SHORT => k.i16().ok().map(i64::from),
            tag::INT => k.i32().ok().map(i64::from),
            tag::LONG => k.i64().ok(),
            _ => None,
        }
    }

    /// Une dimension Sponge : un short NON signé — WorldEdit le lit
    /// `& 0xFFFF`, un côté de 40 000 blocs s'écrit donc −25 536 — ou un entier
    /// plus large, écrit par un autre outil.
    pub fn dimension(&self, nom: &str) -> Option<i64> {
        let c = self.champ(nom)?;
        if c.t == tag::SHORT {
            return self.cur(c).i16().ok().map(|v| i64::from(v as u16));
        }
        self.entier(nom)
    }

    pub fn chaine(&self, nom: &str) -> Option<&'a str> {
        let c = self.champ(nom).filter(|c| c.t == tag::STRING)?;
        self.cur(c).str().ok()
    }

    pub fn compound(&self, nom: &str) -> Result<Option<Compound<'a>>, Erreur> {
        match self.champ(nom).filter(|c| c.t == tag::COMPOUND) {
            Some(c) => Compound::lire(self.buf, c.charge.start).map(Some),
            None => Ok(None),
        }
    }

    pub fn octets(&self, nom: &str) -> Option<&'a [u8]> {
        let c = self.champ(nom).filter(|c| c.t == tag::BYTE_ARRAY)?;
        self.cur(c).byte_array().ok()
    }

    pub fn entiers(&self, nom: &str) -> Option<Vec<i32>> {
        let c = self.champ(nom).filter(|c| c.t == tag::INT_ARRAY)?;
        self.cur(c).int_array().ok()
    }

    pub fn longs(&self, nom: &str) -> Option<Vec<u64>> {
        let c = self.champ(nom).filter(|c| c.t == tag::LONG_ARRAY)?;
        self.cur(c).long_array().ok()
    }

    pub fn liste(&self, nom: &str) -> Result<Option<Liste<'a>>, Erreur> {
        let Some(c) = self.champ(nom).filter(|c| c.t == tag::LIST) else {
            return Ok(None);
        };
        let mut k = self.cur(c);
        let (element, n) = k.list_header().map_err(|_| Erreur::Illisible)?;
        Ok(Some(Liste {
            buf: self.buf,
            element,
            n,
            debut: k.pos(),
        }))
    }

    /// Trois coordonnées, de quelque forme qu'un outil les écrive : un
    /// `TAG_Int_Array` de trois, une liste de trois entiers, ou un compound
    /// `{x, y, z}` (celui de Litematica).
    pub fn triplet(&self, nom: &str) -> Result<Option<[i32; 3]>, Erreur> {
        if let Some(v) = self.entiers(nom) {
            return Ok(<[i32; 3]>::try_from(v).ok());
        }
        if let Some(l) = self.liste(nom)? {
            return Ok(l.entiers()?.and_then(|v| <[i32; 3]>::try_from(v).ok()));
        }
        if let Some(c) = self.compound(nom)? {
            let lire = |k: &str| c.entier(k).and_then(|v| i32::try_from(v).ok());
            return Ok(match (lire("x"), lire("y"), lire("z")) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            });
        }
        Ok(None)
    }

    /// Trois doubles (une position d'entité).
    pub fn position(&self, nom: &str) -> Result<Option<[f64; 3]>, Erreur> {
        let Some(l) = self.liste(nom)? else {
            return Ok(None);
        };
        Ok(l.doubles()?.and_then(|v| <[f64; 3]>::try_from(v).ok()))
    }

    /// La charge entière du compound, `TAG_End` compris : ce qu'une block
    /// entity ou une entité emporte sans être comprise.
    pub fn tout(&self) -> &'a [u8] {
        self.span.slice(self.buf)
    }

    /// Ses champs en octets d'origine, SAUF ceux nommés — et sans le `TAG_End`
    /// final, pour qu'on puisse en ajouter.
    pub fn champs_sauf(&self, noms: &[&str]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.span.len());
        for c in &self.champs {
            if !noms.contains(&c.nom) {
                out.extend_from_slice(&self.buf[c.entete..c.charge.end]);
            }
        }
        out
    }
}

/// Une liste : le type de ses éléments, leur nombre, et où ils commencent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Liste<'a> {
    buf: &'a [u8],
    pub element: u8,
    pub n: usize,
    debut: usize,
}

impl<'a> Liste<'a> {
    /// Ses compounds. Une liste VIDE s'accepte sous toutes ses formes — le jeu
    /// l'écrit avec `TAG_End` pour type, d'autres avec `TAG_Compound`.
    pub fn compounds(&self) -> Result<Vec<Compound<'a>>, Erreur> {
        if self.n == 0 || self.element == tag::END {
            return Ok(Vec::new());
        }
        if self.element != tag::COMPOUND {
            return Err(Erreur::Illisible);
        }
        // La longueur vient du FICHIER : on ne la réserve pas telle quelle.
        let mut out = Vec::with_capacity(self.n.min(4096));
        let mut pos = self.debut;
        for _ in 0..self.n {
            let c = Compound::lire(self.buf, pos)?;
            pos = c.span.end;
            out.push(c);
        }
        Ok(out)
    }

    /// Ses listes (les `palettes` d'une structure à variantes).
    pub fn listes(&self) -> Result<Vec<Liste<'a>>, Erreur> {
        if self.n == 0 || self.element == tag::END {
            return Ok(Vec::new());
        }
        if self.element != tag::LIST {
            return Err(Erreur::Illisible);
        }
        let mut out = Vec::with_capacity(self.n.min(64));
        let mut c = Cur::at(self.buf, self.debut);
        for _ in 0..self.n {
            let (element, n) = c.list_header().map_err(|_| Erreur::Illisible)?;
            let debut = c.pos();
            c.skip_list_body(element, n)
                .map_err(|_| Erreur::Illisible)?;
            out.push(Liste {
                buf: self.buf,
                element,
                n,
                debut,
            });
        }
        Ok(out)
    }

    /// Ses entiers, s'ils en sont. `None` pour une liste d'autre chose.
    pub fn entiers(&self) -> Result<Option<Vec<i32>>, Erreur> {
        if self.element != tag::INT {
            return Ok((self.n == 0).then(Vec::new));
        }
        let mut c = Cur::at(self.buf, self.debut);
        let mut out = Vec::with_capacity(self.n.min(16));
        for _ in 0..self.n {
            out.push(c.i32().map_err(|_| Erreur::Illisible)?);
        }
        Ok(Some(out))
    }

    /// Ses doubles, s'ils en sont.
    pub fn doubles(&self) -> Result<Option<Vec<f64>>, Erreur> {
        if self.element != tag::DOUBLE {
            return Ok((self.n == 0).then(Vec::new));
        }
        let mut c = Cur::at(self.buf, self.debut);
        let mut out = Vec::with_capacity(self.n.min(16));
        for _ in 0..self.n {
            out.push(c.f64().map_err(|_| Erreur::Illisible)?);
        }
        Ok(Some(out))
    }
}

// ── les noms d'états ────────────────────────────────────────────────────────

/// Un nom de bloc ou de propriété qu'on accepte de faire entrer dans une
/// clé : ni vide, ni porteur d'un des séparateurs de la clé canonique —
/// sinon deux états différents pourraient s'y confondre.
fn mot_propre(s: &str) -> bool {
    !s.is_empty() && !s.contains(['|', '[', ']', ',', '=', '{', '}', '"'])
}

/// Le nom complet d'un bloc : sans espace de noms, c'est `minecraft:`.
///
/// C'est la règle du jeu (`ResourceLocation`) ; la garder hors de la clé
/// ferait de `stone` et `minecraft:stone` deux états que la palette de la save
/// porterait côte à côte.
fn nom_complet(nom: &str) -> Cow<'_, str> {
    if nom.contains(':') {
        Cow::Borrowed(nom)
    } else {
        Cow::Owned(format!("minecraft:{nom}"))
    }
}

/// La clé d'un état écrit comme WorldEdit l'écrit :
/// `minecraft:oak_stairs[facing=east,half=bottom]`. `None` si la chaîne ne se
/// lit pas — elle n'est alors ni devinée ni tronquée.
pub(crate) fn cle_depuis_chaine(s: &str) -> Option<String> {
    let s = s.trim();
    let (nom, props) = match s.split_once('[') {
        None => (s, ""),
        Some((nom, reste)) => (nom, reste.strip_suffix(']')?),
    };
    if !mot_propre(nom) {
        return None;
    }
    let mut paires = Vec::new();
    if !props.is_empty() {
        for kv in props.split(',') {
            let (k, v) = kv.split_once('=')?;
            let (k, v) = (k.trim(), v.trim());
            if !mot_propre(k) || !mot_propre(v) {
                return None;
            }
            paires.push((k.to_string(), v.to_string()));
        }
    }
    Some(state_key(&nom_complet(nom), &mut paires))
}

/// La chaîne d'un état, comme WorldEdit l'écrit dans une palette.
pub(crate) fn chaine_depuis_cle(cle: &str) -> String {
    match cle.split_once('|') {
        None => cle.to_string(),
        Some((nom, props)) => format!("{nom}[{props}]"),
    }
}

/// La clé d'un état écrit en compound `{Name, Properties}` — Litematica et le
/// jeu. `None` si le compound ne porte pas de nom lisible.
pub(crate) fn cle_depuis_compound(c: &Compound) -> Result<Option<String>, Erreur> {
    let Some(nom) = c.chaine("Name") else {
        return Ok(None);
    };
    if !mot_propre(nom) {
        return Ok(None);
    }
    let mut paires = Vec::new();
    if let Some(p) = c.compound("Properties")? {
        for ch in &p.champs {
            // Une propriété qui n'est pas une chaîne n'existe pas dans le
            // jeu : on refuse l'état plutôt que d'en deviner la valeur.
            if ch.t != tag::STRING {
                return Ok(None);
            }
            let v = Cur::at(p.buf, ch.charge.start)
                .str()
                .map_err(|_| Erreur::Illisible)?;
            if !mot_propre(ch.nom) || !mot_propre(v) {
                return Ok(None);
            }
            paires.push((ch.nom.to_string(), v.to_string()));
        }
    }
    Ok(Some(state_key(&nom_complet(nom), &mut paires)))
}

/// Écrit un état en compound `{Name, Properties}` — sans `TAG_End` du
/// compound hôte, que l'appelant ferme.
pub(crate) fn ecrire_etat(w: &mut Writer, cle: &str) {
    let (nom, props) = tf_anvil::split_key(cle);
    w.field(tag::STRING, "Name").raw_str(nom);
    if !props.is_empty() {
        w.field(tag::COMPOUND, "Properties");
        for (k, v) in &props {
            w.field(tag::STRING, k).raw_str(v);
        }
        w.end();
    }
}

// ── écrire ──────────────────────────────────────────────────────────────────

/// Trois entiers en `TAG_Int_Array`.
pub(crate) fn champ_triplet(w: &mut Writer, nom: &str, v: [i32; 3]) {
    w.field(tag::INT_ARRAY, nom).int_array_payload(&v);
}

/// Trois entiers en liste — la forme du `.nbt` de structure.
pub(crate) fn champ_liste_entiers(w: &mut Writer, nom: &str, v: [i32; 3]) {
    w.field(tag::LIST, nom).list_header(tag::INT, 3);
    for x in v {
        w.i32_payload(x);
    }
}

/// Trois doubles en liste — une position d'entité.
pub(crate) fn champ_position(w: &mut Writer, nom: &str, v: [f64; 3]) {
    w.field(tag::LIST, nom).list_header(tag::DOUBLE, 3);
    for x in v {
        w.f64_payload(x);
    }
}

/// `{x, y, z}` en compound d'entiers — la forme de Litematica.
pub(crate) fn champ_xyz(w: &mut Writer, nom: &str, v: [i32; 3]) {
    w.field(tag::COMPOUND, nom);
    w.field(tag::INT, "x").i32_payload(v[0]);
    w.field(tag::INT, "y").i32_payload(v[1]);
    w.field(tag::INT, "z").i32_payload(v[2]);
    w.end();
}

/// L'en-tête d'une liste de compounds de `n` éléments — ou la forme VIDE du
/// jeu, `TAG_End` pour type.
pub(crate) fn entete_liste(w: &mut Writer, nom: &str, n: usize) {
    w.field(tag::LIST, nom);
    if n == 0 {
        w.list_header(tag::END, 0);
    } else {
        w.list_header(tag::COMPOUND, n);
    }
}

// ── les varints de Sponge ───────────────────────────────────────────────────

/// Un entier en varint — sept bits par octet, poids faibles d'abord.
pub(crate) fn ecrire_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Lit un varint à `*i`. `None` s'il est tronqué ou déborde les 32 bits.
pub(crate) fn lire_varint(b: &[u8], i: &mut usize) -> Option<u32> {
    let mut v: u32 = 0;
    for rang in 0..5 {
        let o = *b.get(*i)?;
        *i += 1;
        let bits = (o & 0x7f) as u32;
        // Le cinquième octet ne porte que 4 bits utiles : au-delà, le nombre
        // ne tient plus dans un `int` Java, et c'est un fichier forgé.
        if rang == 4 && bits > 0x0f {
            return None;
        }
        v |= bits << (7 * rang);
        if o & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Une bombe gzip s'arrête au plafond** — sans décompresser au-delà.
    /// Mesuré sur un plafond réduit : dix mille zéros tiennent en quelques
    /// dizaines d'octets de gzip.
    #[test]
    fn une_bombe_gzip_s_arrete_au_plafond() {
        let bombe = compresser(&vec![0u8; 10_000]);
        assert!(bombe.len() < 100, "{}", bombe.len());
        assert_eq!(
            decompresser_jusqu_a(&bombe, 1_000).unwrap_err(),
            Erreur::TropGros {
                octets: 1_001,
                plafond: 1_000
            }
        );
        assert_eq!(
            decompresser_jusqu_a(&bombe, 10_000).unwrap().len(),
            10_000,
            "juste au plafond, ça passe"
        );
    }

    #[test]
    fn un_etat_ecrit_par_worldedit_devient_une_cle() {
        let cas = [
            ("minecraft:stone", Some("minecraft:stone")),
            ("stone", Some("minecraft:stone")),
            (
                "minecraft:oak_stairs[waterlogged=false,facing=east]",
                Some("minecraft:oak_stairs|facing=east,waterlogged=false"),
            ),
            (
                " minefield:chaise[facing = south] ",
                Some("minefield:chaise|facing=south"),
            ),
            ("minecraft:stone[]", Some("minecraft:stone")),
            ("minecraft:stone[facing=east", None),
            ("minecraft:stone[facing]", None),
            ("minecraft:stone[=east]", None),
            ("minecraft:stone[facing=]", None),
            ("", None),
            ("minecraft:a|b", None),
            ("minecraft:stone[a=b,c=d=e]", None),
        ];
        for (chaine, cle) in cas {
            assert_eq!(cle_depuis_chaine(chaine).as_deref(), cle, "« {chaine} »");
        }
        assert_eq!(
            chaine_depuis_cle("minecraft:oak_stairs|facing=east,waterlogged=false"),
            "minecraft:oak_stairs[facing=east,waterlogged=false]"
        );
        assert_eq!(chaine_depuis_cle("minecraft:stone"), "minecraft:stone");
    }

    /// Les varints de Sponge : un aller-retour sur toute la plage, et le refus
    /// d'un nombre qui ne tient pas dans un `int` Java.
    #[test]
    fn les_varints_font_l_aller_retour() {
        let valeurs = [
            0u32,
            1,
            127,
            128,
            255,
            16_383,
            16_384,
            2_097_151,
            u32::MAX >> 1,
            u32::MAX,
        ];
        let mut b = Vec::new();
        for &v in &valeurs {
            ecrire_varint(&mut b, v);
        }
        let mut i = 0;
        for &v in &valeurs {
            assert_eq!(lire_varint(&b, &mut i), Some(v));
        }
        assert_eq!(i, b.len());
        // Tronqué, trop long, trop gros.
        assert_eq!(lire_varint(&[0x80], &mut 0), None);
        assert_eq!(
            lire_varint(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01], &mut 0),
            None
        );
        assert_eq!(lire_varint(&[0xff, 0xff, 0xff, 0xff, 0x1f], &mut 0), None);
        assert_eq!(
            lire_varint(&[0xff, 0xff, 0xff, 0xff, 0x0f], &mut 0),
            Some(u32::MAX)
        );
    }
}
