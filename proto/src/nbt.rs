//! Lecteur NBT CIBLÉ et sans copie.
//!
//! Ce n'est PAS un parseur NBT généraliste, et c'est tout l'intérêt : le bench
//! de we-engine montre que `prismarine-nbt` pèse 35 % du décodage d'une région
//! une fois le dépack de sections corrigé. Un parseur généraliste construit un
//! arbre d'objets pour un chunk dont on ne veut que `sections[].block_states`.
//!
//! Ici on MARCHE dans les octets : on saute tout ce qu'on ne cherche pas, et on
//! ne matérialise que la palette et le tableau de longs. Le reste du chunk
//! reste une tranche d'octets — ce qui rend le round-trip lossless gratuit au
//! lieu d'être payé en RAM.

pub const END: u8 = 0;
pub const BYTE: u8 = 1;
pub const SHORT: u8 = 2;
pub const INT: u8 = 3;
pub const LONG: u8 = 4;
pub const FLOAT: u8 = 5;
pub const DOUBLE: u8 = 6;
pub const BYTE_ARRAY: u8 = 7;
pub const STRING: u8 = 8;
pub const LIST: u8 = 9;
pub const COMPOUND: u8 = 10;
pub const INT_ARRAY: u8 = 11;
pub const LONG_ARRAY: u8 = 12;

#[derive(Debug)]
pub struct Trunc;
pub type R<T> = Result<T, Trunc>;

/// Curseur sur les octets d'un chunk inflaté.
pub struct Cur<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Cur { b, p: 0 }
    }

    #[inline]
    fn need(&self, n: usize) -> R<()> {
        if self.p + n <= self.b.len() { Ok(()) } else { Err(Trunc) }
    }

    #[inline]
    pub fn u8(&mut self) -> R<u8> {
        self.need(1)?;
        let v = self.b[self.p];
        self.p += 1;
        Ok(v)
    }

    #[inline]
    pub fn u16(&mut self) -> R<u16> {
        self.need(2)?;
        let v = u16::from_be_bytes([self.b[self.p], self.b[self.p + 1]]);
        self.p += 2;
        Ok(v)
    }

    #[inline]
    pub fn i32(&mut self) -> R<i32> {
        self.need(4)?;
        let v = i32::from_be_bytes(self.b[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        Ok(v)
    }

    /// Nom de tag ou chaîne : u16 BE de longueur puis les octets.
    /// Rend une TRANCHE — aucune allocation tant que l'appelant n'en veut pas.
    #[inline]
    pub fn str(&mut self) -> R<&'a str> {
        let n = self.u16()? as usize;
        self.need(n)?;
        let s = std::str::from_utf8(&self.b[self.p..self.p + n]).map_err(|_| Trunc)?;
        self.p += n;
        Ok(s)
    }

    #[inline]
    pub fn need_long(&mut self) -> R<()> {
        self.need(8)
    }

    /// Un long NBT est BIG-endian. Le lire en natif silencieusement inverserait
    /// chaque indice de palette de la section — et le build sortirait en bouillie
    /// sans la moindre erreur.
    #[inline]
    pub fn u64_be(&mut self) -> u64 {
        let v = u64::from_be_bytes(self.b[self.p..self.p + 8].try_into().unwrap());
        self.p += 8;
        v
    }

    #[inline]
    pub fn skip(&mut self, n: usize) -> R<()> {
        self.need(n)?;
        self.p += n;
        Ok(())
    }

    /// Saute la CHARGE d'un tag dont on connaît le type. Le cœur du lecteur :
    /// tout ce qui n'est pas cherché passe par là, en O(taille) sans allouer.
    pub fn skip_payload(&mut self, t: u8) -> R<()> {
        match t {
            END => Ok(()),
            BYTE => self.skip(1),
            SHORT => self.skip(2),
            INT | FLOAT => self.skip(4),
            LONG | DOUBLE => self.skip(8),
            BYTE_ARRAY => {
                let n = self.i32()?.max(0) as usize;
                self.skip(n)
            }
            STRING => {
                let n = self.u16()? as usize;
                self.skip(n)
            }
            INT_ARRAY => {
                let n = self.i32()?.max(0) as usize;
                self.skip(n.saturating_mul(4))
            }
            LONG_ARRAY => {
                let n = self.i32()?.max(0) as usize;
                self.skip(n.saturating_mul(8))
            }
            LIST => {
                let et = self.u8()?;
                let n = self.i32()?.max(0) as usize;
                // Les types à taille fixe se sautent d'un seul coup.
                let fixed = match et {
                    BYTE => 1,
                    SHORT => 2,
                    INT | FLOAT => 4,
                    LONG | DOUBLE => 8,
                    END => 0,
                    _ => 0,
                };
                if fixed > 0 {
                    return self.skip(n.saturating_mul(fixed));
                }
                if et == END {
                    return Ok(());
                }
                for _ in 0..n {
                    self.skip_payload(et)?;
                }
                Ok(())
            }
            COMPOUND => {
                loop {
                    let t = self.u8()?;
                    if t == END {
                        return Ok(());
                    }
                    let n = self.u16()? as usize;
                    self.skip(n)?; // le nom
                    self.skip_payload(t)?;
                }
            }
            _ => Err(Trunc),
        }
    }
}
