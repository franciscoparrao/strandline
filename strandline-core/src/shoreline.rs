//! Pipeline de extracción de shoreline: bandas → MNDWI (± clasificador) →
//! umbral → contornos subpixel → coordenadas de mundo (convención CoastSat).

use std::path::Path;
use std::time::Instant;

use surtgis_core::ndarray::Array2;
use surtgis_core::io::read_geotiff_bands;
use surtgis_core::raster::{GeoTransform, Raster};

use crate::classify::{classify_pixels, Classification};
use crate::contour::find_contours;
use crate::error::{Result, StrandlineError};
use crate::index::{mndwi, nd_index};
use crate::otsu::{balanced_otsu, otsu_threshold};

/// Parámetros de extracción.
#[derive(Debug, Clone)]
pub struct ExtractParams {
    /// Banda Green, 1-based (default 2: ms de CoastSat L8 = B,G,R,NIR,SWIR1).
    pub green_band: usize,
    /// Banda SWIR1, 1-based (default 5).
    pub swir1_band: usize,
    /// Largo mínimo de shoreline en metros (default 200, como CoastSat).
    pub min_length_m: f64,
    /// Umbral fijo; si es `Some`, se usa directo y se ignoran
    /// `use_classifier`/`refine_radius_px`.
    pub level: Option<f32>,
    /// Refinamiento del umbral cuando `use_classifier = false`: segunda
    /// pasada de Otsu restringida a un corredor de `refine_radius_px`
    /// píxeles alrededor del contorno inicial. `0` = desactivado.
    pub refine_radius_px: usize,
    /// Usa el clasificador de píxeles (MLP portado de CoastSat) + Otsu
    /// balanceado sand/water en vez de Otsu global. Requiere las 5 bandas
    /// L8 en orden CoastSat (B,G,R,NIR,SWIR1).
    pub use_classifier: bool,
    /// Replica el quirk de CoastSat `find_wl_contours2`: el umbral que
    /// contornea el MNDWI se calcula en realidad sobre el NDWI (columnas
    /// intercambiadas en el `int_all` de CoastSat — ver `spike/RESULTS.md`).
    /// Implica `use_classifier = true`.
    pub coastsat_compat: bool,
    /// Área mínima (m²) para conservar un blob sand/water tras clasificar
    /// (default 500, como `min_beach_area` de CoastSat). Sólo aplica si
    /// `use_classifier = true`.
    pub min_beach_area_m2: f64,
}

impl Default for ExtractParams {
    fn default() -> Self {
        Self {
            green_band: 2,
            swir1_band: 5,
            min_length_m: 200.0,
            level: None,
            refine_radius_px: 10,
            use_classifier: false,
            coastsat_compat: false,
            min_beach_area_m2: 500.0,
        }
    }
}

/// Resumen de la clasificación de píxeles, cuando `use_classifier = true`.
#[derive(Debug, Clone, Copy)]
pub struct ClassificationSummary {
    /// Píxeles clasificados como sand tras el filtro de área mínima.
    pub sand_px: usize,
    /// Píxeles clasificados como whitewater (sin filtro de área).
    pub whitewater_px: usize,
    /// Píxeles clasificados como water tras el filtro de área mínima.
    pub water_px: usize,
}

/// Resultado de la extracción para una escena.
#[derive(Debug)]
pub struct Shoreline {
    /// Polilíneas en coordenadas de mundo (CRS nativo del raster).
    pub lines: Vec<Vec<(f64, f64)>>,
    /// Umbral aplicado (dominio MNDWI, salvo `coastsat_compat` donde el
    /// valor numérico proviene del Otsu del NDWI — ver `coastsat_compat`).
    pub threshold: f32,
    /// Código EPSG del CRS nativo, si el raster lo declara.
    pub epsg: Option<u32>,
    /// Resumen de clasificación, si `use_classifier = true`.
    pub classification: Option<ClassificationSummary>,
    /// Tiempos por etapa (ms): lectura, mndwi, clasificación, umbral, contornos.
    pub timings_ms: [f64; 5],
}

/// Convierte un punto de imagen fraccional `(row, col)` a mundo con la
/// convención de CoastSat: geotransform GDAL aplicado directo, sin offset
/// de medio píxel.
#[inline]
fn pix2world(gt: &GeoTransform, row: f64, col: f64) -> (f64, f64) {
    let g = gt.to_gdal();
    (
        g[0] + col * g[1] + row * g[2],
        g[3] + col * g[4] + row * g[5],
    )
}

fn line_length(line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// Segunda pasada de Otsu restringida al corredor alrededor del contorno
/// inicial extraído con `initial_level`. Devuelve `None` si no hay contornos
/// o píxeles suficientes en el corredor.
fn refine_threshold(ndwi: &Raster<f32>, initial_level: f32, radius: usize) -> Option<f32> {
    let contours = find_contours(ndwi.data().view(), initial_level as f64);
    if contours.is_empty() {
        return None;
    }
    let (rows, cols) = ndwi.shape();
    let mut near = vec![false; rows * cols];
    for c in &contours {
        for &(r, col) in c {
            let (r, col) = (r.round() as isize, col.round() as isize);
            for dr in -(radius as isize)..=(radius as isize) {
                for dc in -(radius as isize)..=(radius as isize) {
                    let (rr, cc) = (r + dr, col + dc);
                    if rr >= 0 && cc >= 0 && (rr as usize) < rows && (cc as usize) < cols {
                        near[rr as usize * cols + cc as usize] = true;
                    }
                }
            }
        }
    }
    let data = ndwi.data();
    let vals = near
        .iter()
        .enumerate()
        .filter(|(_, &n)| n)
        .map(|(i, _)| data[[i / cols, i % cols]]);
    otsu_threshold(vals).ok()
}

/// Recolecta los valores finitos de `img` en las posiciones donde `mask`
/// es `true`.
fn values_at(img: &Raster<f32>, mask: &Array2<bool>) -> Vec<f32> {
    let d = img.data();
    let (rows, cols) = mask.dim();
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if mask[[r, c]] {
                let v = d[[r, c]];
                if v.is_finite() {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// Extrae la shoreline de un GeoTIFF multiespectral.
///
/// `qa_path`: máscara QA_PIXEL opcional (Landsat C02) para excluir nubes.
pub fn extract_shoreline(
    ms_path: &Path,
    qa_path: Option<&Path>,
    params: &ExtractParams,
) -> Result<Shoreline> {
    let t0 = Instant::now();
    let bands: Vec<Raster<f32>> = read_geotiff_bands(ms_path)?;
    let n = bands.len();
    for b in [params.green_band, params.swir1_band] {
        if b == 0 || b > n {
            return Err(StrandlineError::BandOutOfRange {
                requested: b,
                available: n,
            });
        }
    }
    let use_classifier = params.use_classifier || params.coastsat_compat;
    if use_classifier && n < 5 {
        return Err(StrandlineError::BandOutOfRange {
            requested: 5,
            available: n,
        });
    }
    let green = &bands[params.green_band - 1];
    let swir1 = &bands[params.swir1_band - 1];

    let qa: Option<Raster<u16>> = match qa_path {
        Some(p) => Some(surtgis_core::io::read_geotiff(p, Some(0))?),
        None => None,
    };
    if let Some(q) = &qa {
        if q.shape() != green.shape() {
            return Err(StrandlineError::ShapeMismatch(format!(
                "QA {:?} vs ms {:?}",
                q.shape(),
                green.shape()
            )));
        }
    }
    let t_read = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let ndwi = mndwi(green, swir1, qa.as_ref());
    let t_mndwi = t1.elapsed().as_secs_f64() * 1000.0;

    let t_classify_start = Instant::now();
    let classification: Option<Classification> = if use_classifier {
        let px_area = green.cell_size().powi(2);
        let min_area_px = ((params.min_beach_area_m2 / px_area).round() as usize).max(1);
        Some(classify_pixels(&bands, qa.as_ref(), min_area_px)?)
    } else {
        None
    };
    let t_classify = t_classify_start.elapsed().as_secs_f64() * 1000.0;

    let t2 = Instant::now();
    let (threshold, contours) = match params.level {
        Some(l) => (l, find_contours(ndwi.data().view(), l as f64)),
        None => match &classification {
            Some(cls) if params.coastsat_compat => {
                // El quirk de CoastSat: el umbral que contornea el MNDWI
                // (signo CoastSat, SWIR−G) en realidad se calcula sobre el
                // NDWI (NIR−G). Ver spike/RESULTS.md.
                let nir = &bands[3];
                let ndwi_cs = nd_index(nir, green, qa.as_ref());
                let mwi_cs = nd_index(swir1, green, qa.as_ref());
                let water_vals = values_at(&ndwi_cs, &cls.water);
                let sand_vals = values_at(&ndwi_cs, &cls.sand);
                let t = balanced_otsu(&water_vals, &sand_vals)?;
                (t, find_contours(mwi_cs.data().view(), t as f64))
            }
            Some(cls) => {
                let water_vals = values_at(&ndwi, &cls.water);
                let sand_vals = values_at(&ndwi, &cls.sand);
                let t = balanced_otsu(&water_vals, &sand_vals)?;
                (t, find_contours(ndwi.data().view(), t as f64))
            }
            None => {
                let global = otsu_threshold(ndwi.data().iter().cloned())?;
                let t = if params.refine_radius_px == 0 {
                    global
                } else {
                    refine_threshold(&ndwi, global, params.refine_radius_px).unwrap_or(global)
                };
                (t, find_contours(ndwi.data().view(), t as f64))
            }
        },
    };
    let t_otsu = t2.elapsed().as_secs_f64() * 1000.0;

    let t3 = Instant::now();
    let gt = green.transform().clone();
    let lines: Vec<Vec<(f64, f64)>> = contours
        .into_iter()
        .map(|c| {
            c.into_iter()
                .map(|(r, col)| pix2world(&gt, r, col))
                .collect::<Vec<_>>()
        })
        .filter(|l: &Vec<(f64, f64)>| line_length(l) >= params.min_length_m)
        .collect();
    let t_contour = t3.elapsed().as_secs_f64() * 1000.0;

    let classification_summary = classification.as_ref().map(|cls| {
        let (sand_px, whitewater_px, water_px) = cls.counts();
        ClassificationSummary {
            sand_px,
            whitewater_px,
            water_px,
        }
    });

    Ok(Shoreline {
        lines,
        threshold,
        epsg: green.crs().and_then(|c| c.epsg()),
        classification: classification_summary,
        timings_ms: [t_read, t_mndwi, t_classify, t_otsu, t_contour],
    })
}

impl Shoreline {
    /// Serializa las polilíneas como FeatureCollection GeoJSON
    /// (coordenadas en el CRS nativo; el EPSG queda en `properties`).
    pub fn to_geojson(&self, scene: &str) -> serde_json::Value {
        let features: Vec<serde_json::Value> = self
            .lines
            .iter()
            .map(|line| {
                let coords: Vec<[f64; 2]> = line.iter().map(|&(x, y)| [x, y]).collect();
                serde_json::json!({
                    "type": "Feature",
                    "properties": {
                        "scene": scene,
                        "threshold": self.threshold,
                        "epsg": self.epsg,
                    },
                    "geometry": {
                        "type": "LineString",
                        "coordinates": coords,
                    }
                })
            })
            .collect();
        serde_json::json!({
            "type": "FeatureCollection",
            "features": features,
        })
    }
}
