//! Índices espectrales normalizados de diferencia.

use ndarray::Array2;
use surtgis_core::raster::Raster;

/// Índice normalizado de diferencia (a − b) / (a + b), réplica de
/// `SDS_tools.nd_index` de CoastSat.
///
/// Devuelve `NaN` en píxeles inválidos:
/// - alguna de las dos bandas no finita,
/// - ambas bandas exactamente 0 (relleno de borde de escena),
/// - denominador 0,
/// - píxel marcado en `cloud` (máscara de nubes, si se entrega — ver
///   [`crate::cloud::cloud_mask_from_qa`]).
pub fn nd_index(a: &Raster<f32>, b: &Raster<f32>, cloud: Option<&Array2<bool>>) -> Raster<f32> {
    let (rows, cols) = a.shape();
    let mut out: Raster<f32> = a.with_same_meta(rows, cols);
    let av = a.data();
    let bv = b.data();
    let ov = out.data_mut();

    for r in 0..rows {
        for c in 0..cols {
            let x = av[[r, c]];
            let y = bv[[r, c]];
            let cloudy = cloud.map(|m| m[[r, c]]).unwrap_or(false);
            let denom = x + y;
            ov[[r, c]] = if !x.is_finite()
                || !y.is_finite()
                || (x == 0.0 && y == 0.0)
                || denom == 0.0
                || cloudy
            {
                f32::NAN
            } else {
                (x - y) / denom
            };
        }
    }
    out.set_nodata(None);
    out
}

/// MNDWI = (Green − SWIR1) / (Green + SWIR1) (convención textbook).
///
/// Nota: CoastSat usa el signo opuesto internamente, (SWIR1 − Green) /
/// (SWIR1 + Green); ver `coastsat_mwi` para esa convención cuando se
/// necesita compatibilidad bit-a-bit (`ExtractParams::coastsat_compat`).
pub fn mndwi(
    green: &Raster<f32>,
    swir1: &Raster<f32>,
    cloud: Option<&Array2<bool>>,
) -> Raster<f32> {
    nd_index(green, swir1, cloud)
}
