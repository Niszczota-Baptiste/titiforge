//! Les points d'intérêt — lits, postes de travail, cloches, ruches, portails.
//!
//! `poi/r.X.Z.mca` porte, par chunk, `{DataVersion, Sections: {"<y>":
//! {Valid, Records}}}`. Chaque section dit où sont ses points d'intérêt, et un
//! drapeau `Valid` : **à 1, le jeu fait confiance à ses enregistrements et ne
//! relit pas les blocs.** Un lit déplacé restait donc inconnu à sa nouvelle
//! place et fantôme à l'ancienne — un villageois se couchait dans le vide.
//!
//! On ne recalcule rien soi-même, pour la raison de l'éclairage : ce serait
//! réécrire la table des points d'intérêt du jeu pour se tromper là où lui ne
//! se trompe pas. On lui DEMANDE de le faire, par son propre mécanisme : une
//! section à `Valid` = 0 est relue depuis ses blocs au chargement du chunk
//! (`PoiSection.refresh`), et ses enregistrements encore justes y gardent
//! leurs tickets — un lit déjà réclamé le reste.

use tf_nbt::{tag, Cur, Span, R};

use crate::chunk::Edit;

/// Les éditions qui font relire au jeu TOUTES les sections d'un chunk de
/// points d'intérêt : chaque `Valid` non nul passe à 0.
///
/// Toutes, et pas seulement celles dont les blocs ont changé : relire une
/// section juste ne coûte au jeu qu'un regard sur sa palette, et garde ses
/// tickets. Viser juste demanderait de faire traverser le numéro de chaque
/// section modifiée jusqu'ici, pour ne rien gagner.
///
/// Un `Valid` absent vaut FAUX pour le jeu (`optionalFieldOf("Valid", false)`) :
/// rien à écrire. Une section déjà invalide non plus — ce sont les OCTETS qui
/// décident, et une opération qui ne change rien ne salit rien.
pub fn faire_invalider(inflated: &[u8]) -> R<Vec<Edit>> {
    let mut c = Cur::new(inflated);
    c.enter_root()?;
    let mut out = Vec::new();
    while let Some((t, key)) = c.next_field()? {
        if !(t == tag::COMPOUND && key == "Sections") {
            c.skip_payload(t)?;
            continue;
        }
        while let Some((t, _)) = c.next_field()? {
            if t != tag::COMPOUND {
                c.skip_payload(t)?;
                continue;
            }
            while let Some((t, key)) = c.next_field()? {
                if t == tag::BYTE && key == "Valid" {
                    let debut = c.pos();
                    if c.i8()? != 0 {
                        out.push(Edit {
                            span: Span {
                                start: debut,
                                end: debut + 1,
                            },
                            bytes: vec![0],
                        });
                    }
                } else {
                    c.skip_payload(t)?;
                }
            }
        }
    }
    Ok(out)
}
