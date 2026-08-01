//! Umbral de Otsu, réplica exacta de `skimage.filters.threshold_otsu`
//! (histograma de 256 bins uniformes entre min y max de los valores finitos,
//! umbral = centro del bin que maximiza la varianza inter-clase).

use crate::error::{Result, StrandlineError};

const NBINS: usize = 256;

/// Otsu ponderado: cada muestra `(valor, peso)` contribuye `peso` al
/// histograma en vez de 1. Base compartida por [`otsu_threshold`] (todos
/// los pesos en 1.0) y [`balanced_otsu`] (pesos normalizados por clase).
pub fn otsu_threshold_weighted(pairs: impl Iterator<Item = (f32, f64)>) -> Result<f32> {
    let vals: Vec<(f64, f64)> = pairs
        .filter(|(v, w)| v.is_finite() && *w > 0.0)
        .map(|(v, w)| (v as f64, w))
        .collect();
    if vals.is_empty() {
        return Err(StrandlineError::NoValidPixels);
    }
    let min = vals.iter().map(|(v, _)| *v).fold(f64::INFINITY, f64::min);
    let max = vals
        .iter()
        .map(|(v, _)| *v)
        .fold(f64::NEG_INFINITY, f64::max);
    if min == max {
        return Ok(min as f32);
    }

    let mut counts = [0f64; NBINS];
    let scale = NBINS as f64 / (max - min);
    for (v, w) in &vals {
        let idx = (((*v - min) * scale) as usize).min(NBINS - 1);
        counts[idx] += w;
    }
    let bin_width = (max - min) / NBINS as f64;
    let centers: Vec<f64> = (0..NBINS)
        .map(|i| min + (i as f64 + 0.5) * bin_width)
        .collect();

    // weight1[i] = cumsum(counts)[i]; weight2[i] = suma desde i hasta el final
    let mut weight1 = [0f64; NBINS];
    let mut weight2 = [0f64; NBINS];
    let mut acc = 0.0;
    for i in 0..NBINS {
        acc += counts[i];
        weight1[i] = acc;
    }
    acc = 0.0;
    for i in (0..NBINS).rev() {
        acc += counts[i];
        weight2[i] = acc;
    }

    // mean1[i] = media acumulada hasta i; mean2[i] = media desde i
    let mut mean1 = [0f64; NBINS];
    let mut mean2 = [0f64; NBINS];
    acc = 0.0;
    for i in 0..NBINS {
        acc += counts[i] * centers[i];
        mean1[i] = if weight1[i] > 0.0 { acc / weight1[i] } else { 0.0 };
    }
    acc = 0.0;
    for i in (0..NBINS).rev() {
        acc += counts[i] * centers[i];
        mean2[i] = if weight2[i] > 0.0 { acc / weight2[i] } else { 0.0 };
    }

    // variance12[i] = weight1[i] * weight2[i+1] * (mean1[i] - mean2[i+1])^2
    let mut best_i = 0;
    let mut best_var = f64::NEG_INFINITY;
    for i in 0..NBINS - 1 {
        let var = weight1[i] * weight2[i + 1] * (mean1[i] - mean2[i + 1]).powi(2);
        if var > best_var {
            best_var = var;
            best_i = i;
        }
    }
    Ok(centers[best_i] as f32)
}

/// Calcula el umbral de Otsu sobre los valores finitos de `values`.
pub fn otsu_threshold(values: impl Iterator<Item = f32>) -> Result<f32> {
    otsu_threshold_weighted(values.map(|v| (v, 1.0)))
}

/// Otsu balanceado entre dos clases: cada clase contribuye el mismo peso
/// total al histograma (1.0 cada una) independiente de su tamaño de
/// muestra, para que la clase más numerosa (típicamente agua) no domine
/// el umbral.
///
/// Réplica funcional y determinística del submuestreo aleatorio que usa
/// CoastSat en `find_wl_contours2` (balancea recortando la clase mayor a
/// `np.random.choice`); en el límite de N grande ambos esquemas convergen
/// al mismo umbral, pero éste no depende de una semilla de RNG.
pub fn balanced_otsu(values_a: &[f32], values_b: &[f32]) -> Result<f32> {
    let n_a = values_a.iter().filter(|v| v.is_finite()).count();
    let n_b = values_b.iter().filter(|v| v.is_finite()).count();
    if n_a == 0 || n_b == 0 {
        return Err(StrandlineError::NoValidPixels);
    }
    let w_a = 1.0 / n_a as f64;
    let w_b = 1.0 / n_b as f64;
    let pairs = values_a
        .iter()
        .map(move |&v| (v, w_a))
        .chain(values_b.iter().map(move |&v| (v, w_b)));
    otsu_threshold_weighted(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bimodal_threshold_between_modes() {
        // dos modas claras: alrededor de -0.5 y +0.5
        let vals = (0..500)
            .map(|i| -0.5 + (i % 10) as f32 * 0.01)
            .chain((0..500).map(|i| 0.5 + (i % 10) as f32 * 0.01));
        // el umbral debe caer entre el fin de la moda baja (≈ -0.41) y el
        // inicio de la moda alta (0.5); con plateau, Otsu toma el primer argmax
        let t = otsu_threshold(vals).unwrap();
        assert!((-0.42..0.5).contains(&t), "t = {t}");
    }

    #[test]
    fn nan_ignored() {
        let vals = vec![f32::NAN, 0.0, 0.0, 1.0, 1.0, f32::NAN];
        let t = otsu_threshold(vals.into_iter()).unwrap();
        assert!(t > 0.0 && t < 1.0);
    }

    #[test]
    fn balanced_ignores_class_size_imbalance() {
        // clase A: 9000 muestras concentradas en 0.0; clase B: 10 muestras en 1.0.
        // Sin balancear, Otsu global (peso uniforme) se sesga fuerte hacia 0
        // porque A domina el histograma; balanceado, el umbral debe seguir
        // cayendo entre ambas modas.
        let a: Vec<f32> = vec![0.0; 9000];
        let b: Vec<f32> = vec![1.0; 10];
        let t = balanced_otsu(&a, &b).unwrap();
        assert!(t > 0.0 && t < 1.0, "t = {t}");
    }
}
