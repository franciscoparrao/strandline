//! Tipos de error del crate.

use thiserror::Error;

/// Errores de strandline-core.
#[derive(Debug, Error)]
pub enum StrandlineError {
    /// Error de I/O o formato propagado desde surtgis-core.
    #[error("surtgis error: {0}")]
    Surtgis(#[from] surtgis_core::error::Error),

    /// Error de I/O al leer/escribir archivos propios (CSV, GeoJSON).
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// Ruta del archivo involucrado.
        path: String,
        /// Error subyacente.
        #[source]
        source: std::io::Error,
    },

    /// Contenido malformado en un archivo de entrada (CSV, timestamps).
    #[error("parse error: {0}")]
    Parse(String),

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

    /// CRS no soportado para la operación (geográfico donde se requiere
    /// proyectado, o EPSG sin transformación disponible).
    #[error("unsupported CRS: {0}")]
    UnsupportedCrs(String),

    /// No hay píxeles válidos para calcular el umbral.
    #[error("no valid (finite) pixels to threshold")]
    NoValidPixels,
}

/// Alias de resultado del crate.
pub type Result<T> = std::result::Result<T, StrandlineError>;
