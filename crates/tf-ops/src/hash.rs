//! Le tirage aléatoire des opérations.
//!
//! **Un tirage PAR BLOC se hache sur la POSITION, jamais sur un état.** Un
//! générateur à état ne rejoue identique que si la boucle visite les cases dans
//! le même ordre — contrainte invisible qu'une parallélisation casserait sans
//! bruit, et on parallélise. Hacher la position rend le résultat indépendant de
//! l'ordre de parcours, donc de la découpe en fils, du nombre de cœurs, et du
//! fait qu'on soit passé par l'étage palette ou l'étage bloc.
//!
//! Conséquence qu'on ne cherchait pas et qui vaut de l'or : le tirage est
//! **stable par morceaux**. Remélanger un coin d'une zone y redonne exactement
//! les mêmes blocs qu'un mélange de la zone entière — l'utilisateur peut
//! retoucher un bout sans voir la couture.

/// Un hachage de position, en 64 bits.
///
/// Trois multiplications pour mélanger les axes, puis **une seule** avalanche.
/// La première version en faisait trois — une par axe — et c'était le poste
/// dominant de l'étage bloc : mesuré sur une région pleine, 12,6 ns par bloc
/// sur 15, contre 2,4 ns pour la même boucle sans tirage.
///
/// Chaque axe a son multiplicateur, impair et sans structure commune : sans
/// ça, `(x, y, z)` et `(y, x, z)` tomberaient sur la même valeur, et un mur
/// vertical montrerait le même motif que le sol. Les rotations entre les axes
/// empêchent les bits hauts d'un axe d'annuler ceux d'un autre.
///
/// **Cette fonction est FIGÉE.** Un build fait avec une graine doit se rejouer
/// à l'identique ; la changer changerait tous les mondes déjà construits, sans
/// que rien ne le signale.
#[inline]
pub fn hash3(x: i32, y: i32, z: i32, seed: u64) -> u64 {
    let mut h = seed;
    h = (h ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)).rotate_left(23);
    h = (h ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)).rotate_left(29);
    h = (h ^ (z as i64 as u64).wrapping_mul(0x1656_67B1_9E37_79F9)).rotate_left(17);
    melanger(h)
}

/// L'avalanche de SplitMix64. Un bit d'entrée qui change en change la moitié
/// en sortie — c'est ce qui fait qu'on peut prendre le modulo des bits BAS
/// sans motif visible.
fn melanger(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
