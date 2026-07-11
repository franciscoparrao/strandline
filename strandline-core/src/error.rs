//! Tipos de error del crate.

use thiserror::Error;

/// Errores de strandline-core.
#[derive(Debug, Error)]
pub enum StrandlineError {
    /// Error de I/O o formato propagado desde surtgis-core.
    #[error("surtgis error: {0}")]
    Surtgis(#[from] surtgis_core::error::Error),

    /// El raster no tiene las bandas esperadas.
    #[error("band {requested} not available: image has {available} bands")]
    BandOutOfRange {
        /// Banda pedida (1-based).
        requested: usize,
        /// Bandas disponibles.
        available: usize,
    },

    /// Dimensiones incompatibles entre bandas o máscara.
    #[error("shape mismatch: {0}")]
    ShapeMismatch(String),

    /// No hay píxeles válidos para calcular el umbral.
    #[error("no valid (finite) pixels to threshold")]
    NoValidPixels,
}

/// Alias de resultado del crate.
pub type Result<T> = std::result::Result<T, StrandlineError>;
