//! Clasificación de píxeles (sand/whitewater/water/other) vía el MLP
//! portado de CoastSat, réplica de `SDS_shoreline.classify_image_NN`.

use ndarray::Array2;
use surtgis_core::raster::Raster;

use crate::error::Result;
use crate::features::FeatureBank;
use crate::mlp;

/// Resultado de la clasificación: máscaras booleanas por clase, del mismo
/// tamaño que las bandas de entrada. `whitewater` no recibe filtro de área
/// mínima (CoastSat tampoco lo aplica a esa clase).
pub struct Classification {
    /// Máscara de píxeles sand, tras remover blobs menores a `min_area_px`.
    pub sand: Array2<bool>,
    /// Máscara de píxeles whitewater (rompiente/swash).
    pub whitewater: Array2<bool>,
    /// Máscara de píxeles water, tras remover blobs menores a `min_area_px`.
    pub water: Array2<bool>,
}

impl Classification {
    /// Cuenta de píxeles por clase (para reporte/diagnóstico).
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.sand.iter().filter(|&&v| v).count(),
            self.whitewater.iter().filter(|&&v| v).count(),
            self.water.iter().filter(|&&v| v).count(),
        )
    }
}

/// Clasifica los píxeles de una escena en sand/whitewater/water/other.
///
/// `bands` debe tener al menos 5 elementos en orden CoastSat
/// (B,G,R,NIR,SWIR1). `min_area_px` es el tamaño mínimo de blob (en
/// píxeles) para conservar sand/water — réplica de
/// `morphology.remove_small_objects(..., connectivity=2)` (8-conectividad).
pub fn classify_pixels(
    bands: &[Raster<f32>],
    qa: Option<&Raster<u16>>,
    min_area_px: usize,
) -> Result<Classification> {
    let bank = FeatureBank::build(bands, qa)?;
    let (rows, cols) = bank.dims();
    let n = rows * cols;

    // Etiqueta de clase por píxel (`SKIP_LABEL` = inválido/other). Las
    // features se reúnen bajo demanda y se predicen en la misma pasada —
    // sin matriz `(n, 20)` intermedia. El cómputo es independiente por
    // píxel: se paraleliza con rayon bajo la feature `parallel`; el
    // resultado no depende del orden.
    let label_at = |row: usize| -> u8 {
        let (r, c) = (row / cols, row % cols);
        let feat = bank.gather(r, c);
        if feat.iter().any(|v| !v.is_finite()) {
            SKIP_LABEL
        } else {
            mlp::predict(&feat)
        }
    };

    #[cfg(feature = "parallel")]
    let labels: Vec<u8> = {
        use rayon::prelude::*;
        (0..n).into_par_iter().map(label_at).collect()
    };
    #[cfg(not(feature = "parallel"))]
    let labels: Vec<u8> = (0..n).map(label_at).collect();

    let mut sand = Array2::from_elem((rows, cols), false);
    let mut whitewater = Array2::from_elem((rows, cols), false);
    let mut water = Array2::from_elem((rows, cols), false);
    for (idx, &lbl) in labels.iter().enumerate() {
        let (r, c) = (idx / cols, idx % cols);
        match lbl {
            mlp::CLASS_SAND => sand[[r, c]] = true,
            mlp::CLASS_WHITEWATER => whitewater[[r, c]] = true,
            mlp::CLASS_WATER => water[[r, c]] = true,
            _ => {}
        }
    }

    if min_area_px > 1 {
        remove_small_objects(&mut sand, min_area_px);
        remove_small_objects(&mut water, min_area_px);
    }

    Ok(Classification {
        sand,
        whitewater,
        water,
    })
}

/// Etiqueta usada para píxeles inválidos (feature no finita) o de clase
/// `other`; ninguna máscara la recoge.
const SKIP_LABEL: u8 = 255;

/// Elimina componentes conexas (8-conectividad) menores a `min_size`
/// píxeles, réplica de `skimage.morphology.remove_small_objects(...,
/// connectivity=2)`.
pub fn remove_small_objects(mask: &mut Array2<bool>, min_size: usize) {
    let (rows, cols) = mask.dim();
    let mut visited = vec![false; rows * cols];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut component: Vec<(usize, usize)> = Vec::new();

    for r0 in 0..rows {
        for c0 in 0..cols {
            let idx0 = r0 * cols + c0;
            if !mask[[r0, c0]] || visited[idx0] {
                continue;
            }
            component.clear();
            stack.push((r0, c0));
            visited[idx0] = true;
            while let Some((r, c)) = stack.pop() {
                component.push((r, c));
                for dr in -1isize..=1 {
                    for dc in -1isize..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r as isize + dr;
                        let nc = c as isize + dc;
                        if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                            continue;
                        }
                        let (nr, nc) = (nr as usize, nc as usize);
                        let nidx = nr * cols + nc;
                        if mask[[nr, nc]] && !visited[nidx] {
                            visited[nidx] = true;
                            stack.push((nr, nc));
                        }
                    }
                }
            }
            if component.len() < min_size {
                for &(r, c) in &component {
                    mask[[r, c]] = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_small_blob_keeps_large_one() {
        let mut mask = Array2::from_elem((10, 10), false);
        // blob pequeño (2 px, 4-conectado)
        mask[[0, 0]] = true;
        mask[[0, 1]] = true;
        // blob grande (9 px, cuadrado 3x3)
        for r in 5..8 {
            for c in 5..8 {
                mask[[r, c]] = true;
            }
        }
        remove_small_objects(&mut mask, 5);
        assert!(!mask[[0, 0]]);
        assert!(!mask[[0, 1]]);
        assert!(mask[[6, 6]]);
    }

    #[test]
    fn diagonal_connectivity_merges_components() {
        let mut mask = Array2::from_elem((5, 5), false);
        // cadena diagonal de 5 px, sólo conexa en 8-conectividad
        for i in 0..5 {
            mask[[i, i]] = true;
        }
        remove_small_objects(&mut mask, 5);
        // con 8-conectividad la cadena entera cuenta como una sola
        // componente de tamaño 5 >= min_size, se conserva
        for i in 0..5 {
            assert!(mask[[i, i]]);
        }
    }
}
