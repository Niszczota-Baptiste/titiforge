//! **Un tableau qui grandit sans jamais recopier ce qu'il porte.**
//!
//! Un `Vec` double sa capacité quand il est plein, et recopie alors TOUT ce
//! qu'il contient. C'est de l'O(1) amorti — et un pic en O(scène) à chaque
//! doublement. Mesuré en vol sur du bâti : **16 ms** d'une image pour passer
//! l'arène des quads de 1,2 à 2,4 millions d'instances, **6,6 ms** pour les
//! poses. Tous les pics de plus de quatre millisecondes des deux arènes
//! étaient ça, et rien d'autre. Une scène qui remplit son budget pèse huit
//! fois ce vol : le doublement suivant aurait figé la fenêtre plus d'un
//! dixième de seconde.
//!
//! Ici le tableau est découpé en PAGES de taille fixe. Grandir ajoute une
//! page ; rien de ce qui existe ne bouge. Le prix est une division par une
//! puissance de deux à chaque accès, et le fait qu'on ne peut plus prendre
//! une tranche contiguë de TOUT le tableau — ce que seul l'envoi au GPU
//! demandait, et qui se fait page par page (`pages`).

use std::ops::{Index, IndexMut};

/// Éléments par page. Une puissance de deux, pour que l'indice se découpe en
/// décalages ; 65 536 fait un mégaoctet de quads, assez pour que le tableau
/// des pages reste minuscule et assez peu pour qu'en ajouter une ne se voie
/// pas.
pub const PAGE: usize = 1 << 16;

#[derive(Debug, Clone)]
pub struct Pages<T: Copy> {
    pages: Vec<Box<[T]>>,
    len: usize,
    /// La valeur des cases qu'on crée en grandissant.
    vide: T,
}

impl<T: Copy> Pages<T> {
    pub fn new(vide: T) -> Pages<T> {
        Pages {
            pages: Vec::new(),
            len: 0,
            vide,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Grandit ou raccourcit jusqu'à `n` éléments. Les nouveaux valent `vide`
    /// — y compris dans une page réemployée après un raccourcissement, qui
    /// porte encore l'ancien contenu.
    pub fn resize(&mut self, n: usize) {
        if n > self.len {
            let pages = n.div_ceil(PAGE);
            while self.pages.len() < pages {
                self.pages.push(vec![self.vide; PAGE].into_boxed_slice());
            }
            // Directement dans les pages : l'indexation vérifie `i < len`, et
            // la longueur n'est posée qu'après.
            for i in self.len..n {
                self.pages[i / PAGE][i % PAGE] = self.vide;
            }
        }
        self.len = n;
        // Les pages entièrement au-delà se rendent : un tableau qui a
        // raccourci ne doit pas garder la mémoire de son plus haut.
        self.pages.truncate(n.div_ceil(PAGE));
    }

    pub fn truncate(&mut self, n: usize) {
        if n < self.len {
            self.resize(n);
        }
    }

    /// Remplit `debut..fin` avec `v`.
    pub fn fill(&mut self, debut: usize, fin: usize, v: T) {
        debug_assert!(fin <= self.len, "remplissage au-delà du tableau");
        for i in debut..fin {
            self[i] = v;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> + '_ {
        self.pages().flat_map(|p| p.iter())
    }

    /// Les pages, tronquées à la longueur : chacune est CONTIGUË, et c'est
    /// ce que l'envoi au GPU prend, page par page.
    pub fn pages(&self) -> impl Iterator<Item = &[T]> + '_ {
        let n = self.len;
        self.pages
            .iter()
            .enumerate()
            .map(move |(k, p)| &p[..(n - k * PAGE).min(PAGE)])
            .filter(|p| !p.is_empty())
    }

    /// Les tranches contiguës qui couvrent `debut..fin`, avec leur indice de
    /// départ — une par page traversée.
    pub fn plage(&self, debut: usize, fin: usize) -> impl Iterator<Item = (usize, &[T])> + '_ {
        let fin = fin.min(self.len);
        let mut i = debut;
        std::iter::from_fn(move || {
            if i >= fin {
                return None;
            }
            let (p, o) = (i / PAGE, i % PAGE);
            let n = (PAGE - o).min(fin - i);
            let d = i;
            i += n;
            Some((d, &self.pages[p][o..o + n]))
        })
    }

    /// Octets occupés par la mémoire réservée — pages entières comprises.
    pub fn octets_reserves(&self) -> usize {
        self.pages.len() * PAGE * std::mem::size_of::<T>()
    }
}

impl<T: Copy> Index<usize> for Pages<T> {
    type Output = T;
    #[inline]
    fn index(&self, i: usize) -> &T {
        debug_assert!(i < self.len, "indice {i} au-delà de {}", self.len);
        &self.pages[i / PAGE][i % PAGE]
    }
}

impl<T: Copy> IndexMut<usize> for Pages<T> {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut T {
        debug_assert!(i < self.len, "indice {i} au-delà de {}", self.len);
        &mut self.pages[i / PAGE][i % PAGE]
    }
}
