//! Chequeo de sensores pareados: desfase de la línea de costa entre dos
//! sensores que observaron las mismas playas en los mismos años.
//!
//! Para cada transecto y cada par de sensores `(a, b)` con un período de
//! traslape `[y0, y1]` (años calendario, inclusivos), toma la mediana del
//! chainage de cada sensor sobre las observaciones de esos años y reporta
//! `mediana_b − mediana_a`. Restringir al traslape saca el cambio real de la
//! costa de la comparación, y la mediana sobre los años quita casi toda la
//! dispersión mareal y estacional. Una extracción consistente da desfases
//! cerca de cero; un sesgo que depende del sensor da un escalón en cada cambio
//! de sensor, que una tasa lineal absorbe como tendencia.
//!
//! El signo se reporta **hacia el mar positivo**, así que necesita el sentido
//! del chainage ([`ChainageDirection`]).
//!
//! Es la definición del test de sensores pareados del estudio de 60 playas
//! (`58_paper1_new_diagnostics.py`, función `offsets`): mediana por transecto y
//! sensor dentro de los años del par, diferencia `b − a`, transectos sin
//! observaciones de alguno de los dos excluidos.

use std::collections::{BTreeMap, HashMap};

use crate::tide::ChainageDirection;

/// Un par de sensores y su período de traslape (años calendario inclusivos).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorPair {
    /// Sensor anterior (el desfase se mide respecto de él).
    pub a: String,
    /// Sensor posterior.
    pub b: String,
    /// Primer año del traslape.
    pub y0: i32,
    /// Último año del traslape.
    pub y1: i32,
}

impl SensorPair {
    /// Etiqueta `b-a`, la convención de las tablas del estudio.
    pub fn label(&self) -> String {
        format!("{}-{}", self.b, self.a)
    }
}

/// Los tres traslapes de Landsat: L7 contra L5 en 2003–2011, L8 contra L7 en
/// 2013–2021 y L9 contra L8 en 2022–2025. Las etiquetas son las de CoastSat
/// (`L5`, `L7`, `L8`, `L9`).
pub fn landsat_pairs() -> Vec<SensorPair> {
    [("L5", "L7", 2003, 2011), ("L7", "L8", 2013, 2021), ("L8", "L9", 2022, 2025)]
        .into_iter()
        .map(|(a, b, y0, y1)| SensorPair {
            a: a.into(),
            b: b.into(),
            y0,
            y1,
        })
        .collect()
}

/// Una observación: año calendario, chainage (m) y sensor.
#[derive(Debug, Clone)]
pub struct Observation {
    pub year: i32,
    pub chainage: f64,
    pub sensor: String,
}

/// Desfase de un transecto para un par.
#[derive(Debug, Clone)]
pub struct TransectOffset {
    pub transect: String,
    pub n_a: usize,
    pub n_b: usize,
    pub median_a: f64,
    pub median_b: f64,
    /// `median_b − median_a`, hacia el mar positivo.
    pub offset: f64,
}

/// Resumen de un par sobre todos los transectos.
#[derive(Debug, Clone, Copy)]
pub struct OffsetSummary {
    pub n: usize,
    pub median: f64,
    pub q25: f64,
    pub q75: f64,
}

/// Resultado de un par.
#[derive(Debug, Clone)]
pub struct PairOffsets {
    pub pair: SensorPair,
    /// Ordenados por nombre de transecto.
    pub transects: Vec<TransectOffset>,
    /// `None` si ningún transecto tiene observaciones de los dos sensores.
    pub summary: Option<OffsetSummary>,
}

/// Año calendario (gregoriano) de una fecha en MJD.
pub fn calendar_year_from_mjd(mjd: f64) -> i32 {
    crate::stl::civil_from_days(mjd.floor() as i64 - 40_587).0
}

/// Mediana con la convención de numpy/pandas (promedio de los dos centrales).
fn median(v: &mut [f64]) -> f64 {
    percentile(v, 50.0)
}

/// Percentil con interpolación lineal (la de `numpy.percentile` por defecto).
/// `v` no debe estar vacío; se ordena en el lugar.
pub(crate) fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n == 1 {
        return v[0];
    }
    let rank = p / 100.0 * (n - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    v[lo] + (v[hi] - v[lo]) * (rank - lo as f64)
}

/// Calcula los desfases pareados.
///
/// `by_transect` agrupa las observaciones por transecto; las no finitas se
/// ignoran. Un transecto entra en un par si tiene al menos `min_obs`
/// observaciones de cada sensor dentro del traslape (1 reproduce la
/// definición del estudio).
pub fn paired_offsets(
    by_transect: &HashMap<String, Vec<Observation>>,
    pairs: &[SensorPair],
    min_obs: usize,
    direction: ChainageDirection,
) -> Vec<PairOffsets> {
    let sign = match direction {
        ChainageDirection::Seaward => 1.0,
        ChainageDirection::Landward => -1.0,
    };
    let min_obs = min_obs.max(1);
    // orden determinista: el resultado no depende del orden del HashMap
    let ordered: BTreeMap<&String, &Vec<Observation>> = by_transect.iter().collect();

    pairs
        .iter()
        .map(|pair| {
            let mut transects = Vec::new();
            for (name, obs) in &ordered {
                let pick = |s: &str| -> Vec<f64> {
                    obs.iter()
                        .filter(|o| {
                            o.sensor == s
                                && o.year >= pair.y0
                                && o.year <= pair.y1
                                && o.chainage.is_finite()
                        })
                        .map(|o| o.chainage)
                        .collect()
                };
                let (mut va, mut vb) = (pick(&pair.a), pick(&pair.b));
                if va.len() < min_obs || vb.len() < min_obs {
                    continue;
                }
                let (n_a, n_b) = (va.len(), vb.len());
                let (median_a, median_b) = (median(&mut va), median(&mut vb));
                transects.push(TransectOffset {
                    transect: (*name).clone(),
                    n_a,
                    n_b,
                    median_a,
                    median_b,
                    offset: sign * (median_b - median_a),
                });
            }
            let summary = if transects.is_empty() {
                None
            } else {
                let mut off: Vec<f64> = transects.iter().map(|t| t.offset).collect();
                Some(OffsetSummary {
                    n: off.len(),
                    median: percentile(&mut off, 50.0),
                    q25: percentile(&mut off, 25.0),
                    q75: percentile(&mut off, 75.0),
                })
            };
            PairOffsets {
                pair: pair.clone(),
                transects,
                summary,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(year: i32, chainage: f64, sensor: &str) -> Observation {
        Observation {
            year,
            chainage,
            sensor: sensor.into(),
        }
    }

    #[test]
    fn anio_calendario_en_los_bordes() {
        // MJD 56293 = 2013-01-01, 56292 = 2012-12-31, 40587 = 1970-01-01
        assert_eq!(calendar_year_from_mjd(56293.0), 2013);
        assert_eq!(calendar_year_from_mjd(56292.9), 2012);
        assert_eq!(calendar_year_from_mjd(40587.0), 1970);
    }

    #[test]
    fn percentil_igual_a_numpy() {
        // numpy.percentile([1,2,3,4], [25,50,75]) = [1.75, 2.5, 3.25]
        let mut v = vec![4.0, 1.0, 3.0, 2.0];
        assert!((percentile(&mut v, 25.0) - 1.75).abs() < 1e-12);
        assert!((percentile(&mut v, 50.0) - 2.5).abs() < 1e-12);
        assert!((percentile(&mut v, 75.0) - 3.25).abs() < 1e-12);
    }

    /// Un escalón de +20 m hacia el mar al pasar de L7 a L8 se recupera, y las
    /// observaciones fuera del traslape no entran.
    #[test]
    fn recupera_un_escalon_entre_sensores() {
        let mut m = HashMap::new();
        let mut v = Vec::new();
        for y in 2013..=2021 {
            v.push(obs(y, 100.0 + (y % 3) as f64, "L7"));
            v.push(obs(y, 120.0 + (y % 3) as f64, "L8"));
        }
        v.push(obs(2010, 500.0, "L7")); // fuera del traslape
        v.push(obs(2024, -500.0, "L8"));
        m.insert("T001".to_string(), v);
        let r = paired_offsets(&m, &landsat_pairs(), 1, ChainageDirection::Seaward);
        let l8 = r.iter().find(|p| p.pair.label() == "L8-L7").unwrap();
        assert_eq!(l8.transects.len(), 1);
        assert!((l8.transects[0].offset - 20.0).abs() < 1e-12);
        // sin L5 ni L9 en sus traslapes, esos pares quedan vacíos
        assert!(r.iter().find(|p| p.pair.label() == "L7-L5").unwrap().summary.is_none());
    }

    /// Con el chainage creciendo hacia tierra, el mismo escalón hacia el mar
    /// es una disminución del chainage y debe reportarse igual, positivo.
    #[test]
    fn el_signo_sigue_al_sentido_del_chainage() {
        let mut m = HashMap::new();
        m.insert(
            "T1".to_string(),
            vec![obs(2015, 100.0, "L7"), obs(2015, 80.0, "L8")],
        );
        let r = paired_offsets(&m, &landsat_pairs(), 1, ChainageDirection::Landward);
        let l8 = r.iter().find(|p| p.pair.label() == "L8-L7").unwrap();
        assert!((l8.transects[0].offset - 20.0).abs() < 1e-12);
    }

    #[test]
    fn min_obs_y_no_finitos() {
        let mut m = HashMap::new();
        m.insert(
            "T1".to_string(),
            vec![
                obs(2015, 1.0, "L7"),
                obs(2016, f64::NAN, "L7"),
                obs(2015, 2.0, "L8"),
                obs(2016, 3.0, "L8"),
            ],
        );
        let p = landsat_pairs();
        let r = paired_offsets(&m, &p, 2, ChainageDirection::Seaward);
        assert!(r[1].transects.is_empty(), "L7 tiene una sola obs. finita");
        let r = paired_offsets(&m, &p, 1, ChainageDirection::Seaward);
        assert!((r[1].transects[0].offset - 1.5).abs() < 1e-12);
    }
}
