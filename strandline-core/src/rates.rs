//! Tasas de cambio costero (DSAS) por transecto.
//!
//! A partir de una serie temporal de posiciones cross-shore (chainage)
//! calcula las métricas estándar de DSAS/CoastSat:
//! - **NSM** (Net Shoreline Movement): desplazamiento neto entre la primera
//!   y la última observación.
//! - **EPR** (End Point Rate): NSM dividido por el lapso temporal.
//! - **SCE** (Shoreline Change Envelope): rango total (max − min).
//! - **LRR** (Linear Regression Rate): pendiente de la regresión lineal
//!   ordinaria, con R² e intervalo de confianza al 95%.
//! - **WLR** (Weighted Linear Regression): pendiente de la regresión lineal
//!   ponderada por la incertidumbre de cada observación, con IC 95%.
//!
//! El tiempo se recibe en **años decimales**; la conversión desde fecha se
//! ofrece en [`decimal_year_from_mjd`] con la convención del pipeline de
//! referencia (día calendario, 365.2425 días/año).

/// Resultado de las tasas de un transecto.
#[derive(Debug, Clone, Copy)]
pub struct Rates {
    /// Observaciones válidas usadas.
    pub n_valid: usize,
    /// Lapso temporal entre la primera y última observación (años).
    pub years: f64,
    /// Net Shoreline Movement (m).
    pub nsm: f64,
    /// Shoreline Change Envelope (m).
    pub sce: f64,
    /// End Point Rate (m/año).
    pub epr: f64,
    /// Linear Regression Rate (m/año).
    pub lrr: f64,
    /// Coeficiente de determinación R² de la regresión LRR.
    pub lrr_r2: f64,
    /// Intervalo de confianza al 95% del LRR (m/año).
    pub lrr_ci95: f64,
    /// Weighted Linear Regression Rate (m/año).
    pub wlr: f64,
    /// Intervalo de confianza al 95% del WLR (m/año).
    pub wlr_ci95: f64,
}

/// Convierte un MJD a año decimal con la convención del pipeline de
/// referencia: se usa el **día calendario** (se descarta la hora, como
/// `datetime.toordinal`) y 365.2425 días/año. MJD 40587 = 1970-01-01.
#[inline]
pub fn decimal_year_from_mjd(mjd: f64) -> f64 {
    1970.0 + (mjd.floor() - 40587.0) / 365.2425
}

/// Elimina outliers aislados de una serie de chainage (despiking), réplica
/// del preproceso del pipeline de referencia antes de calcular tasas.
///
/// Sobre las observaciones válidas (finitas), en orden temporal, remueve
/// iterativamente el primer punto que constituya un salto mayor a
/// `max_change` (m) respecto a sus vecinos:
/// - extremos: si el salto al único vecino supera `max_change`;
/// - interiores: si los saltos a ambos vecinos superan `max_change` y tienen
///   el mismo signo (pico o valle aislado).
///
/// Devuelve la serie con los outliers puestos a `NaN`. Si hay menos de
/// `min_valid` observaciones válidas, la devuelve sin cambios.
pub fn despike(chainage: &[f64], max_change: f64, min_valid: usize) -> Vec<f64> {
    let mut out = chainage.to_vec();
    // índices válidos y sus valores, en orden
    let mut idx: Vec<usize> = (0..out.len()).filter(|&i| out[i].is_finite()).collect();
    if idx.len() < min_valid {
        return out;
    }
    let mut vals: Vec<f64> = idx.iter().map(|&i| out[i]).collect();

    loop {
        let n = vals.len();
        let mut remove: Option<usize> = None;
        for k in 0..n {
            let hit = if k == 0 {
                n > 1 && (vals[0] - vals[1]).abs() > max_change
            } else if k == n - 1 {
                (vals[n - 1] - vals[n - 2]).abs() > max_change
            } else {
                let d_prev = vals[k] - vals[k - 1];
                let d_next = vals[k] - vals[k + 1];
                d_prev.abs() > max_change
                    && d_next.abs() > max_change
                    && d_prev.signum() == d_next.signum()
            };
            if hit {
                remove = Some(k);
                break;
            }
        }
        match remove {
            Some(k) => {
                out[idx[k]] = f64::NAN;
                idx.remove(k);
                vals.remove(k);
            }
            None => break,
        }
    }
    out
}

/// Calcula las tasas DSAS de una serie de chainage.
///
/// - `times_years`: años decimales de cada observación (paralelo a `chainage`).
/// - `chainage`: posición cross-shore (m); `NaN` marca observaciones ausentes.
/// - `weights`: peso de cada observación para el WLR (típicamente
///   `1/incertidumbre`); si es `None`, el WLR usa pesos uniformes y coincide
///   con el LRR.
/// - `min_valid`: mínimo de observaciones válidas para calcular tasas.
///
/// Devuelve `None` si hay menos de `min_valid` observaciones finitas o si el
/// lapso temporal es nulo.
pub fn compute_rates(
    times_years: &[f64],
    chainage: &[f64],
    weights: Option<&[f64]>,
    min_valid: usize,
) -> Option<Rates> {
    assert_eq!(times_years.len(), chainage.len());
    if let Some(w) = weights {
        assert_eq!(w.len(), chainage.len());
    }

    // observaciones válidas, en orden temporal de entrada
    let mut t = Vec::new();
    let mut y = Vec::new();
    let mut w = Vec::new();
    for i in 0..chainage.len() {
        if chainage[i].is_finite() {
            t.push(times_years[i]);
            y.push(chainage[i]);
            w.push(weights.map(|ws| ws[i]).unwrap_or(1.0));
        }
    }
    let n = y.len();
    if n < min_valid {
        return None;
    }

    let years = t[n - 1] - t[0];
    if years == 0.0 {
        return None;
    }
    let nsm = y[n - 1] - y[0];
    let epr = nsm / years;
    let (mut ymin, mut ymax) = (y[0], y[0]);
    for &v in &y {
        ymin = ymin.min(v);
        ymax = ymax.max(v);
    }
    let sce = ymax - ymin;

    let (lrr, lrr_r2, lrr_ci95) = ordinary_regression(&t, &y);
    let (wlr, wlr_ci95) = weighted_regression(&t, &y, &w);

    Some(Rates {
        n_valid: n,
        years,
        nsm,
        sce,
        epr,
        lrr,
        lrr_r2,
        lrr_ci95,
        wlr,
        wlr_ci95,
    })
}

/// Regresión lineal ordinaria `y = a·t + b`. Devuelve `(slope, r², ci95)`
/// con `ci95 = 1.96·std_err(slope)`, réplica de `scipy.stats.linregress`.
fn ordinary_regression(t: &[f64], y: &[f64]) -> (f64, f64, f64) {
    let n = t.len() as f64;
    let tbar = t.iter().sum::<f64>() / n;
    let ybar = y.iter().sum::<f64>() / n;
    let mut stt = 0.0;
    let mut syy = 0.0;
    let mut sty = 0.0;
    for i in 0..t.len() {
        let dt = t[i] - tbar;
        let dy = y[i] - ybar;
        stt += dt * dt;
        syy += dy * dy;
        sty += dt * dy;
    }
    if stt == 0.0 {
        return (0.0, 0.0, f64::NAN);
    }
    let slope = sty / stt;
    let r2 = if syy > 0.0 {
        (sty * sty) / (stt * syy)
    } else {
        0.0
    };
    // error estándar del slope (scipy: sqrt((1-r²)·syy/(n-2) / stt))
    let std_err = if n > 2.0 {
        (((1.0 - r2) * syy / (n - 2.0)) / stt).sqrt()
    } else {
        f64::NAN
    };
    (slope, r2, 1.96 * std_err)
}

/// Regresión lineal ponderada (mínimos cuadrados con pesos). Devuelve
/// `(slope, ci95)` con `ci95 = 1.96·sqrt(cov[0,0])`. Resuelve el sistema
/// 2×2 `AᵀWA·β = AᵀWy` de forma cerrada.
fn weighted_regression(t: &[f64], y: &[f64], w: &[f64]) -> (f64, f64) {
    let n = t.len();
    let (mut sw, mut swt, mut swt2, mut swy, mut swty) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let (wi, ti, yi) = (w[i], t[i], y[i]);
        sw += wi;
        swt += wi * ti;
        swt2 += wi * ti * ti;
        swy += wi * yi;
        swty += wi * ti * yi;
    }
    let det = swt2 * sw - swt * swt;
    if det == 0.0 {
        return (f64::NAN, f64::NAN);
    }
    // β = (AᵀWA)⁻¹ AᵀWy ; slope es la primera componente
    let slope = (sw * swty - swt * swy) / det;
    let intercept = (swt2 * swy - swt * swty) / det;
    if n <= 2 {
        return (slope, f64::NAN);
    }
    // MSE ponderado y covarianza del slope: cov = mse·(AᵀWA)⁻¹, con
    // (AᵀWA)⁻¹[0,0] = sw/det.
    let mut wsse = 0.0;
    for i in 0..n {
        let resid = y[i] - (slope * t[i] + intercept);
        wsse += w[i] * resid * resid;
    }
    let mse = wsse / (n as f64 - 2.0);
    let var_slope = mse * sw / det;
    (slope, 1.96 * var_slope.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_line_recovers_slope() {
        let t: Vec<f64> = (0..10).map(|i| 2000.0 + i as f64).collect();
        let y: Vec<f64> = t.iter().map(|&x| 3.0 * (x - 2000.0) + 5.0).collect();
        let r = compute_rates(&t, &y, None, 5).unwrap();
        assert!((r.lrr - 3.0).abs() < 1e-9);
        assert!((r.epr - 3.0).abs() < 1e-9);
        assert!((r.nsm - 27.0).abs() < 1e-9);
        assert!((r.sce - 27.0).abs() < 1e-9);
        assert!((r.lrr_r2 - 1.0).abs() < 1e-9);
        assert!(r.lrr_ci95 < 1e-6); // ajuste perfecto → sin incertidumbre
        // WLR con pesos uniformes coincide con LRR
        assert!((r.wlr - r.lrr).abs() < 1e-9);
    }

    #[test]
    fn nan_observations_ignored() {
        let t = vec![2000.0, 2001.0, 2002.0, 2003.0];
        let y = vec![0.0, f64::NAN, 20.0, 30.0];
        let r = compute_rates(&t, &y, None, 3).unwrap();
        assert_eq!(r.n_valid, 3);
        assert!((r.nsm - 30.0).abs() < 1e-9);
        assert!((r.years - 3.0).abs() < 1e-9);
    }

    #[test]
    fn too_few_points_returns_none() {
        let t = vec![2000.0, 2001.0];
        let y = vec![0.0, 10.0];
        assert!(compute_rates(&t, &y, None, 10).is_none());
    }

    #[test]
    fn despike_removes_isolated_spike() {
        // pico aislado en el medio: 0,1,2, 100, 4,5 → el 100 se remueve
        let series = vec![0.0, 1.0, 2.0, 100.0, 4.0, 5.0, 6.0];
        let out = despike(&series, 40.0, 3);
        assert!(out[3].is_nan());
        assert!(out.iter().enumerate().all(|(i, v)| i == 3 || v.is_finite()));
    }

    #[test]
    fn despike_keeps_monotonic_trend() {
        // tendencia suave sin saltos > max_change → nada se remueve
        let series: Vec<f64> = (0..20).map(|i| i as f64 * 5.0).collect();
        let out = despike(&series, 40.0, 3);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn decimal_year_epochs() {
        // MJD 40587 = 1970-01-01
        assert!((decimal_year_from_mjd(40587.0) - 1970.0).abs() < 1e-9);
        // 2020-01-10 (MJD 58858.x) → año 2020.02...
        let yr = decimal_year_from_mjd(58858.6);
        assert!((yr - 2020.0).abs() < 0.03, "yr = {yr}");
    }
}
