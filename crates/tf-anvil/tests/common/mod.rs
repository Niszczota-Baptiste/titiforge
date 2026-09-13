//! Outils partagés par les tests d'intégration.
//!
//! Les deux modules sont INDÉPENDANTS de `src/`, et doivent le rester :
//! `fixture` produit des `.mca` en écrivant ses octets à la main, `frozen`
//! les relit en construisant un arbre NBT complet. C'est cette indépendance
//! qui fait que les tests prouvent quelque chose.

pub mod fixture;
pub mod frozen;
