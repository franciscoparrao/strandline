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
/// Timestamps en formato ISO `YYYY-MM-DD[ T]HH:MM:SS[.f][Z|±HH:MM]`. Los
/// sufijos de zona horaria **se aplican** (el resultado interno siempre es
/// UTC); sin sufijo se asume UTC. Las filas se ordenan por tiempo antes de
/// devolverlas.
pub fn read_csv(path: impl AsRef<Path>, time_col: &str, elev_col: &str) -> Result<TideSeries> {
    let text = fs::read_to_string(path.as_ref()).map_err(|source| StrandlineError::Io {
        path: path.as_ref().display().to_string(),
        source,
    })?;
    // se conserva el número de línea FÍSICO para los mensajes de error
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());

    let header = lines
        .next()
        .ok_or_else(|| StrandlineError::Parse("CSV vacío".into()))?
        .1;
    // un BOM UTF-8 (Excel) haría que la primera columna no matchee su nombre
    let header = header.trim_start_matches('\u{feff}');
    let cols: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
    let ti = cols
        .iter()
        .position(|&c| c == time_col)
        .ok_or_else(|| StrandlineError::Parse(format!("falta columna '{time_col}'")))?;
    let ei = cols
        .iter()
        .position(|&c| c == elev_col)
        .ok_or_else(|| StrandlineError::Parse(format!("falta columna '{elev_col}'")))?;

    let mut rows: Vec<(f64, f64)> = Vec::new();
    for (lineno, line) in lines {
        let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        // una fila truncada es un error, no algo que descartar en silencio
        if fields.len() <= ti.max(ei) {
            return Err(StrandlineError::Parse(format!(
                "línea {} con {} campos: se esperaban al menos {}",
                lineno + 1,
                fields.len(),
                ti.max(ei) + 1
            )));
        }
        let mjd = parse_timestamp(fields[ti]).ok_or_else(|| {
            StrandlineError::Parse(format!(
                "timestamp inválido en línea {}: '{}'",
                lineno + 1,
                fields[ti]
            ))
        })?;
        let elev: f64 = fields[ei].parse().map_err(|_| {
            StrandlineError::Parse(format!(
                "elevación inválida en línea {}: '{}'",
                lineno + 1,
                fields[ei]
            ))
        })?;
        // `"NaN"` parsea como f64 válido y es la marca de hueco habitual en
        // exports de mareógrafos/pyTMD: sin este filtro entra en silencio a
        // la interpolación y contamina las correcciones.
        if !elev.is_finite() {
            return Err(StrandlineError::Parse(format!(
                "elevación no finita en línea {}: '{}'",
                lineno + 1,
                fields[ei]
            )));
        }
        rows.push((mjd, elev));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));

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
    let text = fs::read_to_string(path.as_ref()).map_err(|source| StrandlineError::Io {
        path: path.as_ref().display().to_string(),
        source,
    })?;
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| StrandlineError::Parse("CSV vacío".into()))?
        .1;
    let header = header.trim_start_matches('\u{feff}');
    let cols: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
    let find = |name: &str| {
        cols.iter()
            .position(|&c| c == name)
            .ok_or_else(|| StrandlineError::Parse(format!("falta columna '{name}'")))
    };
    let (ni, ai, pi) = (find("name")?, find("amplitude")?, find("phase_g")?);

    let mut out: Vec<(String, f64, f64)> = Vec::new();
    for (lineno, line) in lines {
        let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        // una fila truncada haría desaparecer un constituyente entero (p. ej.
        // M2) del predictor sin ninguna señal
        if f.len() <= ni.max(ai).max(pi) {
            return Err(StrandlineError::Parse(format!(
                "línea {} con {} campos: se esperaban al menos {}",
                lineno + 1,
                f.len(),
                ni.max(ai).max(pi) + 1
            )));
        }
        let amp: f64 = f[ai].parse().map_err(|_| {
            StrandlineError::Parse(format!("amplitud inválida en línea {}: '{}'", lineno + 1, f[ai]))
        })?;
        let ph: f64 = f[pi].parse().map_err(|_| {
            StrandlineError::Parse(format!("fase inválida en línea {}: '{}'", lineno + 1, f[pi]))
        })?;
        if !amp.is_finite() || !ph.is_finite() {
            return Err(StrandlineError::Parse(format!(
                "amplitud/fase no finita en línea {}: '{}', '{}'",
                lineno + 1,
                f[ai],
                f[pi]
            )));
        }
        // un constituyente repetido se sumaría dos veces en `predict`: para
        // M2 en Chile central eso son ~0.4 m de marea espuria (~4 m de
        // corrección horizontal) sin ninguna advertencia
        if let Some((prev, _, _)) = out
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(f[ni]))
        {
            return Err(StrandlineError::Parse(format!(
                "constituyente duplicado en línea {}: '{}' ya aparece como '{}'",
                lineno + 1,
                f[ni],
                prev
            )));
        }
        out.push((f[ni].to_string(), amp, ph));
    }
    Ok(out)
}

/// Parsea un timestamp ISO a MJD (UTC). Tolerante al separador fecha-hora
/// (`T` o espacio) y a fracción de segundo. Los sufijos de zona horaria
/// (`Z`, `±HH:MM`, `±HHMM`, `±HH`) **se aplican**: el resultado siempre es
/// UTC (bug R1 de la auditoría 2026-07 — antes se ignoraban en silencio).
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
    // sin validar el rango, `2020-13-01` o `2020-02-30` se convertirían a un
    // MJD aritméticamente coherente pero incorrecto, en silencio
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (mut hour, mut minute, mut second) = (0u32, 0u32, 0.0f64);
    let mut offset_hours = 0.0f64;
    if !time_part.is_empty() {
        let (core, offset) = split_timezone(time_part)?;
        offset_hours = offset;
        let mut t = core.split(':');
        hour = t.next()?.parse().ok()?;
        // un campo *ausente* vale 0; uno *presente pero ilegible* es un error
        // (antes `unwrap_or(0)` absorbía ambos y `14:3x:52` daba 14:00:00)
        minute = match t.next() {
            None => 0,
            Some(x) => x.parse().ok()?,
        };
        second = match t.next() {
            None => 0.0,
            Some(x) => x.parse().ok()?,
        };
        // `"nan"`/`"inf"` parsean como f64 válidos: sin este filtro un
        // timestamp con segundos NaN produce un MJD NaN que más tarde hace
        // panic al ordenar la serie.
        if !second.is_finite() || hour > 23 || minute > 59 || !(0.0..60.0).contains(&second) {
            return None;
        }
    }
    // hora_local = UTC + offset  →  UTC = hora_local − offset
    let mjd = datetime_to_mjd(year, month, day, hour, minute, second) - offset_hours / 24.0;
    mjd.is_finite().then_some(mjd)
}

/// Separa la parte de hora de su sufijo de zona horaria y devuelve
/// `(hora_sin_zona, offset_en_horas)`. Acepta `Z` (offset 0), `±HH:MM`,
/// `±HHMM` y `±HH`. Sin sufijo → offset 0 (se asume UTC). Devuelve `None`
/// si el sufijo existe pero está malformado — mejor rechazar que
/// interpretar mal una zona horaria.
fn split_timezone(time_part: &str) -> Option<(&str, f64)> {
    if let Some(stripped) = time_part.strip_suffix('Z') {
        return Some((stripped, 0.0));
    }
    // Todo el trabajo se hace sobre bytes ASCII: cortar un `&str` por índice
    // de byte hace panic si el corte cae dentro de un carácter multibyte, y
    // los CSV con mojibake (Latin-1 releído como UTF-8) lo disparan.
    let bytes = time_part.as_bytes();
    if !time_part.is_ascii() {
        return None;
    }
    // un '+' o un '-' que aparezca DESPUÉS del primer carácter marca la zona
    let Some(pos) = bytes
        .iter()
        .skip(1)
        .position(|b| *b == b'+' || *b == b'-')
        .map(|p| p + 1)
    else {
        return Some((time_part, 0.0));
    };
    let (core, suffix) = (&time_part[..pos], &time_part[pos..]);
    let sign = if suffix.starts_with('-') { -1.0 } else { 1.0 };
    let digits = &suffix[1..];
    let (hh, mm): (&str, &str) = match digits.len() {
        // ±HH:MM
        5 if digits.as_bytes()[2] == b':' => (&digits[..2], &digits[3..]),
        // ±HHMM
        4 => (&digits[..2], &digits[2..]),
        // ±HH
        2 => (digits, "0"),
        _ => return None,
    };
    let hh: f64 = hh.parse().ok()?;
    let mm: f64 = mm.parse().ok()?;
    Some((core, sign * (hh + mm / 60.0)))
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
    fn timezone_offset_applied() {
        // R1 (auditoría 2026-07): el offset debe APLICARSE, no ignorarse.
        // 14:33:52 en Chile continental (-03:00) = 17:33:52 UTC.
        let utc = parse_timestamp("2020-01-10T17:33:52Z").unwrap();
        for local in [
            "2020-01-10T14:33:52-03:00",
            "2020-01-10T14:33:52-0300",
            "2020-01-10T14:33:52-03",
        ] {
            let x = parse_timestamp(local).unwrap();
            assert!((x - utc).abs() < 1e-9, "{local}: {x} vs {utc}");
        }
        // offset positivo (este de Greenwich)
        let plus = parse_timestamp("2020-01-10T20:33:52+03:00").unwrap();
        assert!((plus - utc).abs() < 1e-9);
        // offset con minutos (India +05:30)
        let india = parse_timestamp("2020-01-10T23:03:52+05:30").unwrap();
        assert!((india - utc).abs() < 1e-9);
    }

    #[test]
    fn malformed_timezone_rejected() {
        // sufijo de zona presente pero ilegible → None, no interpretación
        assert!(parse_timestamp("2020-01-10T14:33:52-3:0").is_none());
        assert!(parse_timestamp("2020-01-10T14:33:52+abc").is_none());
    }

    /// `"nan"`/`"inf"` parsean como f64 válidos: un MJD NaN hacía panic más
    /// tarde al ordenar la serie. Y los componentes fuera de rango daban un
    /// MJD aritméticamente coherente pero incorrecto, en silencio.
    #[test]
    fn componentes_no_finitos_o_fuera_de_rango_se_rechazan() {
        for bad in [
            "2020-01-10T14:33:nan",
            "2020-01-10T14:33:inf",
            "2020-01-10T14:nan:52",
            "2020-13-01T00:00:00", // mes 13
            "2020-01-32T00:00:00", // día 32
            "2020-01-10T25:00:00", // hora 25
            "2020-01-10T14:99:00", // minuto 99
            "2020-01-10T14:33:99", // segundo 99
            "2020-01-10T14:3x:52", // minuto ilegible (antes → 14:00:00)
        ] {
            assert!(
                parse_timestamp(bad).is_none(),
                "'{bad}' debería rechazarse, dio {:?}",
                parse_timestamp(bad)
            );
        }
        // el camino feliz sigue intacto, incluida la fracción de segundo
        assert!(parse_timestamp("2020-01-10T14:33:52.5Z").is_some());
        assert!(parse_timestamp("2020-01-10").is_some());
        assert!(parse_timestamp("2020-01-10T14").is_some());
    }

    /// `read_csv` sobre un CSV con las marcas de hueco habituales: antes un
    /// timestamp con segundos `nan` producía un MJD NaN y hacía **panic** al
    /// ordenar, y una elevación `NaN` entraba en silencio a la interpolación.
    #[test]
    fn read_csv_rechaza_marcas_de_hueco_en_vez_de_panicar() {
        let dir = std::env::temp_dir();
        let write = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            p
        };

        let p = write(
            "strandline_tide_nan_elev.csv",
            "timestamp,tide_m\n2000-01-15T14:30:00Z,0.5\n2000-01-16T14:30:00Z,NaN\n",
        );
        let err = read_csv(&p, "timestamp", "tide_m").unwrap_err();
        assert!(
            format!("{err}").contains("no finita"),
            "se esperaba error de elevación no finita, hubo: {err}"
        );

        let p = write(
            "strandline_tide_nan_stamp.csv",
            "timestamp,tide_m\n2000-01-15T14:30:00Z,0.5\n2000-01-16T14:30:nan,0.7\n",
        );
        // lo importante: es un Err, no un panic
        assert!(read_csv(&p, "timestamp", "tide_m").is_err());

        // fila truncada: antes desaparecía en silencio
        let p = write(
            "strandline_tide_trunc.csv",
            "timestamp,tide_m\n2000-01-15T14:30:00Z,0.5\n2000-01-16T14:30:00Z\n",
        );
        assert!(read_csv(&p, "timestamp", "tide_m").is_err());

        // camino feliz intacto
        let p = write(
            "strandline_tide_ok.csv",
            "timestamp,tide_m\n2000-01-16T14:30:00Z,0.7\n2000-01-15T14:30:00Z,0.5\n",
        );
        let s = read_csv(&p, "timestamp", "tide_m").expect("CSV válido");
        assert_eq!(s.elevations.len(), 2);
        // se ordena por tiempo
        assert!(s.times_mjd[0] < s.times_mjd[1]);
        assert!((s.elevations[0] - 0.5).abs() < 1e-12);
    }

    /// Un constituyente repetido se sumaba dos veces en `predict` (para M2 en
    /// Chile central, ~0.4 m de marea espuria) sin ninguna advertencia.
    #[test]
    fn constituyente_duplicado_se_rechaza() {
        let p = std::env::temp_dir().join("strandline_const_dup.csv");
        std::fs::write(
            &p,
            "name,amplitude,phase_g\nM2,0.42,100.0\nm2,0.42,100.0\n",
        )
        .unwrap();
        let err = read_constituents_csv(&p).unwrap_err();
        assert!(
            format!("{err}").contains("duplicado"),
            "se esperaba error de duplicado, hubo: {err}"
        );
    }

    /// Un CSV con mojibake (Latin-1 releído como UTF-8) hacía panic al cortar
    /// el `&str` por índice de byte dentro de un carácter multibyte.
    #[test]
    fn entrada_no_ascii_se_rechaza_sin_panic() {
        assert!(parse_timestamp("2020-01-10Té4:33").is_none());
        assert!(parse_timestamp("2020-01-10T14:33:52+0é0").is_none());
        assert!(parse_timestamp("2020-01-10T14:33:52+é").is_none());
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
