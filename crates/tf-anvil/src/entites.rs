//! Les block entities — le contenu d'un coffre, le texte d'un panneau.
//!
//! **Elles ne sont pas dans la grille de blocs.** C'est une liste à part du
//! chunk, dont chaque entrée porte ses propres coordonnées MONDE. Toute
//! opération qui déplace ou efface des blocs doit donc les suivre
//! explicitement : `ExeWorldEdit` a payé ce piège, un build pivoté y
//! abandonnait ses coffres.
//!
//! ## Rien n'est ré-encodé
//!
//! Une entrée voyage par ses OCTETS, et on ne réécrit que les douze qui
//! portent `x`, `y` et `z` — dont le balayage a relevé la position exacte.
//! C'est la même propriété structurelle que le splice du chunk : le contenu
//! d'un coffre, les lignes d'un panneau, les données d'un mod passent sans
//! être compris, **donc sans pouvoir être abîmés**. Reconstruire l'entrée
//! depuis un arbre NBT parsé perdrait tout ce que le parseur ne nomme pas, et
//! le perdrait en silence.
//!
//! ## Ce qu'on ne sait pas situer, on n'y touche pas
//!
//! Une entrée sans `x`/`y`/`z` ne se déplace pas : on ne sait pas où elle est,
//! donc on ne sait pas où la reposer. Elle est recopiée telle quelle. La
//! deviner la poserait ailleurs, ce qui est pire que de ne rien faire.

use tf_nbt::{tag, Cur, Span, Trunc, Writer, R};

use crate::format::Layout;

/// Où vivent les trois coordonnées d'une entrée, et ce qu'elles disent.
///
/// Les deux vont ENSEMBLE : connaître la case sans savoir où la réécrire
/// donnerait une entité qu'on sait déplacer mais pas reposer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ancrage {
    /// La case habitée, en coordonnées MONDE, telle qu'écrite.
    pub case: [i32; 3],
    /// Décalages ABSOLUS des charges `x`, `y`, `z` dans le tampon inflaté.
    pub champs: [usize; 3],
}

/// Une block entity repérée dans un chunk, sans rien matérialiser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntiteReperee {
    /// Le compound ENTIER, `TAG_End` compris.
    pub span: Span,
    /// `None` quand l'une des trois coordonnées manque.
    pub ancrage: Option<Ancrage>,
}

/// La liste des block entities d'un chunk : où elle vit, et ce qu'elle porte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListeEntites {
    /// Le CHAMP entier — type, nom et charge — pour pouvoir le remplacer d'un
    /// bloc. `None` quand le chunk n'en porte pas.
    pub champ: Option<Span>,
    /// Le `TAG_End` du compound hôte : là où insérer le champ s'il manque.
    /// Pour la disposition `Legacy`, c'est celui de `Level`.
    pub inserer_a: usize,
    pub entrees: Vec<EntiteReperee>,
}

impl ListeEntites {
    pub fn est_vide(&self) -> bool {
        self.entrees.is_empty()
    }
}

/// Le nom du champ, selon la disposition du chunk.
///
/// Il change avec elle, et se tromper de nom écrirait une liste que le jeu ne
/// lirait pas — les coffres disparaîtraient au chargement, sans erreur.
pub fn nom_du_champ(layout: Layout) -> &'static str {
    match layout {
        Layout::Flat => "block_entities",
        Layout::Legacy => "TileEntities",
    }
}

/// Balaye une liste de block entities, le curseur étant sur son EN-TÊTE.
pub fn balayer_liste(c: &mut Cur) -> R<Vec<EntiteReperee>> {
    let (et, n) = c.list_header()?;
    // Une liste vide s'écrit avec `TAG_End` pour type d'élément — et certains
    // écrivains la marquent `TAG_Compound` avec une longueur nulle. Les deux
    // formes sont vides.
    if et == tag::END || n == 0 {
        return Ok(Vec::new());
    }
    if et != tag::COMPOUND {
        return Err(Trunc);
    }
    // La longueur annoncée vient du FICHIER : la réserver telle quelle fait
    // tenter une allocation de plusieurs gigaoctets sur un `.mca` forgé ou
    // tronqué. Un compound pèse au moins son `TAG_End`, donc la boucle butera
    // de toute façon ; on se contente de ne pas l'annoncer d'avance.
    let mut out = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        out.push(balayer_une(c)?);
    }
    Ok(out)
}

fn balayer_une(c: &mut Cur) -> R<EntiteReperee> {
    let start = c.pos();
    let mut xyz: [Option<(i32, usize)>; 3] = [None; 3];
    while let Some((t, key)) = c.next_field()? {
        // Le décalage est relevé AVANT la lecture : c'est la position de la
        // charge, pas celle de l'en-tête.
        let rang = match (t, key) {
            (tag::INT, "x") => 0,
            (tag::INT, "y") => 1,
            (tag::INT, "z") => 2,
            _ => {
                c.skip_payload(t)?;
                continue;
            }
        };
        let at = c.pos();
        xyz[rang] = Some((c.i32()?, at));
    }
    let ancrage = match (xyz[0], xyz[1], xyz[2]) {
        (Some(x), Some(y), Some(z)) => Some(Ancrage {
            case: [x.0, y.0, z.0],
            champs: [x.1, y.1, z.1],
        }),
        _ => None,
    };
    Ok(EntiteReperee {
        span: Span {
            start,
            end: c.pos(),
        },
        ancrage,
    })
}

/// Une block entity détachée de son chunk, prête à voyager.
///
/// `nbt` sont les octets d'ORIGINE du compound. `champs` dit où y réécrire les
/// trois coordonnées ; `case` dit ce qu'il faut y écrire. Poser l'entité, c'est
/// recopier `nbt` et remplacer ces douze octets-là.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entite {
    /// La case habitée. **Le repère est celui de qui la porte** : monde dans
    /// un chunk, local dans un presse-papiers. Une unité qui traverse une
    /// frontière se vérifie en traversant, et c'est le test de bout en bout
    /// qui le fait.
    pub case: [i32; 3],
    /// Le compound entier, `TAG_End` compris.
    pub nbt: Vec<u8>,
    /// Décalages des charges `x`, `y`, `z` DANS `nbt`.
    pub champs: [usize; 3],
}

impl Entite {
    /// L'entité repérée, détachée du tampon. `None` sans ancrage.
    pub fn depuis(inflated: &[u8], e: &EntiteReperee) -> Option<Entite> {
        let a = e.ancrage?;
        let debut = e.span.start;
        Some(Entite {
            case: a.case,
            nbt: e.span.slice(inflated).to_vec(),
            // Absolus dans le tampon, relatifs dans `nbt` : la conversion se
            // fait ici et nulle part ailleurs.
            champs: [
                a.champs[0] - debut,
                a.champs[1] - debut,
                a.champs[2] - debut,
            ],
        })
    }

    /// Une entrée qu'on ne sait pas SITUER — il lui manque une coordonnée.
    ///
    /// Elle voyage telle quelle et ses coordonnées ne sont jamais réécrites :
    /// on ne sait pas où elle est, donc on ne sait pas où la reposer. La
    /// deviner la poserait ailleurs, ce qui est pire que de ne rien faire.
    pub fn intouchable(nbt: Vec<u8>) -> Entite {
        Entite {
            case: [0, 0, 0],
            nbt,
            champs: [usize::MAX; 3],
        }
    }

    /// Ses octets, coordonnées réécrites à `case`.
    ///
    /// Un décalage hors de `nbt` désigne une entrée qu'on ne sait pas situer :
    /// on recopie sans réécrire, plutôt que de paniquer sur la save de
    /// quelqu'un. L'addition est vérifiée parce que la sentinelle est
    /// `usize::MAX`, et que ce dépôt compile avec `overflow-checks`.
    pub fn octets(&self) -> Vec<u8> {
        let mut out = self.nbt.clone();
        for (k, &at) in self.champs.iter().enumerate() {
            if at.checked_add(4).is_some_and(|fin| fin <= out.len()) {
                out[at..at + 4].copy_from_slice(&self.case[k].to_be_bytes());
            }
        }
        out
    }

    /// La même, déplacée de `d`.
    pub fn deplacee(&self, d: [i32; 3]) -> Entite {
        Entite {
            case: [
                self.case[0] + d[0],
                self.case[1] + d[1],
                self.case[2] + d[2],
            ],
            nbt: self.nbt.clone(),
            champs: self.champs,
        }
    }

    /// Son `id`, tel qu'écrit. Pour l'AFFICHAGE : rien du moteur n'en dépend,
    /// et surtout pas le fait de la suivre — une entité de mod que personne ne
    /// nomme se déplace comme les autres.
    pub fn id(&self) -> Option<&str> {
        let mut c = Cur::at(&self.nbt, 0);
        while let Ok(Some((t, key))) = c.next_field() {
            if t == tag::STRING && key == "id" {
                return c.str().ok();
            }
            if c.skip_payload(t).is_err() {
                return None;
            }
        }
        None
    }
}

/// Le CHAMP complet d'une liste de block entities — type, nom et charge.
pub fn champ_entites(nom: &str, entites: &[Entite]) -> Vec<u8> {
    let poids: usize = entites.iter().map(|e| e.nbt.len()).sum();
    let mut w = Writer::with_capacity(16 + nom.len() + poids);
    w.field(tag::LIST, nom);
    if entites.is_empty() {
        // Une liste vide porte `TAG_End` pour type d'élément : c'est ce
        // qu'écrit le jeu, et une liste de compounds de longueur nulle n'a pas
        // le même octet.
        w.list_header(tag::END, 0);
    } else {
        w.list_header(tag::COMPOUND, entites.len());
        for e in entites {
            w.raw(&e.octets());
        }
    }
    w.into_bytes()
}
