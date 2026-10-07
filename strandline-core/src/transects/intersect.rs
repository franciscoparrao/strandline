//! Intersección vectorizada shoreline×transecto con control de calidad,
//! réplica del `compute_intersection_QC` de CoastSat (Vos et al.) tal como
//! se vectorizó en el pipeline de referencia del paper de erosión costera.
//!
//! Para cada transecto se calcula, por cada shoreline, la distancia
//! cross-shore (chainage) del cruce: se toman los puntos de la shoreline
//! dentro de una franja de `along_dist` alrededor del eje del transecto, se
//! rotan al sistema shore-normal y se resume su posición con estadísticos
//! robustos, aplicando filtros de calidad.

use super::Transect;

/// Qué hacer cuando una shoreline no pasa el control de calidad en un
/// transecto (típicamente por intersecciones múltiples).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultipleIntersection {
    /// Decide automáticamente por transecto: si una fracción alta de
    /// shorelines tiene dispersión grande, usa el cruce más hacia el mar
    /// (`max`); si no, descarta (`NaN`). Es el default de CoastSat.
    Auto,
    /// Usa siempre el cruce más hacia el mar para los que fallan la calidad.
    Max,
    /// Descarta (NaN) los que fallan la calidad.
    Nan,
}

/// Parámetros de la intersección y su control de calidad. Los defaults son
/// los del pipeline de referencia (Viña del Mar).
///
/// **Nota de semántica (paridad CoastSat)**: la "franja" alrededor del
/// transecto se mide respecto a la **recta infinita** que pasa por
/// `origin`→`end`, no al segmento. Un punto de shoreline más allá del
/// extremo mar del transecto sigue contando mientras esté a menos de
/// `along_dist` del eje y a menos de `max_origin_dist` del `origin` — es el
/// comportamiento exacto de `compute_intersection_QC` de CoastSat, y por
/// eso `max_origin_dist` (y no el largo del transecto) es el límite real
/// de alcance hacia el mar.
#[derive(Debug, Clone, Copy)]
pub struct IntersectionSettings {
    /// Semiancho de la franja alrededor del eje del transecto (m).
    pub along_dist: f64,
    /// Mínimo de puntos para aceptar el cruce.
    pub min_points: usize,
    /// Dispersión máxima (desviación estándar, m) de los chainages.
    pub max_std: f64,
    /// Rango máximo (max − min, m) de los chainages.
    pub max_range: f64,
    /// Chainage mínimo admisible (m); valores más hacia tierra se descartan.
    pub min_chainage: f64,
    /// Manejo de intersecciones múltiples.
    pub multiple_inter: MultipleIntersection,
    /// Fracción de shorelines con dispersión alta que dispara el modo `max`
    /// en `Auto`.
    pub auto_prc: f64,
    /// Distancia máxima al origen del transecto para considerar un punto (m).
    pub max_origin_dist: f64,
}

impl Default for IntersectionSettings {
    fn default() -> Self {
        Self {
            along_dist: 25.0,
            min_points: 3,
            max_std: 15.0,
            max_range: 30.0,
            min_chainage: -100.0,
            multiple_inter: MultipleIntersection::Auto,
            auto_prc: 0.1,
            max_origin_dist: 1000.0,
        }
    }
}

/// Estadísticos NaN-aware de una muestra de chainages: `(std, median, max,
/// min, n_finitos)`. `std` es poblacional (ddof=0), como `np.nanstd`.
/// Devuelve `NaN` (y `n=0`) si no hay valores finitos.
fn nan_stats(vals: &[f64]) -> (f64, f64, f64, f64, usize) {
    let mut finite: Vec<f64> = vals.iter().copied().filter(|v| v.is_finite()).collect();
    let n = finite.len();
    if n == 0 {
        return (f64::NAN, f64::NAN, f64::NAN, f64::NAN, 0);
    }
    let mean = finite.iter().sum::<f64>() / n as f64;
    let var = finite.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
    let std = var.sqrt();
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if n % 2 == 1 {
        finite[n / 2]
    } else {
        (finite[n / 2 - 1] + finite[n / 2]) / 2.0
    };
    (std, median, finite[n - 1], finite[0], n)
}

/// Estadísticos del cruce de una shoreline con un transecto.
struct CrossStats {
    std: f64,
    median: f64,
    max: f64,
    min: f64,
    n: usize,
}

/// Calcula, para un transecto, los estadísticos de cruce de cada shoreline
/// (antes del QC entre shorelines).
fn transect_cross_stats(
    t: &Transect,
    shorelines: &[Vec<(f64, f64)>],
    s: &IntersectionSettings,
) -> Vec<CrossStats> {
    let p1 = t.origin;
    let temp = (t.end.0 - p1.0, t.end.1 - p1.1);
    let seg_len = (temp.0 * temp.0 + temp.1 * temp.1).sqrt();
    let phi = temp.1.atan2(temp.0);
    let (cos_phi, sin_phi) = (phi.cos(), phi.sin());

    let mut out = Vec::with_capacity(shorelines.len());
    let mut chain = Vec::new();
    for sl in shorelines {
        chain.clear();
        for &pt in sl {
            let dp = (pt.0 - p1.0, pt.1 - p1.1);
            let cross = dp.0 * temp.1 - dp.1 * temp.0;
            let d_line = cross.abs() / seg_len;
            let d_origin = (dp.0 * dp.0 + dp.1 * dp.1).sqrt();
            if d_line <= s.along_dist && d_origin <= s.max_origin_dist {
                let x_rot = cos_phi * dp.0 + sin_phi * dp.1;
                chain.push(if x_rot < s.min_chainage {
                    f64::NAN
                } else {
                    x_rot
                });
            }
        }
        let (std, median, max, min, n) = nan_stats(&chain);
        out.push(CrossStats {
            std,
            median,
            max,
            min,
            n,
        });
    }
    out
}

/// Aplica el QC entre shorelines de un transecto y devuelve el chainage
/// final por shoreline (NaN si se descarta).
fn apply_qc(stats: &[CrossStats], s: &IntersectionSettings) -> Vec<f64> {
    let n_shore = stats.len();
    let mut med: Vec<f64> = stats.iter().map(|c| c.median).collect();

    let cond3: Vec<bool> = stats.iter().map(|c| c.n >= s.min_points).collect();
    let good: Vec<bool> = stats
        .iter()
        .zip(&cond3)
        .map(|(c, &c3)| c.std <= s.max_std && (c.max - c.min) <= s.max_range && c3)
        .collect();

    let use_max = |med: &mut [f64]| {
        for i in 0..n_shore {
            if !good[i] {
                med[i] = stats[i].max;
            }
        }
        for i in 0..n_shore {
            if !cond3[i] {
                med[i] = f64::NAN;
            }
        }
    };
    let use_nan = |med: &mut [f64]| {
        for i in 0..n_shore {
            if !good[i] {
                med[i] = f64::NAN;
            }
        }
    };

    match s.multiple_inter {
        MultipleIntersection::Nan => use_nan(&mut med),
        MultipleIntersection::Max => use_max(&mut med),
        MultipleIntersection::Auto => {
            let over = stats.iter().filter(|c| c.std > s.max_std).count();
            let prc_over = over as f64 / n_shore as f64;
            if prc_over > s.auto_prc {
                use_max(&mut med);
            } else {
                use_nan(&mut med);
            }
        }
    }
    med
}

/// Calcula la distancia cross-shore (chainage) de cada shoreline en cada
/// transecto. La salida es indexada `[transecto][shoreline]`; `NaN` marca
/// los cruces descartados por el control de calidad.
///
/// Con la feature `parallel` (default) los transectos se procesan en
/// paralelo (cada uno es independiente).
pub fn compute_intersections(
    shorelines: &[Vec<(f64, f64)>],
    transects: &[Transect],
    settings: &IntersectionSettings,
) -> Vec<Vec<f64>> {
    let per_transect = |t: &Transect| {
        let stats = transect_cross_stats(t, shorelines, settings);
        apply_qc(&stats, settings)
    };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        transects.par_iter().map(per_transect).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        transects.iter().map(per_transect).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_stats_basic() {
        let (std, med, max, min, n) = nan_stats(&[1.0, 2.0, 3.0, f64::NAN]);
        assert_eq!(n, 3);
        assert!((med - 2.0).abs() < 1e-12);
        assert!((max - 3.0).abs() < 1e-12 && (min - 1.0).abs() < 1e-12);
        assert!((std - (2.0f64 / 3.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn straight_shoreline_known_chainage() {
        // transecto horizontal de (100,0)[tierra] a (0,0)[mar]; una shoreline
        // vertical en x=40 cruza el transecto en chainage 60 (medido desde el
        // origin tierra a lo largo del eje hacia el mar).
        let t = Transect {
            origin: (100.0, 0.0),
            end: (0.0, 0.0),
        };
        let shoreline: Vec<(f64, f64)> = (-20..=20).map(|k| (40.0, k as f64)).collect();
        let s = IntersectionSettings {
            along_dist: 25.0,
            min_points: 3,
            ..Default::default()
        };
        let out = compute_intersections(&[shoreline], &[t], &s);
        // chainage = distancia desde origin (100,0) al punto (40,0) = 60
        assert!((out[0][0] - 60.0).abs() < 1e-6, "chainage = {}", out[0][0]);
    }

    #[test]
    fn far_shoreline_gives_nan() {
        // shoreline lejos del transecto → sin puntos en la franja → NaN
        let t = Transect {
            origin: (100.0, 0.0),
            end: (0.0, 0.0),
        };
        let shoreline: Vec<(f64, f64)> = (0..10).map(|k| (5000.0, k as f64)).collect();
        let out = compute_intersections(&[shoreline], &[t], &IntersectionSettings::default());
        assert!(out[0][0].is_nan());
    }
}
