//! Máscara de nubes desde QA_PIXEL (Landsat Collection 2) y transformada
//! de distancia para el buffer de exclusión alrededor de nubes.
//!
//! Réplica de `SDS_preprocess.create_cloud_mask` + el buffer `dist_clouds`
//! de `SDS_shoreline.process_shoreline` de CoastSat:
//! - bits cirrus (2) y cloud (3) del QA marcan nube;
//! - los blobs de nube menores a `min_size` píxeles se descartan
//!   (4-conectividad, como CoastSat): CFMASK marca erróneamente píxeles de
//!   swash/espuma como nube, y sin este filtro el buffer de exclusión
//!   borraría la playa;
//! - la distancia euclídea a la nube más cercana se calcula con la
//!   transformada exacta de Felzenszwalb & Huttenlocher (2012), O(n).

use ndarray::Array2;
use surtgis_core::raster::Raster;

use crate::classify::remove_small_objects;

/// Bits del QA_PIXEL considerados nube (cirrus y cloud, como CoastSat).
const QA_CLOUD_BITS: u16 = (1 << 2) | (1 << 3);

/// Construye la máscara booleana de nubes desde la banda QA_PIXEL,
/// descartando los blobs menores a `min_size_px` (0 = sin filtro). El
/// filtro solo se aplica si la máscara tiene mezcla de nube y no-nube
/// (mismo guard que CoastSat).
pub fn cloud_mask_from_qa(qa: &Raster<u16>, min_size_px: usize) -> Array2<bool> {
    let (rows, cols) = qa.shape();
    let q = qa.data();
    let mut mask = Array2::from_elem((rows, cols), false);
    for r in 0..rows {
        for c in 0..cols {
            mask[[r, c]] = q[[r, c]] & QA_CLOUD_BITS != 0;
        }
    }
    if min_size_px > 1 {
        let n_cloud = mask.iter().filter(|&&v| v).count();
        if n_cloud > 0 && n_cloud < rows * cols {
            // 4-conectividad: CoastSat usa connectivity=1 para nubes
            remove_small_objects(&mut mask, min_size_px, false);
        }
    }
    mask
}

/// Transformada de distancia euclídea **exacta** (en píxeles) a la celda
/// `true` más cercana de `mask`. Celdas `true` → 0. Si la máscara está
/// vacía, devuelve `f32::INFINITY` en todas partes.
///
/// Algoritmo separable de Felzenszwalb & Huttenlocher (distancia al
/// cuadrado por columnas y luego parábolas por filas), O(rows·cols).
pub fn distance_to_true_px(mask: &Array2<bool>) -> Array2<f32> {
    let (rows, cols) = mask.dim();
    const INF: f64 = f64::INFINITY;

    // paso 1: distancia al cuadrado 1D por columnas (vertical)
    let mut g = vec![INF; rows * cols];
    for c in 0..cols {
        // barrido hacia abajo
        let mut d = INF;
        for r in 0..rows {
            d = if mask[[r, c]] { 0.0 } else { d + 1.0 };
            g[r * cols + c] = d;
        }
        // barrido hacia arriba
        d = INF;
        for r in (0..rows).rev() {
            d = if mask[[r, c]] { 0.0 } else { d + 1.0 };
            let v = &mut g[r * cols + c];
            if d < *v {
                *v = d;
            }
        }
        // a distancia al cuadrado
        for r in 0..rows {
            let v = &mut g[r * cols + c];
            if v.is_finite() {
                *v *= *v;
            }
        }
    }

    // paso 2: envolvente inferior de parábolas por filas (horizontal)
    let mut out = Array2::from_elem((rows, cols), f32::INFINITY);
    let mut v_idx = vec![0usize; cols]; // vértices de las parábolas
    let mut z = vec![0f64; cols + 1]; // fronteras entre parábolas
    for r in 0..rows {
        let f = |i: usize| g[r * cols + i];
        let mut k = 0usize;
        v_idx[0] = 0;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..cols {
            if !f(q).is_finite() {
                continue;
            }
            loop {
                let p = v_idx[k];
                // intersección de la parábola en q con la del vértice p
                let s = if f(p).is_finite() {
                    ((f(q) + (q * q) as f64) - (f(p) + (p * p) as f64))
                        / (2.0 * q as f64 - 2.0 * p as f64)
                } else {
                    -INF
                };
                if s <= z[k] {
                    if k == 0 {
                        v_idx[0] = q;
                        z[1] = INF;
                        break;
                    }
                    k -= 1;
                } else {
                    k += 1;
                    v_idx[k] = q;
                    z[k] = s;
                    z[k + 1] = INF;
                    break;
                }
            }
        }
        let mut k2 = 0usize;
        for q in 0..cols {
            while z[k2 + 1] < q as f64 {
                k2 += 1;
            }
            let p = v_idx[k2];
            let dq = q as f64 - p as f64;
            let d2 = dq * dq + f(p);
            out[[r, q]] = if d2.is_finite() {
                (d2.sqrt()) as f32
            } else {
                f32::INFINITY
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EDT por fuerza bruta para validar la implementación separable.
    fn brute_force(mask: &Array2<bool>) -> Array2<f32> {
        let (rows, cols) = mask.dim();
        let trues: Vec<(f64, f64)> = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (r, c)))
            .filter(|&(r, c)| mask[[r, c]])
            .map(|(r, c)| (r as f64, c as f64))
            .collect();
        let mut out = Array2::from_elem((rows, cols), f32::INFINITY);
        for r in 0..rows {
            for c in 0..cols {
                let mut best = f64::INFINITY;
                for &(tr, tc) in &trues {
                    let d = ((r as f64 - tr).powi(2) + (c as f64 - tc).powi(2)).sqrt();
                    best = best.min(d);
                }
                out[[r, c]] = best as f32;
            }
        }
        out
    }

    #[test]
    fn edt_matches_brute_force() {
        // patrón irregular de nubes
        let mut mask = Array2::from_elem((15, 12), false);
        for &(r, c) in &[(0, 0), (3, 7), (4, 8), (9, 2), (14, 11), (7, 7)] {
            mask[[r, c]] = true;
        }
        let fast = distance_to_true_px(&mask);
        let slow = brute_force(&mask);
        for r in 0..15 {
            for c in 0..12 {
                assert!(
                    (fast[[r, c]] - slow[[r, c]]).abs() < 1e-4,
                    "({r},{c}): {} vs {}",
                    fast[[r, c]],
                    slow[[r, c]]
                );
            }
        }
    }

    #[test]
    fn edt_empty_mask_is_infinite() {
        let mask = Array2::from_elem((4, 4), false);
        let d = distance_to_true_px(&mask);
        assert!(d.iter().all(|v| v.is_infinite()));
    }

    #[test]
    fn small_cloud_blobs_removed() {
        // QA sintética: un blob de 2 px (falso swash) y uno de 50 px (nube real)
        let mut data = vec![0u16; 30 * 30];
        data[5 * 30 + 5] = 1 << 3;
        data[5 * 30 + 6] = 1 << 3;
        for r in 15..20 {
            for c in 10..20 {
                data[r * 30 + c] = 1 << 3;
            }
        }
        let qa = {
            let mut r = Raster::<u16>::from_vec(data, 30, 30).unwrap();
            r.set_nodata(None);
            r
        };
        let mask = cloud_mask_from_qa(&qa, 40);
        assert!(!mask[[5, 5]], "blob chico debió eliminarse");
        assert!(mask[[17, 15]], "nube real debió conservarse");
    }
}
