//! Longitudes astronómicas medias para la síntesis armónica de marea.
//!
//! Réplica del método `'Meeus'` de `pyTMD.astro.mean_longitudes`
//! (coeficientes de Meeus, *Astronomical Algorithms* 1991; perigeo solar
//! de Simon et al. 1994). Se elige Meeus sobre el default `'Cartwright'`
//! de pyTMD porque el paper de erosión costera cubre 1984–2025 y Cartwright
//! está calibrado solo para 1990–2010.

/// MJD del epoch J2000.0 (2000-01-01T12:00:00 TT).
const MJD_J2000: f64 = 51544.5;
/// Días en un siglo juliano.
const CENTURY: f64 = 36525.0;

/// Longitudes astronómicas medias en grados, en el instante `mjd`
/// (Modified Julian Day). Todas normalizadas a `[0, 360)`.
#[derive(Debug, Clone, Copy)]
pub struct MeanLongitudes {
    /// `S`: longitud media de la Luna.
    pub s: f64,
    /// `H`: longitud media del Sol.
    pub h: f64,
    /// `P`: longitud media del perigeo lunar.
    pub p: f64,
    /// `N`: longitud media del nodo lunar ascendente (decreciente en el
    /// tiempo; `N' = −N` es la forma creciente usada en los argumentos).
    pub n: f64,
    /// `Ps`: longitud del perigeo solar.
    pub ps: f64,
}

#[inline]
fn polynomial_sum(coeffs: &[f64], t: f64) -> f64 {
    // Horner sobre potencias crecientes de t (coeffs[0] + coeffs[1]·t + …).
    coeffs.iter().rev().fold(0.0, |acc, &c| acc * t + c)
}

#[inline]
fn normalize_angle(deg: f64) -> f64 {
    let m = deg % 360.0;
    if m < 0.0 {
        m + 360.0
    } else {
        m
    }
}

/// Calcula las longitudes astronómicas medias en `mjd` (método Meeus).
pub fn mean_longitudes(mjd: f64) -> MeanLongitudes {
    // días relativos a 2000-01-01T12:00:00
    let t = mjd - MJD_J2000;

    let s = polynomial_sum(
        &[
            218.3164591,
            13.176_396_477_545_79,
            -9.9454632e-13,
            3.8086292e-20,
            -8.6184958e-27,
        ],
        t,
    );
    let h = polynomial_sum(&[280.46645, 0.985_647_360_164_271, 2.2727347e-13], t);
    let p = polynomial_sum(
        &[
            83.3532430,
            0.111_403_523_917_864_47,
            -7.7385418e-12,
            -2.5636086e-19,
            2.95738836e-26,
        ],
        t,
    );
    let n = polynomial_sum(
        &[
            125.0445550,
            -0.052_953_762_762_491_446,
            1.55628359e-12,
            4.390675353e-20,
            -9.26940435e-27,
        ],
        t,
    );
    let ps = 282.94 + (1.7192 * t) / CENTURY;

    MeanLongitudes {
        s: normalize_angle(s),
        h: normalize_angle(h),
        p: normalize_angle(p),
        n: normalize_angle(n),
        ps: normalize_angle(ps),
    }
}

/// Convierte una fecha-hora del calendario gregoriano (UTC) a Modified
/// Julian Day. `year` completo (ej. 2020), `month` 1–12, `day` 1–31,
/// más la hora del día. Algoritmo de Fliegel–Van Flandern.
pub fn datetime_to_mjd(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: f64,
) -> f64 {
    let (y, m) = if month <= 2 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    let jd_day = (365.25 * (y as f64 + 4716.0)).floor()
        + (30.6001 * (m as f64 + 1.0)).floor()
        + day as f64
        + b
        - 1524.5;
    let frac = (hour as f64 + minute as f64 / 60.0 + second / 3600.0) / 24.0;
    (jd_day + frac) - 2_400_000.5
}

/// Tiempo solar medio local τ en grados (ángulo horario de la Luna media),
/// base del argumento de los constituyentes diurnos/semidiurnos:
/// `τ = 15°·hora_UT + H − S`.
#[inline]
pub fn lunar_time_tau(mjd: f64, longitudes: &MeanLongitudes) -> f64 {
    let ut_hours = (mjd - mjd.floor()) * 24.0;
    normalize_angle(15.0 * ut_hours + longitudes.h - longitudes.s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mjd_known_epochs() {
        // 2000-01-01T00:00:00 UTC = MJD 51544.0
        assert!((datetime_to_mjd(2000, 1, 1, 0, 0, 0.0) - 51544.0).abs() < 1e-6);
        // 1858-11-17T00:00:00 = MJD 0 (origen del MJD)
        assert!((datetime_to_mjd(1858, 11, 17, 0, 0, 0.0)).abs() < 1e-6);
        // 2020-01-10T14:33:52 UTC (una escena del spike)
        let mjd = datetime_to_mjd(2020, 1, 10, 14, 33, 52.0);
        assert!((mjd - 58858.6068518).abs() < 1e-5, "mjd = {mjd}");
    }

    #[test]
    fn longitudes_in_range() {
        let l = mean_longitudes(58858.6);
        for v in [l.s, l.h, l.p, l.n, l.ps] {
            assert!((0.0..360.0).contains(&v), "fuera de rango: {v}");
        }
    }

    #[test]
    fn mean_longitudes_match_pytmd_meeus() {
        // Referencia: pyTMD.astro.mean_longitudes(mjd, method='Meeus').
        // (mjd, S, H, P, N, Ps)
        let cases = [
            (
                46090.5, 354.250041, 304.745754, 195.758193, 53.854423, 282.683285,
            ),
            (
                51544.0, 211.728261, 279.973626, 83.297541, 125.071032, 282.939976,
            ),
            (
                58858.6068518,
                111.888164,
                289.596573,
                178.170107,
                97.735159,
                283.284269,
            ),
            (
                60000.25, 34.630903, 334.854132, 305.353037, 37.280887, 283.338005,
            ),
        ];
        for (mjd, s, h, p, n, ps) in cases {
            let l = mean_longitudes(mjd);
            assert!((l.s - s).abs() < 1e-4, "S en {mjd}: {} vs {s}", l.s);
            assert!((l.h - h).abs() < 1e-4, "H en {mjd}: {} vs {h}", l.h);
            assert!((l.p - p).abs() < 1e-4, "P en {mjd}: {} vs {p}", l.p);
            assert!((l.n - n).abs() < 1e-4, "N en {mjd}: {} vs {n}", l.n);
            assert!((l.ps - ps).abs() < 1e-4, "Ps en {mjd}: {} vs {ps}", l.ps);
        }
    }
}
