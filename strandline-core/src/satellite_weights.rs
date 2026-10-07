//! Pesos WLR reales por satélite, calibrados desde datos — cierra el
//! hallazgo E1 (documentado desde la adenda 8): el proxy histórico de
//! pesos por fecha de corte (ej. antes/después de 2013-04 para L5→L8) solo
//! vale en sitios Landsat-only con un único salto de sensor, y en
//! cualquier caso es una suposición, no una medición.
//!
//! Se hace *detrend* (OLS) de la serie de cada transecto y se agrupan los
//! residuos por satélite, pooled entre TODOS los transectos (más potencia
//! estadística que calibrar transecto por transecto). El peso de cada
//! satélite es `1/varianza` de sus residuos — WLR es invariante a la
//! escala de los pesos (la pendiente y su IC95 solo dependen de las
//! proporciones relativas, ver `weighted_regression` en `rates.rs`), así
//! que no hace falta normalizar.
//!
//! Validado empíricamente en Viña (adenda 12, datos reales de
//! `/mnt/kingston`): L8 tiene ~3× menos varianza residual que L5/L7/L9
//! combinados (test de Levene, p=0.0001) — un peso uniforme o el proxy de
//! fecha de corte ignora una diferencia real y medible de precisión entre
//! sensores.

use std::collections::{BTreeMap, HashMap};

/// Peso calibrado de un satélite.
#[derive(Debug, Clone)]
pub struct SatelliteWeight {
    /// Nombre del satélite (p. ej. "L5", "L8", "S2").
    pub satname: String,
    /// Número de residuos pooled usados para esta calibración.
    pub n_obs: usize,
    /// Desviación estándar de los residuos (m) — para inspección/reporte,
    /// no se usa directo en el peso (que es 1/varianza).
    pub std_m: f64,
    /// Peso WLR: `1/varianza`.
    pub weight: f64,
}

/// Regresión OLS mínima: devuelve `(slope, intercept)` o `None` si hay
/// menos de 2 puntos o la varianza temporal es nula.
fn ols(t: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let n = t.len() as f64;
    if n < 2.0 {
        return None;
    }
    let tbar = t.iter().sum::<f64>() / n;
    let ybar = y.iter().sum::<f64>() / n;
    let mut stt = 0.0;
    let mut sty = 0.0;
    for i in 0..t.len() {
        let dt = t[i] - tbar;
        stt += dt * dt;
        sty += dt * (y[i] - ybar);
    }
    if stt == 0.0 {
        return None;
    }
    let slope = sty / stt;
    let intercept = ybar - slope * tbar;
    Some((slope, intercept))
}

/// Calibra pesos por satélite desde observaciones reales agrupadas por
/// transecto.
///
/// `by_transect`: nombre de transecto → lista de `(t_year, chainage,
/// satname)`, sin ordenar (se ordena internamente si hace falta — la OLS
/// no requiere orden temporal).
///
/// `min_valid_per_transect`: mínimo de observaciones **finitas** para
/// intentar el detrend de un transecto; los que tienen menos se ignoran (no
/// aportan residuos calibrables, pero no invalidan el resto).
///
/// Las observaciones con `t` o `chainage` no finitos se descartan antes del
/// detrend: el CSV de `intersect` marca con `NaN` las intersecciones que el
/// QC rechaza, así que llegan por diseño. Sin este filtro un solo `NaN`
/// vuelve `NaN` la pendiente OLS del transecto y, por el pool compartido, la
/// varianza de **todos** los satélites presentes en él — que entonces
/// desaparecen de la salida en silencio.
///
/// Devuelve un `SatelliteWeight` por cada satélite con al menos 2
/// residuos pooled y varianza positiva, ordenados por nombre. El resultado
/// es determinista: los transectos se recorren en orden de clave, de modo
/// que el orden de acumulación de los residuos (y por tanto los últimos
/// bits de las sumas flotantes) no depende del orden de iteración del
/// `HashMap`.
pub fn calibrate(
    by_transect: &HashMap<String, Vec<(f64, f64, String)>>,
    min_valid_per_transect: usize,
) -> Vec<SatelliteWeight> {
    let mut residuals_by_sat: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut names: Vec<&String> = by_transect.keys().collect();
    names.sort_unstable();
    for name in names {
        let obs = &by_transect[name];
        // solo observaciones utilizables: el detrend no tolera NaN/inf
        let usable: Vec<&(f64, f64, String)> = obs
            .iter()
            .filter(|o| o.0.is_finite() && o.1.is_finite())
            .collect();
        if usable.len() < min_valid_per_transect {
            continue;
        }
        let t: Vec<f64> = usable.iter().map(|o| o.0).collect();
        let y: Vec<f64> = usable.iter().map(|o| o.1).collect();
        let Some((slope, intercept)) = ols(&t, &y) else {
            continue;
        };
        for (i, o) in usable.iter().enumerate() {
            let resid = y[i] - (slope * t[i] + intercept);
            residuals_by_sat.entry(o.2.clone()).or_default().push(resid);
        }
    }

    let mut out = Vec::new();
    for (satname, resids) in residuals_by_sat {
        let n = resids.len();
        if n < 2 {
            continue;
        }
        let mean = resids.iter().sum::<f64>() / n as f64;
        let var = resids.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
        if var.is_nan() || var <= 0.0 {
            continue;
        }
        out.push(SatelliteWeight {
            satname,
            n_obs: n,
            std_m: var.sqrt(),
            weight: 1.0 / var,
        });
    }
    out.sort_by(|a, b| a.satname.cmp(&b.satname));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serie sintética determinista (xorshift) con tendencia lineal común
    /// y ruido de amplitud distinta por "satélite" — replica el escenario
    /// real de Viña (L8 más preciso que el resto).
    fn synthetic_transect(
        trend: f64,
        n: usize,
        sat_noise: &[(&str, f64)],
        seed: u64,
    ) -> Vec<(f64, f64, String)> {
        let mut rng = seed;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f64 / u64::MAX as f64) - 0.5
        };
        let mut obs = Vec::new();
        for i in 0..n {
            let t = 2015.0 + i as f64 * 0.05;
            let (sat, noise_amp) = sat_noise[i % sat_noise.len()];
            let y = 100.0 + trend * (t - 2015.0) + noise_amp * next();
            obs.push((t, y, sat.to_string()));
        }
        obs
    }

    #[test]
    fn satelite_mas_ruidoso_recibe_menor_peso() {
        let mut by_transect = HashMap::new();
        // 3 transectos con tendencias distintas, mismo patrón de ruido
        // relativo por satélite: L8 con desviación 1 m, L5 con 5 m.
        for (name, trend, seed) in [("T001", 2.0, 1), ("T002", -1.0, 2), ("T003", 0.5, 3)] {
            by_transect.insert(
                name.to_string(),
                synthetic_transect(trend, 200, &[("L8", 1.0), ("L5", 5.0)], seed),
            );
        }
        let weights = calibrate(&by_transect, 10);
        let l8 = weights
            .iter()
            .find(|w| w.satname == "L8")
            .expect("L8 presente");
        let l5 = weights
            .iter()
            .find(|w| w.satname == "L5")
            .expect("L5 presente");
        assert!(
            l8.weight > l5.weight,
            "L8 (menos ruido) debería tener mayor peso: L8={} L5={}",
            l8.weight,
            l5.weight
        );
        // razón de varianza esperada ~ (5/1)^2 = 25, con tolerancia amplia
        // por el ruido finito de la muestra
        let ratio = l8.weight / l5.weight;
        assert!(ratio > 10.0 && ratio < 60.0, "ratio = {ratio}");
    }

    #[test]
    fn transecto_bajo_minimo_se_ignora() {
        let mut by_transect = HashMap::new();
        by_transect.insert(
            "T001".to_string(),
            synthetic_transect(1.0, 5, &[("L8", 1.0)], 7),
        );
        let weights = calibrate(&by_transect, 10);
        assert!(
            weights.is_empty(),
            "transecto con <10 obs no debería aportar pesos"
        );
    }

    #[test]
    fn tendencia_fuerte_no_contamina_el_residuo() {
        // sin ruido: el detrend debe dejar residuos ~0, no explotar por
        // la tendencia grande
        let mut by_transect = HashMap::new();
        by_transect.insert(
            "T001".to_string(),
            synthetic_transect(50.0, 100, &[("L8", 0.0)], 11),
        );
        let weights = calibrate(&by_transect, 10);
        // varianza ~0 -> se descarta (var>0.0 requerido), no debe panicar
        // ni dar un peso infinito/NaN
        for w in &weights {
            assert!(w.weight.is_finite());
        }
    }

    #[test]
    fn sin_transectos_da_vacio() {
        let by_transect = HashMap::new();
        assert!(calibrate(&by_transect, 10).is_empty());
    }

    /// El CSV de `intersect` marca con NaN las intersecciones que rechaza el
    /// QC, así que llegan por diseño. Antes del filtro de finitud un solo NaN
    /// volvía NaN la pendiente OLS del transecto y, vía el pool compartido,
    /// borraba de la salida a todos los satélites presentes en él.
    #[test]
    fn nan_aislado_no_borra_los_satelites_del_pool() {
        let mut by_transect = HashMap::new();
        for (name, trend, seed) in [("T001", 2.0, 1), ("T002", -1.0, 2)] {
            let mut obs = synthetic_transect(trend, 200, &[("L8", 1.0), ("L5", 5.0)], seed);
            // una intersección rechazada por QC en medio de la serie
            obs[37].1 = f64::NAN;
            by_transect.insert(name.to_string(), obs);
        }
        let weights = calibrate(&by_transect, 10);
        let l8 = weights.iter().find(|w| w.satname == "L8");
        let l5 = weights.iter().find(|w| w.satname == "L5");
        assert!(
            l8.is_some() && l5.is_some(),
            "un NaN aislado no debe borrar satélites: {weights:?}"
        );
        // y el resultado sigue siendo utilizable: L8 (menos ruido) pesa más
        assert!(l8.unwrap().weight > l5.unwrap().weight);
        for w in &weights {
            assert!(w.std_m.is_finite() && w.weight.is_finite(), "{w:?}");
        }
    }

    /// El proyecto promete resultados bit-idénticos. Dos `HashMap` distintos
    /// con el mismo contenido lógico tienen (por `RandomState`) órdenes de
    /// iteración distintos; si la acumulación de residuos dependiera de ese
    /// orden, las sumas flotantes diferirían en los últimos bits.
    #[test]
    fn calibracion_es_bit_identica_ante_distinto_orden_de_hashmap() {
        let build = || {
            let mut m = HashMap::new();
            for (i, (name, trend, seed)) in [
                ("T001", 2.0, 1),
                ("T002", -1.0, 2),
                ("T003", 0.5, 3),
                ("T004", 1.25, 4),
                ("T005", -0.75, 5),
            ]
            .into_iter()
            .enumerate()
            {
                let _ = i;
                m.insert(
                    name.to_string(),
                    synthetic_transect(trend, 150, &[("L8", 1.0), ("L5", 5.0), ("L7", 3.0)], seed),
                );
            }
            m
        };
        let a = calibrate(&build(), 10);
        let b = calibrate(&build(), 10);
        assert_eq!(a.len(), b.len());
        for (wa, wb) in a.iter().zip(b.iter()) {
            assert_eq!(wa.satname, wb.satname);
            assert_eq!(wa.n_obs, wb.n_obs);
            assert_eq!(
                wa.weight.to_bits(),
                wb.weight.to_bits(),
                "peso de {} no es bit-idéntico: {} vs {}",
                wa.satname,
                wa.weight,
                wb.weight
            );
            assert_eq!(wa.std_m.to_bits(), wb.std_m.to_bits());
        }
    }
}
