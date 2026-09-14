//! L'appareil de rendu, **sans fenêtre**.
//!
//! Un moteur qui ne sait dessiner que dans une fenêtre ne se teste pas : il
//! faut un écran, un serveur graphique, et un humain pour regarder. Ici la
//! cible par défaut est une TEXTURE qu'on relit en mémoire — donc une image
//! qu'un test peut comparer au pixel près, et qu'une intégration continue peut
//! produire sans écran.
//!
//! La fenêtre viendra par-dessus : c'est la même chaîne, avec une chaîne
//! d'échange à la place de la texture.

use std::sync::Arc;

#[derive(Debug)]
pub enum AppareilError {
    AucunAdaptateur,
    Peripherique(String),
}

impl std::fmt::Display for AppareilError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppareilError::AucunAdaptateur => write!(
                f,
                "aucun adaptateur graphique : ni carte, ni pilote logiciel (lavapipe)"
            ),
            AppareilError::Peripherique(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AppareilError {}

pub struct Appareil {
    pub instance: wgpu::Instance,
    pub adaptateur: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

impl Appareil {
    /// Ouvre le meilleur adaptateur disponible.
    ///
    /// On accepte un pilote LOGICIEL en dernier recours, et c'est délibéré :
    /// sans lui, rien ne se teste hors d'une machine à écran. Les images par
    /// seconde mesurées dessus ne valent rien — c'est du rastériseur logiciel —
    /// mais le nombre d'appels de dessin et la JUSTESSE de l'image, eux, sont
    /// transposables.
    pub fn ouvrir() -> Result<Appareil, AppareilError> {
        pollster::block_on(Self::ouvrir_async())
    }

    pub async fn ouvrir_async() -> Result<Appareil, AppareilError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adaptateur = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                // Un pilote logiciel vaut mieux que pas de rendu du tout.
                force_fallback_adapter: false,
            })
            .await
            .ok_or(AppareilError::AucunAdaptateur)?;

        let (device, queue) = adaptateur
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("titiforge"),
                    required_features: wgpu::Features::empty(),
                    // Les limites de l'adaptateur, pas celles par défaut : une
                    // carte dédiée porte des tampons bien plus gros, et se
                    // brider dessus plafonnerait l'arène sans raison.
                    required_limits: adaptateur.limits(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| AppareilError::Peripherique(e.to_string()))?;

        Ok(Appareil {
            instance,
            adaptateur,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }

    /// De quoi on dispose vraiment. Sert aux messages et aux mesures.
    pub fn decrire(&self) -> String {
        let i = self.adaptateur.get_info();
        format!("{} · {:?} · {:?}", i.name, i.backend, i.device_type)
    }
}
