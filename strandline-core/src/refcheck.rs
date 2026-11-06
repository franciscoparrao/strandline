//! Chequeo de la geometría de la referencia: ¿miden los transectos la costa
//! que se quiere estudiar?
//!
//! Una línea de referencia derivada de las propias detecciones puede
//! desviarse tierra adentro, siguiendo un estuario, una laguna o un río. Los
//! transectos generados desde ella miden esas orillas interiores, y sus series
//! pasan los controles de calidad estadísticos porque son internamente
//! consistentes. Este módulo compara cada transecto con una costa
//! independiente (por ejemplo la `natural=coastline` de OpenStreetMap) por
//! tres vías:
//!
//! 1. **Ancla**: distancia del ancla (punto medio del transecto, sobre la
//!    línea base) a la costa independiente.
//! 2. **Elevación** (opcional): altura del ancla en un modelo de elevación; un
//!    ancla a decenas de metros sobre el mar no está en una playa. Con un
//!    modelo de superficie (DSM, como Copernicus GLO-30) el criterio da falsos
//!    positivos donde hay vegetación o edificios.
//! 3. **Cruces medidos** (opcional): distancia a la costa de los puntos donde
//!    las líneas de costa de cada fecha cruzaron el transecto. Ve lo que el
//!    transecto midió de verdad, no solo dónde quedó su ancla.
//!
//! Un transecto pasa si cumple las dos primeras (la elevación solo si se
//! entregó un modelo). Un sitio se marca como contaminado si la fracción de
//! transectos que pasan es menor que `site_min_pass`.

use surtgis_core::raster::Raster;

use crate::error::{Result, StrandlineError};
use crate::proj::transform_points;
use crate::transects::Transect;

/// Umbrales del chequeo.
#[derive(Debug, Clone, Copy)]
pub struct RefcheckSettings {
    /// Distancia máxima (m) del ancla o de un cruce a la costa independiente.
    pub max_dist_m: f64,
    /// Elevación máxima (m) del ancla.
    pub max_elev_m: f64,
    /// Fracción mínima de transectos que pasan para no marcar el sitio.
    pub site_min_pass: f64,
}

impl Default for RefcheckSettings {
    fn default() -> Self {
        Self {
            max_dist_m: 200.0,
            max_elev_m: 5.0,
            site_min_pass: 0.5,
        }
    }
}

/// Resultado del chequeo para un transecto.
#[derive(Debug, Clone)]
pub struct TransectCheck {
    /// Ancla (punto medio del transecto).
    pub anchor: (f64, f64),
    /// Distancia del ancla a la costa independiente (m).
    pub anchor_dist_m: f64,
    /// Elevación del ancla (m), si se entregó un modelo y el punto tiene dato.
    pub anchor_elev_m: Option<f64>,
    /// Cruces medidos usados (0 si no se entregaron).
    pub n_crossings: usize,
    /// Mediana de la distancia de los cruces a la costa (m).
    pub crossing_median_dist_m: Option<f64>,
    /// Fracción de cruces a menos de `max_dist_m` de la costa.
    pub crossing_frac_near: Option<f64>,
    /// El ancla está a menos de `max_dist_m` de la costa.
    pub pass_dist: bool,
    /// El ancla está bajo `max_elev_m` (`None` sin modelo o sin dato).
    pub pass_elev: Option<bool>,
    /// Pasa los criterios disponibles del ancla.
    pub pass: bool,
}

/// Resumen del sitio.
#[derive(Debug, Clone, Copy)]
pub struct SiteSummary {
    pub n: usize,
    pub frac_pass_dist: f64,
    /// Fracción bajo `max_elev_m` entre los transectos con elevación.
    pub frac_pass_elev: Option<f64>,
    pub frac_pass: f64,
    pub contaminated: bool,
}

/// Distancia euclídea mínima de `p` a un conjunto de polilíneas.
pub fn distance_to_lines(p: (f64, f64), lines: &[Vec<(f64, f64)>]) -> f64 {
    let mut best = f64::INFINITY;
    for line in lines {
        if line.len() == 1 {
            best = best.min(((p.0 - line[0].0).powi(2) + (p.1 - line[0].1).powi(2)).sqrt());
        }
        for w in line.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
            best = best.min(((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt());
        }
    }
    best
}

/// Puntos medidos sobre un transecto: origen + distancia a lo largo de él.
pub fn crossing_points(t: &Transect, chainages: &[f64]) -> Vec<(f64, f64)> {
    let (dx, dy) = (t.end.0 - t.origin.0, t.end.1 - t.origin.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return Vec::new();
    }
    let (ux, uy) = (dx / len, dy / len);
    chainages
        .iter()
        .filter(|c| c.is_finite())
        .map(|&c| (t.origin.0 + c * ux, t.origin.1 + c * uy))
        .collect()
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    })
}

/// Chequea cada transecto. `coast` debe estar en el mismo CRS métrico que los
/// transectos. `anchor_elev[i]` es la elevación del ancla del transecto `i`
/// (ver [`sample_raster`]); `crossings[i]`, las distancias a lo largo del
/// transecto `i` medidas en cada fecha.
pub fn check_transects(
    transects: &[Transect],
    coast: &[Vec<(f64, f64)>],
    anchor_elev: Option<&[Option<f64>]>,
    crossings: Option<&[Vec<f64>]>,
    s: &RefcheckSettings,
) -> Vec<TransectCheck> {
    transects
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let anchor = t.midpoint();
            let anchor_dist_m = distance_to_lines(anchor, coast);
            let anchor_elev_m = anchor_elev.and_then(|e| e.get(i).copied().flatten());
            let pass_dist = anchor_dist_m <= s.max_dist_m;
            let pass_elev = anchor_elev_m.map(|z| z <= s.max_elev_m);
            let pts = crossings
                .and_then(|c| c.get(i))
                .map(|ch| crossing_points(t, ch))
                .unwrap_or_default();
            let dists: Vec<f64> = pts.iter().map(|&p| distance_to_lines(p, coast)).collect();
            let n_crossings = dists.len();
            let crossing_frac_near = (n_crossings > 0).then(|| {
                dists.iter().filter(|&&d| d <= s.max_dist_m).count() as f64 / n_crossings as f64
            });
            TransectCheck {
                anchor,
                anchor_dist_m,
                anchor_elev_m,
                n_crossings,
                crossing_median_dist_m: median(dists),
                crossing_frac_near,
                pass_dist,
                pass_elev,
                pass: pass_dist && pass_elev.unwrap_or(true),
            }
        })
        .collect()
}

/// Resume el chequeo de un sitio.
pub fn summarize(rows: &[TransectCheck], s: &RefcheckSettings) -> SiteSummary {
    let n = rows.len();
    let frac = |k: usize| if n == 0 { 0.0 } else { k as f64 / n as f64 };
    let with_elev: Vec<bool> = rows.iter().filter_map(|r| r.pass_elev).collect();
    let frac_pass = frac(rows.iter().filter(|r| r.pass).count());
    SiteSummary {
        n,
        frac_pass_dist: frac(rows.iter().filter(|r| r.pass_dist).count()),
        frac_pass_elev: (!with_elev.is_empty())
            .then(|| with_elev.iter().filter(|&&b| b).count() as f64 / with_elev.len() as f64),
        frac_pass,
        contaminated: n > 0 && frac_pass < s.site_min_pass,
    }
}

/// Lee la banda 1 de un GeoTIFF como `f32` (por ejemplo un modelo de
/// elevación para [`sample_raster`]).
pub fn read_raster(path: &std::path::Path) -> Result<Raster<f32>> {
    Ok(surtgis_core::io::read_geotiff(path, Some(0))?)
}

/// Valor de un raster (por ejemplo un modelo de elevación) en cada punto, con
/// el píxel que contiene al punto. Los puntos vienen en `points_epsg` y se
/// reproyectan al CRS del raster. `None` si el punto cae fuera o el píxel no
/// tiene dato.
pub fn sample_raster(
    raster: &Raster<f32>,
    points: &[(f64, f64)],
    points_epsg: u32,
) -> Result<Vec<Option<f64>>> {
    let raster_epsg = raster
        .crs()
        .and_then(|c| c.epsg())
        .ok_or_else(|| StrandlineError::UnsupportedCrs("el raster no declara EPSG".into()))?;
    let pts = transform_points(points, points_epsg, raster_epsg)?;
    let g = raster.transform().to_gdal();
    let (rows, cols) = raster.shape();
    let data = raster.data();
    let nodata = raster.nodata();
    Ok(pts
        .iter()
        .map(|&(x, y)| {
            // geotransform por esquina (convención GDAL): el píxel que
            // contiene al punto es el piso de la coordenada fraccional
            let c = ((x - g[0]) / g[1]).floor();
            let r = ((y - g[3]) / g[5]).floor();
            if r < 0.0 || c < 0.0 || r as usize >= rows || c as usize >= cols {
                return None;
            }
            let v = data[[r as usize, c as usize]];
            (v.is_finite() && !nodata.is_some_and(|nd| v == nd)).then_some(v as f64)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use surtgis_core::raster::GeoTransform;

    fn coast() -> Vec<Vec<(f64, f64)>> {
        // costa recta norte-sur en x = 1000
        vec![vec![(1000.0, 0.0), (1000.0, 5000.0)]]
    }

    fn transect_at(x_mid: f64, y: f64) -> Transect {
        Transect {
            origin: (x_mid + 250.0, y),
            end: (x_mid - 250.0, y),
        }
    }

    #[test]
    fn distancia_a_polilineas() {
        let c = coast();
        assert!((distance_to_lines((1150.0, 2000.0), &c) - 150.0).abs() < 1e-9);
        // más allá del extremo del segmento: distancia al vértice
        assert!((distance_to_lines((1000.0, 5300.0), &c) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn ancla_cerca_pasa_y_ancla_tierra_adentro_falla() {
        let ts = vec![transect_at(1050.0, 1000.0), transect_at(2500.0, 2000.0)];
        let r = check_transects(&ts, &coast(), None, None, &RefcheckSettings::default());
        assert!(r[0].pass && (r[0].anchor_dist_m - 50.0).abs() < 1e-9);
        assert!(!r[1].pass && (r[1].anchor_dist_m - 1500.0).abs() < 1e-9);
        assert!(
            r[0].pass_elev.is_none(),
            "sin modelo no hay criterio de elevación"
        );
    }

    #[test]
    fn la_elevacion_reprueba_un_ancla_cercana_pero_alta() {
        let ts = vec![transect_at(1050.0, 1000.0), transect_at(1050.0, 2000.0)];
        let elev = vec![Some(2.0), Some(40.0)];
        let r = check_transects(
            &ts,
            &coast(),
            Some(&elev),
            None,
            &RefcheckSettings::default(),
        );
        assert!(r[0].pass);
        assert!(r[1].pass_dist && r[1].pass_elev == Some(false) && !r[1].pass);
    }

    #[test]
    fn los_cruces_ven_lo_que_el_transecto_midio() {
        // ancla sobre la costa, pero las fechas cruzaron 800 m tierra adentro
        // (el transecto mide una laguna detrás de la playa)
        let t = transect_at(1000.0, 1000.0); // origen en x=1250, va hacia -x
                                             // chainage desde el origen: 250 m = la costa; 0 m = 250 m tierra adentro
        let near = vec![240.0, 250.0, 260.0];
        let far = vec![0.0, 0.0, 10.0];
        let r = check_transects(
            &[t, t],
            &coast(),
            None,
            Some(&[near, far]),
            &RefcheckSettings {
                max_dist_m: 100.0,
                ..Default::default()
            },
        );
        assert_eq!(r[0].crossing_frac_near, Some(1.0));
        assert_eq!(r[1].crossing_frac_near, Some(0.0));
        assert!(r[1].crossing_median_dist_m.unwrap() > 200.0);
        assert!(
            r[0].pass && r[1].pass,
            "el ancla pasa en los dos; solo los cruces distinguen"
        );
    }

    #[test]
    fn regla_de_sitio() {
        let s = RefcheckSettings::default();
        let ts: Vec<Transect> = (0..10)
            .map(|i| transect_at(if i < 4 { 1050.0 } else { 3000.0 }, 100.0 * i as f64))
            .collect();
        let sum = summarize(&check_transects(&ts, &coast(), None, None, &s), &s);
        assert_eq!(sum.n, 10);
        assert!((sum.frac_pass - 0.4).abs() < 1e-12);
        assert!(sum.contaminated, "40 % < 50 %");
    }

    #[test]
    fn muestreo_de_un_raster_en_otro_crs() {
        // raster en geográficas (como GLO-30), puntos en UTM 19S
        let mut dem =
            Raster::<f32>::from_array(ndarray::Array2::from_shape_fn((10, 10), |(r, c)| {
                (r * 10 + c) as f32
            }));
        dem.set_transform(GeoTransform::new(-71.60, -33.00, 0.001, -0.001));
        dem.set_crs(Some(surtgis_core::CRS::from_epsg(4326)));
        let lonlat = vec![(-71.5955, -33.0025), (-80.0, -33.0)];
        let utm = transform_points(&lonlat, 4326, 32719).unwrap();
        let v = sample_raster(&dem, &utm, 32719).unwrap();
        // fila 2, columna 4
        assert_eq!(v[0], Some(24.0));
        assert_eq!(v[1], None, "fuera del raster");
    }
}
