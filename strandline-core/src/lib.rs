//! # strandline-core
//!
//! Extracción y análisis de líneas de costa desde imágenes satelitales.
//!
//! Pipeline V1: GeoTIFF multiespectral → MNDWI → umbral Otsu →
//! marching squares con interpolación subpixel → shoreline georeferenciada.
//!
//! Convención de georeferenciación: idéntica a CoastSat
//! (`SDS_tools.convert_pix2world`): el geotransform GDAL se aplica
//! directamente a los índices fraccionales `(col, row)` del contorno,
//! sin offset de medio píxel.

pub mod classify;
pub mod contour;
pub mod error;
pub mod features;
pub mod index;
pub mod mlp;
pub mod otsu;
pub mod shoreline;

pub use classify::Classification;
pub use error::StrandlineError;
pub use shoreline::{extract_shoreline, ExtractParams, Shoreline};
