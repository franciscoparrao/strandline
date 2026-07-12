//! Estimación de la pendiente de playa desde las propias series
//! satelitales (S4 del roadmap SOTA; principio de CoastSat.slope,
//! Vos et al. 2020 GRL).
//!
//! Idea: la marea contamina la serie cross-shore con energía en las
//! frecuencias mareales (submuestreadas/aliaseadas por el revisit
//! satelital). Al corregir con la pendiente correcta
//! (`x − tide/slope`), esa energía desaparece; con una pendiente
//! incorrecta, queda residuo. Se barre un rango de pendientes candidatas y
//! se elige la que **minimiza la energía espectral** de la serie corregida
//! en la banda de la frecuencia pico de la marea muestreada. El espectro
//! sobre fechas irregulares se calcula con el periodograma de
//! Lomb-Scargle.
//!
//! Elimina el parámetro más arbitrario de la corrección mareal (la
//! pendiente "de la literatura", 0.10 plano en el paper de referencia) y
//! la vuelve autocalibrada por transecto.

/// Parámetros del barrido de pendientes.
#[derive(Debug, Clone)]
pub struct SlopeSettings {
    /// Pendientes candidatas tan(β). Default: 0.02–0.30 en pasos de 0.0025.
    pub candidates: Vec<f64>,
    /// Semiancho de la banda de frecuencia alrededor del pico mareal,
    /// como múltiplo de la resolución espectral `1/span`. Default: 2.
    pub band_halfwidth_bins: f64,
    /// Mínimo de observaciones válidas.
    pub min_valid: usize,
}

impl Default for SlopeSettings {
    fn default() -> Self {
        Self {
            candidates: (0..=112).map(|i| 0.02 + i as f64 * 0.0025).collect(),
            band_halfwidth_bins: 2.0,
            min_valid: 30,
        }
    }
}

/// Resultado de la estimación para un transecto.
#[derive(Debug, Clone)]
pub struct SlopeEstimate {
    /// Pendiente que minimiza la energía mareal residual.
    pub slope: f64,
    /// Frecuencia pico de la marea muestreada (ciclos/día).
    pub peak_freq_cpd: f64,
    /// Energía residual normalizada en el óptimo (energía con el slope
    /// óptimo / energía sin corrección); valores ≪ 1 indican señal mareal
    /// fuerte bien removida.
    pub energy_ratio: f64,
    /// Observaciones usadas.
    pub n: usize,
}

/// Periodograma de Lomb-Scargle (Lomb 1976, Scargle 1982) para series con
/// muestreo irregular. `t` en días, `freqs` en ciclos/día. La serie se
/// centra internamente; normalización estándar (por 2σ²·N no aplicada — se
/// usa la potencia bruta, suficiente para comparar energías).
pub fn lomb_scargle(t: &[f64], y: &[f64], freqs: &[f64]) -> Vec<f64> {
    let n = t.len();
    let mean = y.iter().sum::<f64>() / n as f64;
    let yc: Vec<f64> = y.iter().map(|v| v - mean).collect();

    freqs
        .iter()
        .map(|&f| {
            let omega = 2.0 * std::f64::consts::PI * f;
            // desfase τ de Lomb
            let (mut s2, mut c2) = (0.0, 0.0);
            for &ti in t {
                s2 += (2.0 * omega * ti).sin();
                c2 += (2.0 * omega * ti).cos();
            }
            let tau = s2.atan2(c2) / (2.0 * omega);
            let (mut cy, mut sy, mut cc, mut ss) = (0.0, 0.0, 0.0, 0.0);
            for (&ti, &yi) in t.iter().zip(&yc) {
                let arg = omega * (ti - tau);
                let (sn, cs) = arg.sin_cos();
                cy += yi * cs;
                sy += yi * sn;
                cc += cs * cs;
                ss += sn * sn;
            }
            let mut p = 0.0;
            if cc > 0.0 {
                p += cy * cy / cc;
            }
            if ss > 0.0 {
                p += sy * sy / ss;
            }
            0.5 * p
        })
        .collect()
}

/// Grilla de frecuencias para el análisis: desde `2/span` hasta la
/// pseudo-Nyquist del muestreo mediano, con paso `1/(oversample·span)`.
fn frequency_grid(t: &[f64], oversample: f64) -> Vec<f64> {
    let span = t[t.len() - 1] - t[0];
    // dt mediano
    let mut dts: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).collect();
    dts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let dt_med = dts[dts.len() / 2].max(1.0);
    let f_max = 0.5 / dt_med;
    let df = 1.0 / (oversample * span);
    let f_min = 2.0 / span;
    let n = ((f_max - f_min) / df) as usize;
    (0..n).map(|i| f_min + i as f64 * df).collect()
}

/// Frecuencia pico (ciclos/día) de la marea muestreada en los instantes de
/// las imágenes — la firma aliaseada del forzante mareal que la corrección
/// debe remover.
pub fn peak_tidal_frequency(t_days: &[f64], tide_m: &[f64]) -> Option<f64> {
    if t_days.len() < 8 {
        return None;
    }
    let freqs = frequency_grid(t_days, 4.0);
    if freqs.is_empty() {
        return None;
    }
    let power = lomb_scargle(t_days, tide_m, &freqs);
    let (mut best_i, mut best_p) = (0usize, f64::NEG_INFINITY);
    for (i, &p) in power.iter().enumerate() {
        if p > best_p {
            best_p = p;
            best_i = i;
        }
    }
    Some(freqs[best_i])
}

/// Estima la pendiente de playa de un transecto minimizando la energía
/// mareal residual de la serie corregida.
///
/// - `t_days`: instantes de las observaciones (días, p. ej. MJD).
/// - `chainage_m`: posición cross-shore (m); `NaN` = faltante.
/// - `tide_m`: elevación de marea (m) en cada instante.
pub fn estimate_beach_slope(
    t_days: &[f64],
    chainage_m: &[f64],
    tide_m: &[f64],
    settings: &SlopeSettings,
) -> Option<SlopeEstimate> {
    assert_eq!(t_days.len(), chainage_m.len());
    assert_eq!(t_days.len(), tide_m.len());

    // observaciones válidas
    let mut t = Vec::new();
    let mut x = Vec::new();
    let mut h = Vec::new();
    for i in 0..t_days.len() {
        if chainage_m[i].is_finite() && tide_m[i].is_finite() {
            t.push(t_days[i]);
            x.push(chainage_m[i]);
            h.push(tide_m[i]);
        }
    }
    let n = t.len();
    if n < settings.min_valid {
        return None;
    }

    let peak = peak_tidal_frequency(&t, &h)?;
    let span = t[n - 1] - t[0];
    let df = 1.0 / span;
    let half = settings.band_halfwidth_bins * df;
    // banda de evaluación alrededor del pico, con paso fino
    let band: Vec<f64> = (-8..=8)
        .map(|k| peak + k as f64 * half / 8.0)
        .filter(|f| *f > 0.0)
        .collect();

    // de-tendencia lineal de x (la tendencia erosiva no debe influir)
    let tbar = t.iter().sum::<f64>() / n as f64;
    let xbar = x.iter().sum::<f64>() / n as f64;
    let (mut stt, mut stx) = (0.0, 0.0);
    for i in 0..n {
        stt += (t[i] - tbar) * (t[i] - tbar);
        stx += (t[i] - tbar) * (x[i] - xbar);
    }
    let trend = if stt > 0.0 { stx / stt } else { 0.0 };
    let x_dt: Vec<f64> = (0..n).map(|i| x[i] - trend * (t[i] - tbar)).collect();

    let band_energy = |series: &[f64]| -> f64 {
        lomb_scargle(&t, series, &band).iter().sum()
    };
    let e_uncorrected = band_energy(&x_dt);
    if e_uncorrected <= 0.0 {
        return None;
    }

    let mut best = (f64::INFINITY, settings.candidates[0]);
    for &slope in &settings.candidates {
        let corrected: Vec<f64> = (0..n).map(|i| x_dt[i] - h[i] / slope).collect();
        let e = band_energy(&corrected);
        if e < best.0 {
            best = (e, slope);
        }
    }

    Some(SlopeEstimate {
        slope: best.1,
        peak_freq_cpd: peak,
        energy_ratio: best.0 / e_uncorrected,
        n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_known_slope_landsat_sampling() {
        // Serie sintética con TODO realista: marea generada con el propio
        // predictor armónico (M2+S2+K1+O1 de la costa central de Chile),
        // muestreo tipo Landsat (16 días + jitter), tendencia erosiva y
        // ruido de posición. La estimación debe recuperar el slope usado.
        use crate::tide::harmonic::{Constituent, TidePredictor};
        let (pred, _) = TidePredictor::new(
            &[
                Constituent { name: "M2", amplitude: 0.42, phase_g: 95.0 },
                Constituent { name: "S2", amplitude: 0.14, phase_g: 130.0 },
                Constituent { name: "K1", amplitude: 0.15, phase_g: 330.0 },
                Constituent { name: "O1", amplitude: 0.10, phase_g: 300.0 },
            ],
            0.0,
        );
        let mut seed = 99u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed as f64 / u64::MAX as f64) - 0.5
        };
        for &slope_true in &[0.06, 0.10, 0.15] {
            let mut t = Vec::new();
            let mut x = Vec::new();
            let mut h = Vec::new();
            let mut mjd = 46100.6; // ~1985, hora de pasada fija + jitter
            for _ in 0..500 {
                mjd += 16.0 + (rnd() * 0.02); // revisit Landsat
                let tide = pred.predict(mjd);
                let years = (mjd - 46100.0) / 365.25;
                t.push(mjd);
                h.push(tide);
                // posición: tendencia −0.5 m/año + marea/slope + ruido 3 m
                x.push(-0.5 * years + tide / slope_true + 3.0 * rnd());
            }
            let est = estimate_beach_slope(&t, &x, &h, &SlopeSettings::default())
                .expect("estimación falló");
            assert!(
                (est.slope - slope_true).abs() <= 0.01,
                "slope_true {slope_true}: estimado {} (ratio {:.3})",
                est.slope,
                est.energy_ratio
            );
            // la corrección con el slope correcto debe remover la mayor
            // parte de la energía mareal
            assert!(est.energy_ratio < 0.35, "ratio = {}", est.energy_ratio);
        }
    }

    #[test]
    fn lomb_scargle_finds_known_frequency() {
        // señal pura de 0.2 ciclos/día muestreada irregularmente
        let mut t = Vec::new();
        let mut y = Vec::new();
        let mut seed = 12345u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as f64 / u64::MAX as f64
        };
        let mut ti = 0.0;
        for _ in 0..200 {
            ti += 5.0 + 10.0 * rnd();
            t.push(ti);
            y.push((2.0 * std::f64::consts::PI * 0.2 * ti).sin());
        }
        let freqs: Vec<f64> = (1..500).map(|i| i as f64 * 0.001).collect();
        let p = lomb_scargle(&t, &y, &freqs);
        let imax = p
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        // pseudo-Nyquist del muestreo (~dt 10 días) → 0.2 queda aliased,
        // pero la grilla llega a 0.5: el pico debe estar en 0.2 ± df
        assert!(
            (freqs[imax] - 0.2).abs() < 0.005,
            "pico en {} cpd",
            freqs[imax]
        );
    }
}
