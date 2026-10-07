//! Descomposición estacional-tendencia de series de shoreline (STL,
//! Cleveland et al. 1990) — validado como práctica activa por los propios
//! creadores de CoastSat/CoastSeg a escala (Warrick, Vos & Buscombe 2025,
//! *JGR: Earth Surface*, 7777 transectos de California, 22 años de datos
//! CoastSat): ~90% de los transectos muestran ciclos estacionales
//! significativos, y la erosión NO es uniformemente invernal (59% invierno,
//! más de 30% primavera/verano) — desafía el supuesto implícito de las
//! tasas lineales DSAS de que solo importa la pendiente. Ver investigación
//! de estado del arte 2026-07-17 en la memoria del proyecto.
//!
//! Las series satelitales son de muestreo irregular (nubes, revisita
//! desigual entre Landsat/Sentinel-2); STL clásico asume muestreo regular.
//! Se resamplea a una grilla mensual (media de las observaciones de cada mes
//! calendario, interpolación lineal en los meses sin observaciones) antes de
//! aplicar STL con periodo 12 (estacionalidad anual). Recorta a `[primer,
//! último]` mes con datos reales — no extrapola en los bordes.
//!
//! Los meses son **calendario real**: el año decimal de entrada se invierte
//! al día del que proviene y de ahí se toma el mes civil. Una división del
//! año en 12 bins iguales (30.44 días) desplazaría observaciones entre meses
//! vecinos en los bordes, que es justo lo que rompe un cruce por mes contra
//! una serie externa. Cada punto de la grilla trae su `(año, mes)` explícito
//! para que ese join sea inequívoco.

use crate::rates::decimal_year_from_mjd;

/// Resultado de la descomposición STL de una serie de transecto.
#[derive(Debug, Clone)]
pub struct StlResult {
    /// Año decimal de cada punto de la grilla mensual (día 15 del mes
    /// calendario correspondiente).
    pub months: Vec<f64>,
    /// `(año, mes)` calendario de cada punto de la grilla, con `mes` en
    /// 1..=12. Es la etiqueta a usar para cruzar la serie contra índices
    /// mensuales externos (ONI, oleaje): el año decimal es útil para
    /// graficar, pero un join numérico sobre él es ambiguo en los bordes.
    pub year_month: Vec<(i32, u32)>,
    /// Componente de tendencia (m).
    pub trend: Vec<f64>,
    /// Componente estacional (m).
    pub seasonal: Vec<f64>,
    /// Residuo (m).
    pub residual: Vec<f64>,
    /// Meses de la grilla que fueron interpolados (sin observación real).
    pub interpolated: Vec<bool>,
    /// Amplitud del ciclo estacional: max(seasonal) − min(seasonal) (m).
    pub seasonal_amplitude_m: f64,
    /// Fracción de la varianza (estacional + residuo) explicada por el
    /// componente estacional — proxy de "significancia estacional" en el
    /// espíritu de Warrick et al. 2025 (var(seasonal) / var(seasonal+resid)).
    /// `NaN` si esa varianza total es 0 (serie sin variabilidad de corto
    /// plazo, toda la señal es tendencia).
    pub seasonal_variance_frac: f64,
}

/// Convierte MJD a año decimal — reexporta la convención de
/// [`crate::rates::decimal_year_from_mjd`] para quien solo trabaje con STL.
#[inline]
pub fn decimal_year(mjd: f64) -> f64 {
    decimal_year_from_mjd(mjd)
}

/// MJD del día 0 del calendario civil usado como época: MJD 40587 =
/// 1970-01-01, la misma referencia que [`decimal_year_from_mjd`].
const MJD_EPOCH_1970: f64 = 40587.0;

/// Fecha civil `(año, mes, día)` desde días transcurridos since 1970-01-01
/// (algoritmo de Howard Hinnant, exacto para el calendario gregoriano
/// proléptico).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Días transcurridos since 1970-01-01 para una fecha civil (inversa de
/// [`civil_from_days`]).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Índice de mes **calendario** absoluto (`año·12 + mes−1`) para un año
/// decimal.
///
/// El año decimal se invierte primero al día juliano modificado que lo
/// originó (la conversión de [`decimal_year_from_mjd`] es exactamente
/// invertible sobre días enteros) y de ahí se toma el mes civil real.
///
/// La versión anterior hacía `(frac·12).floor()`, que parte el año en 12
/// bins iguales de 365.2425/12 = 30.44 días: no son meses calendario y la
/// época acumula desfase. Con ese cálculo, 2020-01-01 caía en el bin
/// "diciembre 2019" y 2020-03-01 compartía bin con 2020-02-29 — suficiente
/// para corromper cualquier cruce por mes contra una serie externa
/// (climática, de oleaje) en los bordes de mes.
#[inline]
fn abs_month(t_year: f64) -> i64 {
    let mjd_day = ((t_year - 1970.0) * 365.2425 + MJD_EPOCH_1970).round();
    let days = (mjd_day - MJD_EPOCH_1970) as i64;
    let (y, m, _) = civil_from_days(days);
    y as i64 * 12 + (m as i64 - 1)
}

/// `(año, mes)` civil de un índice de mes absoluto.
#[inline]
fn year_month_of(m: i64) -> (i32, u32) {
    (m.div_euclid(12) as i32, m.rem_euclid(12) as u32 + 1)
}

/// Año decimal representativo de un mes calendario (su día 15), en la misma
/// convención que la entrada — inversa de [`abs_month`] para etiquetar la
/// grilla de salida.
#[inline]
fn month_to_decimal_year(m: i64) -> f64 {
    let (y, mo) = year_month_of(m);
    let mjd = days_from_civil(y, mo, 15) as f64 + MJD_EPOCH_1970;
    decimal_year_from_mjd(mjd)
}

/// Resamplea una serie irregular a una grilla mensual: media de las
/// observaciones finitas de cada mes calendario, interpolación lineal en
/// los meses sin observación, recortada a `[primer, último]` mes con datos
/// reales (sin extrapolar en los bordes).
///
/// Devuelve `None` si hay menos de 2 meses distintos con datos (no hay nada
/// que interpolar/decomponer).
type MonthlyGrid = (Vec<f64>, Vec<(i32, u32)>, Vec<f64>, Vec<bool>);

fn resample_monthly(times_years: &[f64], chainage: &[f64]) -> Option<MonthlyGrid> {
    use std::collections::BTreeMap;
    let mut bins: BTreeMap<i64, (f64, usize)> = BTreeMap::new();
    for (&t, &y) in times_years.iter().zip(chainage) {
        if !t.is_finite() || !y.is_finite() {
            continue;
        }
        let m = abs_month(t);
        let e = bins.entry(m).or_insert((0.0, 0));
        e.0 += y;
        e.1 += 1;
    }
    if bins.len() < 2 {
        return None;
    }
    let means: BTreeMap<i64, f64> = bins
        .into_iter()
        .map(|(m, (sum, n))| (m, sum / n as f64))
        .collect();
    let first = *means.keys().next().unwrap();
    let last = *means.keys().next_back().unwrap();

    let observed: Vec<(i64, f64)> = means.into_iter().collect();
    let mut months = Vec::new();
    let mut year_month = Vec::new();
    let mut values = Vec::new();
    let mut interpolated = Vec::new();
    let mut idx = 0usize; // índice del próximo punto observado a la derecha
    for m in first..=last {
        months.push(month_to_decimal_year(m));
        year_month.push(year_month_of(m));
        if idx < observed.len() && observed[idx].0 == m {
            values.push(observed[idx].1);
            interpolated.push(false);
            idx += 1;
        } else {
            // interpolación lineal entre el punto observado anterior y el siguiente
            let (m0, v0) = observed[idx - 1];
            let (m1, v1) = observed[idx];
            let frac = (m - m0) as f64 / (m1 - m0) as f64;
            values.push(v0 + frac * (v1 - v0));
            interpolated.push(true);
        }
    }
    Some((months, year_month, values, interpolated))
}

fn variance(xs: &[f64]) -> f64 {
    let n = xs.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let mean = xs.iter().sum::<f64>() / n;
    xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n
}

/// Descompone una serie de transecto (año decimal + chainage, muestreo
/// irregular admitido) en tendencia + estacional + residuo.
///
/// - `min_months`: mínimo de meses distintos con al menos una observación
///   real (no interpolados) para intentar la descomposición — evita ajustar
///   STL sobre series demasiado dispersas donde la mayoría de la grilla
///   sería interpolación pura.
/// - `min_span_years`: lapso mínimo cubierto (recomendado ≥ 2 años: STL con
///   periodo 12 necesita al menos 2 ciclos completos para separar
///   estacional de tendencia de forma no degenerada).
///
/// Devuelve `None` si no hay suficientes datos o si `stlrs` no converge.
pub fn decompose(
    times_years: &[f64],
    chainage: &[f64],
    min_months: usize,
    min_span_years: f64,
) -> Option<StlResult> {
    if times_years.len() != chainage.len() {
        return None;
    }
    let (months, year_month, values, interpolated) = resample_monthly(times_years, chainage)?;

    let n_observed = interpolated.iter().filter(|&&i| !i).count();
    if n_observed < min_months {
        return None;
    }
    let span = months.last()? - months.first()?;
    if span < min_span_years {
        return None;
    }

    let fit = stlrs::params()
        .seasonal_length(7)
        .robust(true)
        .fit(&values, 12)
        .ok()?;

    let seasonal = fit.seasonal().to_vec();
    let trend = fit.trend().to_vec();
    let residual = fit.remainder().to_vec();

    let seasonal_amplitude_m = seasonal.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - seasonal.iter().cloned().fold(f64::INFINITY, f64::min);

    let var_seasonal = variance(&seasonal);
    let combined: Vec<f64> = seasonal.iter().zip(&residual).map(|(s, r)| s + r).collect();
    let var_combined = variance(&combined);
    let seasonal_variance_frac = if var_combined > 0.0 {
        var_seasonal / var_combined
    } else {
        f64::NAN
    };

    Some(StlResult {
        months,
        year_month,
        trend,
        seasonal,
        residual,
        interpolated,
        seasonal_amplitude_m,
        seasonal_variance_frac,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serie sintética: tendencia lineal + estacional anual senoidal + ruido
    /// determinista pequeño, muestreada irregularmente (como un satélite
    /// real: huecos aleatorios, no cada mes).
    fn synthetic_series(
        n_years: usize,
        trend_per_year: f64,
        amplitude: f64,
        noise: f64,
        seed: u64,
    ) -> (Vec<f64>, Vec<f64>) {
        let mut rng = seed;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f64 / u64::MAX as f64) - 0.5
        };
        let mut times = Vec::new();
        let mut vals = Vec::new();
        let mut t = 2015.0;
        let end = 2015.0 + n_years as f64;
        while t < end {
            // ~16 días de paso medio (irregular, con huecos ocasionales)
            let step = (16.0 + 10.0 * next()) / 365.2425;
            t += step.max(1.0 / 365.2425);
            if t >= end {
                break;
            }
            // ~15% de las escenas "se pierden" (nube) -> muestreo más irregular
            if next() < -0.35 {
                continue;
            }
            let trend = trend_per_year * (t - 2015.0);
            let seasonal = amplitude * (2.0 * std::f64::consts::PI * (t - 2015.0)).sin();
            vals.push(trend + seasonal + noise * next());
            times.push(t);
        }
        (times, vals)
    }

    #[test]
    fn recupera_amplitud_estacional_conocida() {
        let (t, y) = synthetic_series(6, 2.0, 8.0, 0.5, 42);
        let result = decompose(&t, &y, 12, 2.0).expect("debería descomponer");
        // amplitud pico-a-pico de un seno de amplitud 8 es 16; STL con ruido
        // y resampleo mensual no la recupera exacta, pero debe quedar en un
        // rango razonable (no degenerar a ~0 ni explotar)
        assert!(
            result.seasonal_amplitude_m > 8.0 && result.seasonal_amplitude_m < 24.0,
            "amplitud = {}",
            result.seasonal_amplitude_m
        );
        // la mayor parte de la varianza de corto plazo debe explicarse por
        // el estacional, dado que el ruido es pequeño frente a la amplitud
        assert!(
            result.seasonal_variance_frac > 0.5,
            "frac = {}",
            result.seasonal_variance_frac
        );
    }

    #[test]
    fn serie_sin_estacionalidad_da_amplitud_chica() {
        // solo tendencia + ruido, sin componente senoidal
        let (t, y) = synthetic_series(6, 3.0, 0.0, 1.0, 7);
        let result = decompose(&t, &y, 12, 2.0).expect("debería descomponer");
        assert!(
            result.seasonal_amplitude_m < 6.0,
            "amplitud sin estacionalidad real = {}",
            result.seasonal_amplitude_m
        );
    }

    #[test]
    fn serie_corta_devuelve_none() {
        let t = vec![2020.0, 2020.1, 2020.2];
        let y = vec![10.0, 10.5, 11.0];
        assert!(decompose(&t, &y, 12, 2.0).is_none());
    }

    #[test]
    fn serie_vacia_devuelve_none() {
        assert!(decompose(&[], &[], 12, 2.0).is_none());
    }

    /// Round-trip del calendario civil sobre fechas de referencia y sobre
    /// todos los días de varios años (incluidos bisiestos y seculares).
    #[test]
    fn calendario_civil_es_exacto() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        // MJD 58849 = 2020-01-01 (58849 − 40587 = 18262 días)
        assert_eq!(civil_from_days(18262), (2020, 1, 1));
        assert_eq!(civil_from_days(18262 + 59), (2020, 2, 29)); // 2020 bisiesto
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        for d in -40_000..40_000i64 {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "round-trip falla en {d}");
        }
    }

    /// Los casos exactos que el binning de 30.44 días clasificaba mal.
    #[test]
    fn meses_son_calendario_real_no_bins_de_30_44_dias() {
        let dy = |y: i32, m: u32, d: u32| {
            decimal_year_from_mjd(days_from_civil(y, m, d) as f64 + MJD_EPOCH_1970)
        };
        // 2020-01-01 caía en "diciembre 2019" con el cálculo anterior
        assert_eq!(year_month_of(abs_month(dy(2020, 1, 1))), (2020, 1));
        // 2020-03-01 compartía bin con 2020-02-29
        assert_eq!(year_month_of(abs_month(dy(2020, 3, 1))), (2020, 3));
        assert_ne!(abs_month(dy(2020, 3, 1)), abs_month(dy(2020, 2, 29)));
        assert_eq!(year_month_of(abs_month(dy(2020, 2, 29))), (2020, 2));
        // primer y último día de cada mes de un año bisiesto y uno normal
        for year in [2019, 2020] {
            for m in 1..=12u32 {
                let last = match m {
                    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                    4 | 6 | 9 | 11 => 30,
                    _ if year % 4 == 0 => 29,
                    _ => 28,
                };
                assert_eq!(year_month_of(abs_month(dy(year, m, 1))), (year, m));
                assert_eq!(year_month_of(abs_month(dy(year, m, last))), (year, m));
            }
        }
        // la etiqueta de salida cae dentro del mes que representa
        for m in [abs_month(dy(2020, 1, 15)), abs_month(dy(1999, 12, 15))] {
            assert_eq!(
                year_month_of(abs_month(month_to_decimal_year(m))),
                year_month_of(m)
            );
        }
    }

    #[test]
    fn resample_monthly_interpola_huecos() {
        // dos meses con datos separados por 3 meses vacíos
        let t = vec![2020.0, 2020.3333]; // mes 0 y mes ~4
        let y = vec![0.0, 12.0];
        let (months, _ym, values, interpolated) = resample_monthly(&t, &y).unwrap();
        assert_eq!(months.len(), values.len());
        assert!(!interpolated[0]);
        assert!(!interpolated[interpolated.len() - 1]);
        assert!(interpolated[1..interpolated.len() - 1].iter().all(|&i| i));
        // interpolación lineal: el valor debe crecer monótonamente de 0 a 12
        for w in values.windows(2) {
            assert!(w[1] >= w[0] - 1e-9);
        }
    }
}
