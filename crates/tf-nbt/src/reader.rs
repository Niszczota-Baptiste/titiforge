//! Curseur de lecture. Rend des tranches empruntées au tampon d'origine :
//! aucune allocation tant que l'appelant n'en demande pas.

use crate::tag;

/// Entrée tronquée, malformée, ou type de tag inconnu.
///
/// Un seul type d'erreur exprès : à ce niveau la distinction « tronqué » /
/// « invalide » n'aide personne, et un chunk corrompu se traite pareil dans
/// les deux cas — on le saute et on garde ses octets d'origine.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Trunc;

pub type R<T> = Result<T, Trunc>;

/// Type d'un tag, tel que lu dans le flux.
pub type TagId = u8;

/// Plage d'octets dans le tampon d'origine, demi-ouverte : `[start, end)`.
///
/// C'est la clé du round-trip lossless : réécrire une section, c'est remplacer
/// les octets de cette plage et recopier le reste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
    pub fn slice<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.start.min(buf.len())..self.end.min(buf.len())]
    }
}

pub struct Cur<'a> {
    buf: &'a [u8],
    pos: usize,
    /// Garde-fou contre une imbrication pathologique. Un chunk légitime ne
    /// dépasse pas une dizaine de niveaux ; sans limite, un fichier forgé fait
    /// déborder la pile par récursion de `skip_payload`.
    depth: u16,
}

const MAX_DEPTH: u16 = 512;

impl<'a> Cur<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Cur { buf, pos: 0, depth: 0 }
    }

    pub fn at(buf: &'a [u8], pos: usize) -> Self {
        Cur { buf, pos, depth: 0 }
    }

    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn buf(&self) -> &'a [u8] {
        self.buf
    }

    #[inline]
    pub fn is_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    #[inline]
    fn need(&self, n: usize) -> R<()> {
        // `pos + n` peut déborder sur une longueur forgée : on compare par
        // soustraction, jamais par addition.
        if n <= self.buf.len() - self.pos.min(self.buf.len()) {
            Ok(())
        } else {
            Err(Trunc)
        }
    }

    #[inline]
    pub fn u8(&mut self) -> R<u8> {
        self.need(1)?;
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    #[inline]
    pub fn i8(&mut self) -> R<i8> {
        Ok(self.u8()? as i8)
    }

    #[inline]
    pub fn u16(&mut self) -> R<u16> {
        self.need(2)?;
        let v = u16::from_be_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    #[inline]
    pub fn i32(&mut self) -> R<i32> {
        self.need(4)?;
        let v = i32::from_be_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    /// Un long NBT est BIG-endian. Le lire en natif inverserait silencieusement
    /// chaque indice de palette de la section : le build sortirait en bouillie
    /// et rien ne le signalerait.
    #[inline]
    pub fn u64(&mut self) -> R<u64> {
        self.need(8)?;
        let v = u64::from_be_bytes(self.buf[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    /// Chaîne NBT : longueur `u16` BE puis les octets.
    ///
    /// Minecraft utilise l'UTF-8 *modifié* de Java. Les identifiants de blocs
    /// et les noms de propriétés sont de l'ASCII pur, donc les deux coïncident
    /// là où on lit. Une chaîne qui n'est pas de l'UTF-8 valide est une erreur
    /// plutôt qu'un remplacement silencieux : mieux vaut garder le chunk tel
    /// quel que d'en réécrire une version approximative.
    #[inline]
    pub fn str(&mut self) -> R<&'a str> {
        let n = self.u16()? as usize;
        self.need(n)?;
        let s = core::str::from_utf8(&self.buf[self.pos..self.pos + n]).map_err(|_| Trunc)?;
        self.pos += n;
        Ok(s)
    }

    #[inline]
    pub fn skip(&mut self, n: usize) -> R<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }

    /// Saute la charge d'un tag dont on connaît le type, sans rien allouer.
    /// C'est le cœur du lecteur : tout ce qui n'est pas cherché passe par là.
    pub fn skip_payload(&mut self, t: TagId) -> R<()> {
        if self.depth >= MAX_DEPTH {
            return Err(Trunc);
        }
        if let Some(n) = tag::fixed_size(t) {
            return self.skip(n);
        }
        match t {
            tag::BYTE_ARRAY => {
                let n = self.count()?;
                self.skip(n)
            }
            tag::STRING => {
                let n = self.u16()? as usize;
                self.skip(n)
            }
            tag::INT_ARRAY => {
                let n = self.count()?;
                self.skip(n.checked_mul(4).ok_or(Trunc)?)
            }
            tag::LONG_ARRAY => {
                let n = self.count()?;
                self.skip(n.checked_mul(8).ok_or(Trunc)?)
            }
            tag::LIST => {
                let et = self.u8()?;
                let n = self.count()?;
                if !tag::is_valid(et) {
                    return Err(Trunc);
                }
                // Une liste de TAG_End ne porte aucune charge, quelle que soit
                // la longueur annoncée — c'est la forme d'une liste vide.
                if et == tag::END {
                    return Ok(());
                }
                if let Some(sz) = tag::fixed_size(et) {
                    return self.skip(n.checked_mul(sz).ok_or(Trunc)?);
                }
                self.depth += 1;
                let r = (|| {
                    for _ in 0..n {
                        self.skip_payload(et)?;
                    }
                    Ok(())
                })();
                self.depth -= 1;
                r
            }
            tag::COMPOUND => {
                self.depth += 1;
                let r = (|| loop {
                    let t = self.u8()?;
                    if t == tag::END {
                        return Ok(());
                    }
                    if !tag::is_valid(t) {
                        return Err(Trunc);
                    }
                    let n = self.u16()? as usize;
                    self.skip(n)?; // le nom
                    self.skip_payload(t)?;
                })();
                self.depth -= 1;
                r
            }
            _ => Err(Trunc),
        }
    }

    /// Longueur d'un tableau ou d'une liste.
    ///
    /// NBT l'encode en `i32` SIGNÉ. Une longueur négative existe dans la
    /// nature (fichiers tronqués, générateurs bogués) : on la traite comme
    /// zéro plutôt que de la convertir en un `usize` colossal qui ferait
    /// tenter une allocation de plusieurs exaoctets.
    #[inline]
    fn count(&mut self) -> R<usize> {
        Ok(self.i32()?.max(0) as usize)
    }

    /// Lit l'en-tête de la racine (`TAG_Compound` + son nom) et laisse le
    /// curseur sur le premier champ. Rend le nom de la racine.
    pub fn enter_root(&mut self) -> R<&'a str> {
        if self.u8()? != tag::COMPOUND {
            return Err(Trunc);
        }
        self.str()
    }

    /// Parcourt les champs d'un compound. Rend `None` au `TAG_End`.
    /// Après un `Some((t, nom))`, le curseur est sur la CHARGE du champ :
    /// à l'appelant de la lire ou de la sauter.
    pub fn next_field(&mut self) -> R<Option<(TagId, &'a str)>> {
        let t = self.u8()?;
        if t == tag::END {
            return Ok(None);
        }
        if !tag::is_valid(t) {
            return Err(Trunc);
        }
        Ok(Some((t, self.str()?)))
    }

    /// Saute la charge d'un tag et rend la plage d'octets qu'elle occupait.
    pub fn span_of_payload(&mut self, t: TagId) -> R<Span> {
        let start = self.pos;
        self.skip_payload(t)?;
        Ok(Span { start, end: self.pos })
    }

    /// En-tête de liste : type d'élément et longueur.
    pub fn list_header(&mut self) -> R<(TagId, usize)> {
        let et = self.u8()?;
        let n = self.count()?;
        if !tag::is_valid(et) {
            return Err(Trunc);
        }
        Ok((et, n))
    }

    /// Lit un `TAG_Long_Array` en longs natifs.
    pub fn long_array(&mut self) -> R<Vec<u64>> {
        let n = self.count()?;
        // On vérifie la place AVANT de réserver : sinon une longueur forgée
        // fait allouer des gigaoctets pour un fichier de trois octets.
        self.need(n.checked_mul(8).ok_or(Trunc)?)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.u64()?);
        }
        Ok(out)
    }
}
