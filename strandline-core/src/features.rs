//! Vector de 20 features por píxel para el clasificador MLP, réplica de
//! `SDS_shoreline.calculate_features` de CoastSat.
//!
//! Orden (idéntico al de entrenamiento del modelo portado en `mlp.rs`):
//! 0–4 bandas B,G,R,NIR,SWIR1 crudas; 5–9 índices NIR−G, SWIR−G, NIR−R,
//! SWIR−NIR, B−R (convención CoastSat, ver [`nd_index`](crate::index::nd_index));
//! 10–14 desviación estándar local (ventana 3×3) de las 5 bandas; 15–19
//! desviación estándar local de los 5 índices.

use surtgis_core::ndarray::Array2;
use surtgis_core::raster::Raster;

use crate::error::{Result, StrandlineError};
use crate::index::nd_index;

/// Número de features del vector (fijo, arquitectura del MLP portado).
pub const N_FEATURES: usize = 20;

/// Arrays componente de las features, calculados una sola vez por escena.
///
/// Evita materializar la matriz `(rows*cols, 20)` intermedia: el
/// clasificador reúne las 20 features de cada píxel bajo demanda con
/// [`FeatureBank::gather`], fusionando el cómputo de features con la
/// predicción en una sola pasada (paralela) sobre los píxeles.
pub struct FeatureBank<'a> {
    bands: &'a [Raster<f32>],
    /// Índices NIR−G, SWIR−G, NIR−R, SWIR−NIR, B−R (orden de entrenamiento).
    indices: [Array2<f32>; 5],
    /// Desviación estándar local 3×3 de las 5 bandas.
    band_std: [Array2<f32>; 5],
    /// Desviación estándar local 3×3 de los 5 índices.
    index_std: [Array2<f32>; 5],
}

impl<'a> FeatureBank<'a> {
    /// Construye el banco: 5 índices + 10 desviaciones estándar locales.
    /// `bands` debe tener al menos 5 elementos (orden CoastSat B,G,R,NIR,SWIR1).
    pub fn build(bands: &'a [Raster<f32>], qa: Option<&Raster<u16>>) -> Result<Self> {
        if bands.len() < 5 {
            return Err(StrandlineError::BandOutOfRange {
                requested: 5,
                available: bands.len(),
            });
        }
        let indices = [
            nd_index(&bands[3], &bands[1], qa).into_array(), // NIR−G
            nd_index(&bands[4], &bands[1], qa).into_array(), // SWIR−G
            nd_index(&bands[3], &bands[2], qa).into_array(), // NIR−R
            nd_index(&bands[4], &bands[3], qa).into_array(), // SWIR−NIR
            nd_index(&bands[0], &bands[2], qa).into_array(), // B−R
        ];
        let band_std = std::array::from_fn(|b| local_std(bands[b].data()));
        let index_std = std::array::from_fn(|k| local_std(&indices[k]));
        Ok(Self {
            bands,
            indices,
            band_std,
            index_std,
        })
    }

    /// Dimensiones `(rows, cols)` de la escena.
    #[inline]
    pub fn dims(&self) -> (usize, usize) {
        self.bands[0].shape()
    }

    /// Reúne el vector de 20 features del píxel `(r, c)`, en el orden de
    /// entrenamiento del MLP.
    #[inline]
    pub fn gather(&self, r: usize, c: usize) -> [f32; N_FEATURES] {
        let mut f = [0f32; N_FEATURES];
        for (b, band) in self.bands.iter().take(5).enumerate() {
            f[b] = band.data()[[r, c]];
        }
        for (k, idx) in self.indices.iter().enumerate() {
            f[5 + k] = idx[[r, c]];
        }
        for (b, std) in self.band_std.iter().enumerate() {
            f[10 + b] = std[[r, c]];
        }
        for (k, std) in self.index_std.iter().enumerate() {
            f[15 + k] = std[[r, c]];
        }
        f
    }
}

/// Calcula el vector de 20 features por píxel como matriz `(rows*cols, 20)`
/// en orden row-major. Retenido para pruebas y usos que necesiten la matriz
/// completa; el clasificador usa [`FeatureBank`] directamente para evitar
/// materializarla.
pub fn compute_features(bands: &[Raster<f32>], qa: Option<&Raster<u16>>) -> Result<Array2<f32>> {
    let bank = FeatureBank::build(bands, qa)?;
    let (rows, cols) = bank.dims();
    let mut out = Array2::<f32>::from_elem((rows * cols, N_FEATURES), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            let feats = bank.gather(r, c);
            let idx = r * cols + c;
            for (k, &v) in feats.iter().enumerate() {
                out[[idx, k]] = v;
            }
        }
    }
    Ok(out)
}

/// Mapea un índice (posiblemente fuera de rango) al índice espejado
/// dentro de `[0, n)`, réplica de `numpy.pad(..., mode='reflect')` sin
/// duplicar el borde. Sólo se usa con desplazamientos de ±1 (ventana 3×3).
#[inline]
fn reflect_index(i: isize, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let n = n as isize;
    let period = 2 * (n - 1);
    let mut m = i % period;
    if m < 0 {
        m += period;
    }
    if m >= n {
        (period - m) as usize
    } else {
        m as usize
    }
}

/// Desviación estándar local en ventana 3×3, NaN-aware (ignora vecinos no
/// finitos, restaura NaN si el píxel central no es finito), con padding
/// por reflexión en los bordes. Réplica funcional de `SDS_tools.image_std`
/// (astropy `convolve` con kernel uniforme, `nan_treatment='interpolate'`,
/// `preserve_nan=True`): para un kernel uniforme, la interpolación NaN-aware
/// de astropy equivale a promediar sólo sobre los vecinos finitos.
///
/// Con la feature `parallel` (default) el cómputo se reparte por filas con
/// rayon. El resultado es idéntico bit-a-bit al camino secuencial: cada
/// píxel se calcula de forma independiente y sin acumulación de orden
/// variable, así que la paralelización no altera el redondeo.
fn local_std(img: &Array2<f32>) -> Array2<f32> {
    let (rows, cols) = img.dim();
    let mut data = vec![f32::NAN; rows * cols];
    let fill_row = |r: usize, row_out: &mut [f32]| {
        for (c, out) in row_out.iter_mut().enumerate() {
            *out = std_at(img, r, c, rows, cols);
        }
    };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        data.par_chunks_mut(cols)
            .enumerate()
            .for_each(|(r, row_out)| fill_row(r, row_out));
    }
    #[cfg(not(feature = "parallel"))]
    {
        data.chunks_mut(cols)
            .enumerate()
            .for_each(|(r, row_out)| fill_row(r, row_out));
    }

    Array2::from_shape_vec((rows, cols), data).expect("local_std: shape mismatch building output")
}

/// Desviación estándar local en un solo píxel `(r, c)` sobre la ventana
/// 3×3 con padding por reflexión. Devuelve `NaN` si el píxel central no es
/// finito o si ningún vecino de la ventana lo es.
#[inline]
fn std_at(img: &Array2<f32>, r: usize, c: usize, rows: usize, cols: usize) -> f32 {
    if !img[[r, c]].is_finite() {
        return f32::NAN;
    }
    let mut sum = 0f64;
    let mut sqsum = 0f64;
    let mut count = 0usize;
    for dr in -1isize..=1 {
        for dc in -1isize..=1 {
            let rr = reflect_index(r as isize + dr, rows);
            let cc = reflect_index(c as isize + dc, cols);
            let v = img[[rr, cc]];
            if v.is_finite() {
                sum += v as f64;
                sqsum += (v as f64) * (v as f64);
                count += 1;
            }
        }
    }
    if count == 0 {
        return f32::NAN;
    }
    let mean = sum / count as f64;
    let sqmean = sqsum / count as f64;
    let var = (sqmean - mean * mean).max(0.0);
    var.sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_index_matches_numpy_reflect() {
        // np.pad([0,1,2,3,4], 1, 'reflect') == [1,0,1,2,3,4,3]
        assert_eq!(reflect_index(-1, 5), 1);
        assert_eq!(reflect_index(5, 5), 3);
        assert_eq!(reflect_index(2, 5), 2);
    }

    #[test]
    fn local_std_zero_on_uniform_field() {
        let img = Array2::<f32>::from_elem((5, 5), 3.0);
        let std = local_std(&img);
        for v in std.iter() {
            assert!((*v).abs() < 1e-6);
        }
    }

    #[test]
    fn local_std_nan_preserved_at_invalid_center() {
        let mut img = Array2::<f32>::from_elem((3, 3), 1.0);
        img[[1, 1]] = f32::NAN;
        let std = local_std(&img);
        assert!(std[[1, 1]].is_nan());
        // los vecinos siguen calculando su std ignorando el centro NaN
        assert!(std[[0, 0]].is_finite());
    }
}
