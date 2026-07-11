//! Lectura de niveles de marea desde un CSV externo (SHOA, pyTMD, tide
//! gauge) e interpolación temporal a los instantes de las imágenes.
//!
//! Camino pragmático cuando ya existe una serie de marea calculada por
//! otra herramienta; la alternativa nativa es [`super::harmonic`].

use std::fs;
use std::path::Path;

use crate::error::{Result, StrandlineError};
use crate::tide::astro::datetime_to_mjd;

/// Serie temporal de elevaciones de marea, ordenada por tiempo ascendente.
#[derive(Debug, Clone)]
pub struct TideSeries {
    /// Instantes en Modified Julian Day (UTC), ascendentes.
    pub times_mjd: Vec<f64>,
    /// Elevación de marea (m) en cada instante.
    pub elevations: Vec<f64>,
}

impl TideSeries {
    /// Interpola linealmente la elevación en `mjd`. Devuelve `None` si
    /// `mjd` cae fuera del rango cubierto por la serie (no extrapola).
    pub fn interpolate(&self, mjd: f64) -> Option<f64> {
        let n = self.times_mjd.len();
        if n == 0 || mjd < self.times_mjd[0] || mjd > self.times_mjd[n - 1] {
            return None;
        }
        // búsqueda binaria del intervalo que contiene mjd
        let idx = match self
            .times_mjd
            .binary_search_by(|t| t.partial_cmp(&mjd).unwrap())
        {
            Ok(i) => return Some(self.elevations[i]),
            Err(i) => i, // times_mjd[i-1] < mjd < times_mjd[i]
        };
        let (t0, t1) = (self.times_mjd[idx - 1], self.times_mjd[idx]);
        let (e0, e1) = (self.elevations[idx - 1], self.elevations[idx]);
        let frac = (mjd - t0) / (t1 - t0);
        Some(e0 + frac * (e1 - e0))
    }

    /// Interpola a varios instantes; `NaN` donde queda fuera de rango.
    pub fn interpolate_many(&self, mjds: &[f64]) -> Vec<f64> {
        mjds.iter()
            .map(|&m| self.interpolate(m).unwrap_or(f64::NAN))
            .collect()
    }
}

/// Lee una serie de marea desde CSV. `time_col` y `elev_col` son los
/// nombres de las columnas de timestamp y elevación en la fila de header.
///
/// Los timestamps se asumen en **UTC**, en formato ISO
/// `YYYY-MM-DD[ T]HH:MM:SS[.f][Z|±HH:MM]` (cualquier sufijo de zona se
/// ignora — convierte a UTC antes si tu fuente está en hora local). Las
/// filas se ordenan por tiempo antes de devolverlas.
pub fn read_csv(path: impl AsRef<Path>, time_col: &str, elev_col: &str) -> Result<TideSeries> {
    let text = fs::read_to_string(path.as_ref()).map_err(|e| {
        StrandlineError::ShapeMismatch(format!("no se pudo leer CSV: {e}"))
    })?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());

    let header = lines
        .next()
        .ok_or_else(|| StrandlineError::ShapeMismatch("CSV vacío".into()))?;
    let cols: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
    let ti = cols
        .iter()
        .position(|&c| c == time_col)
        .ok_or_else(|| StrandlineError::ShapeMismatch(format!("falta columna '{time_col}'")))?;
    let ei = cols
        .iter()
        .position(|&c| c == elev_col)
        .ok_or_else(|| StrandlineError::ShapeMismatch(format!("falta columna '{elev_col}'")))?;

    let mut rows: Vec<(f64, f64)> = Vec::new();
    for (lineno, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if fields.len() <= ti.max(ei) {
            continue;
        }
        let mjd = parse_timestamp(fields[ti]).ok_or_else(|| {
            StrandlineError::ShapeMismatch(format!(
                "timestamp inválido en línea {}: '{}'",
                lineno + 2,
                fields[ti]
            ))
        })?;
        let elev: f64 = fields[ei].parse().map_err(|_| {
            StrandlineError::ShapeMismatch(format!(
                "elevación inválida en línea {}: '{}'",
                lineno + 2,
                fields[ei]
            ))
        })?;
        rows.push((mjd, elev));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    Ok(TideSeries {
        times_mjd: rows.iter().map(|r| r.0).collect(),
        elevations: rows.iter().map(|r| r.1).collect(),
    })
}

/// Lee constituyentes de marea locales desde CSV. Columnas por nombre:
/// `name` (nombre estándar, ej. M2), `amplitude` (m), `phase_g` (fase de
/// Greenwich, grados). Devuelve `(name, amplitude, phase_g)` por fila.
pub fn read_constituents_csv(
    path: impl AsRef<Path>,
) -> Result<Vec<(String, f64, f64)>> {
    let text = fs::read_to_string(path.as_ref())
        .map_err(|e| StrandlineError::ShapeMismatch(format!("no se pudo leer CSV: {e}")))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| StrandlineError::ShapeMismatch("CSV vacío".into()))?;
    let cols: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
    let find = |name: &str| {
        cols.iter()
            .position(|&c| c == name)
            .ok_or_else(|| StrandlineError::ShapeMismatch(format!("falta columna '{name}'")))
    };
    let (ni, ai, pi) = (find("name")?, find("amplitude")?, find("phase_g")?);

    let mut out = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if f.len() <= ni.max(ai).max(pi) {
            continue;
        }
        let amp: f64 = f[ai]
            .parse()
            .map_err(|_| StrandlineError::ShapeMismatch(format!("amplitud inválida: '{}'", f[ai])))?;
        let ph: f64 = f[pi]
            .parse()
            .map_err(|_| StrandlineError::ShapeMismatch(format!("fase inválida: '{}'", f[pi])))?;
        out.push((f[ni].to_string(), amp, ph));
    }
    Ok(out)
}

/// Parsea un timestamp ISO a MJD (UTC). Tolerante al separador fecha-hora
/// (`T` o espacio), a fracción de segundo y a sufijos de zona, que ignora.
pub fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date_part, time_part) = if let Some(pos) = s.find(['T', ' ']) {
        (&s[..pos], &s[pos + 1..])
    } else {
        (s, "")
    };

    let mut d = date_part.split('-');
    let year: i32 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;

    let (mut hour, mut minute, mut second) = (0u32, 0u32, 0.0f64);
    if !time_part.is_empty() {
        // recortar sufijo de zona (Z, +HH:MM, -HH:MM) del final
        let core = strip_timezone(time_part);
        let mut t = core.split(':');
        hour = t.next()?.parse().ok()?;
        minute = t.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        second = t.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
    }
    Some(datetime_to_mjd(year, month, day, hour, minute, second))
}

/// Quita un sufijo de zona horaria de la parte de hora (`Z`, `+HH:MM`,
/// `-HH:MM`). No aplica el offset — se asume que el CSV está en UTC.
fn strip_timezone(time_part: &str) -> &str {
    if let Some(stripped) = time_part.strip_suffix('Z') {
        return stripped;
    }
    // un '+' o un '-' que aparezca DESPUÉS del primer carácter marca la zona
    if let Some(pos) = time_part[1..].find(['+', '-']) {
        return &time_part[..pos + 1];
    }
    time_part
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_variants() {
        let a = parse_timestamp("2020-01-10T14:33:52").unwrap();
        let b = parse_timestamp("2020-01-10 14:33:52").unwrap();
        let c = parse_timestamp("2020-01-10T14:33:52+00:00").unwrap();
        let d = parse_timestamp("2020-01-10T14:33:52Z").unwrap();
        for x in [b, c, d] {
            assert!((a - x).abs() < 1e-9);
        }
        assert!((parse_timestamp("2020-01-10").unwrap() - 58858.0).abs() < 1e-9);
    }

    #[test]
    fn linear_interpolation() {
        let s = TideSeries {
            times_mjd: vec![0.0, 1.0, 2.0],
            elevations: vec![0.0, 10.0, 20.0],
        };
        assert!((s.interpolate(0.5).unwrap() - 5.0).abs() < 1e-9);
        assert!((s.interpolate(1.0).unwrap() - 10.0).abs() < 1e-9);
        assert!(s.interpolate(-0.1).is_none());
        assert!(s.interpolate(2.1).is_none());
    }
}
