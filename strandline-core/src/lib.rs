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

pub mod baseline;
pub mod classify;
pub mod cloud;
pub mod contour;
pub mod error;
pub mod features;
pub mod index;
pub mod mlp;
pub mod otsu;
pub mod proj;
pub mod qc;
pub mod rates;
pub mod satellite_weights;
pub mod shoreline;
pub mod slope;
pub mod stl;
pub mod subpixel;
pub mod tide;
pub mod transects;

pub use classify::Classification;
pub use error::StrandlineError;
pub use proj::transform_points;
pub use qc::{classify, evaluate as qc_evaluate, ChangeClass, QcCriteria, QcReason};
pub use rates::{compute_rates, Rates};
pub use satellite_weights::{calibrate as calibrate_satellite_weights, SatelliteWeight};
pub use shoreline::{extract_shoreline, ExtractParams, Shoreline};
pub use slope::{estimate_beach_slope, SlopeEstimate, SlopeSettings};
pub use stl::{decompose as stl_decompose, StlResult};
pub use subpixel::{fit_profile, fit_profiles, refine_line_subpixel, ProfileFit, ProfileSettings};
pub use tide::{correct_chainage, TideCorrection};
pub use transects::{compute_intersections, generate_transects, IntersectionSettings, Transect};
