//! Subcomando `fetch`: adquisición cloud-native de escenas desde un
//! catálogo STAC (Microsoft Planetary Computer) **sin descargar las
//! escenas completas** — solo la ventana del área de interés, vía HTTP
//! range requests sobre los COGs (S2 del roadmap SOTA).
//!
//! Produce por escena el par `*_ms.tif` (5 bandas en orden CoastSat:
//! B,G,R,NIR,SWIR1, reflectancia f32) + `*_mask.tif` (QA_PIXEL de Landsat
//! o SCL de Sentinel-2, u16) que consumen `extract`/`profile`.
//!
//! Colecciones soportadas:
//! - `landsat-c2-l2`: reflectancia superficial `DN·0.0000275 − 0.2`,
//!   30 m, máscara `qa_pixel`. Cubre el archivo completo 1984–presente:
//!   TM (L4/L5), ETM+ (L7), OLI (L8/L9) — mismos nombres de asset común
//!   (`blue`, `nir08`, `swir16`, ...) y mismo escalado de reflectancia
//!   para los tres sensores en Planetary Computer (verificado contra el
//!   catálogo STAC real). Filtrable por `--sensors`.
//! - `sentinel-2-l2a`: reflectancia `(DN + offset)/10000`, donde el
//!   offset es −1000 para escenas con processing baseline ≥ 04.00
//!   (`BOA_ADD_OFFSET`, introducido el 2022-01-25) y 0 antes — se decide
//!   por el `s2:processing_baseline` del item STAC, no por la fecha,
//!   porque ESA reprocesa escenas antiguas con baselines nuevos.
//!   B02/B03/B04/B08 a 10 m, B11 (20 m) re-muestreado bilinealmente a la
//!   grilla de 10 m, máscara `SCL` (nearest).

use std::path::Path;

use anyhow::Context;
use strandline_core::proj::transform_points;
use surtgis_cloud::sync_api::{CogReaderBlocking, StacClientBlocking};
use surtgis_cloud::{BBox, CogReaderOptions, StacCatalog, StacClientOptions, StacSearchParams};
use surtgis_core::io::{write_geotiff, write_geotiff_stack, GeoTiffOptions};
use surtgis_core::raster::Raster;

/// Bandas por colección: claves de asset en orden CoastSat + máscara.
fn collection_assets(collection: &str) -> anyhow::Result<([&'static str; 5], &'static str)> {
    match collection {
        "landsat-c2-l2" => Ok((["blue", "green", "red", "nir08", "swir16"], "qa_pixel")),
        "sentinel-2-l2a" => Ok((["B02", "B03", "B04", "B08", "B11"], "SCL")),
        other => {
            anyhow::bail!("colección no soportada: '{other}' (usa landsat-c2-l2 | sentinel-2-l2a)")
        }
    }
}

/// Reflectancia desde DN según la colección; DN de relleno (0) → NaN.
/// `dn_offset` solo aplica a sentinel-2-l2a (ver [`s2_dn_offset`]).
fn to_reflectance(collection: &str, dn: f32, dn_offset: f32) -> f32 {
    if dn == 0.0 {
        return f32::NAN;
    }
    match collection {
        "landsat-c2-l2" => dn * 0.000_027_5 - 0.2,
        _ => (dn + dn_offset) / 10_000.0, // sentinel-2-l2a
    }
}

/// Offset de DN para Sentinel-2 L2A: el processing baseline 04.00 sumó
/// +1000 a los DN (`BOA_ADD_OFFSET = −1000`); sin restarlo la
/// reflectancia queda corrida +0.1 y el MNDWI sale mal en silencio.
/// Se decide por `s2:processing_baseline` (las escenas antiguas pueden
/// venir reprocesadas con baseline nuevo); si el item no trae la
/// property, cae al corte por fecha de adquisición (2022-01-25).
fn s2_dn_offset(baseline: Option<&str>, datetime: Option<&str>) -> f32 {
    if let Some(b) = baseline.and_then(|s| s.trim().parse::<f64>().ok()) {
        return if b >= 4.0 { -1000.0 } else { 0.0 };
    }
    match datetime {
        Some(dt) if dt >= "2022-01-25" => -1000.0,
        _ => 0.0,
    }
}

/// Filtro de sensor Landsat por `platform` STAC (`landsat-4/5/7/8/9`).
/// `sensors` es uno de `tm` (L4/L5), `etm` (L7), `oli` (L8/L9), `all`.
/// `None` (item sin `platform`) nunca pasa, salvo que el filtro sea
/// inaplicable (colecciones no-Landsat no llaman a esta función).
fn sensor_allowed(platform: Option<&str>, sensors: &str) -> bool {
    let Some(p) = platform else { return false };
    match sensors {
        "all" => true,
        "tm" => p.contains("landsat-4") || p.contains("landsat-5"),
        "etm" => p.contains("landsat-7"),
        "oli" => p.contains("landsat-8") || p.contains("landsat-9"),
        _ => false,
    }
}

/// Fecha (inclusive) desde la que las escenas Landsat-7 ETM+ llevan las
/// franjas de datos faltantes por la falla del Scan Line Corrector.
const L7_SLC_FAILURE_DATE: &str = "2003-05-31";

/// Re-muestrea `src` a la grilla de `target` (mismo CRS): para cada píxel
/// del target evalúa el punto mundo en `src` (bilineal o nearest).
fn resample_to_grid(src: &Raster<f32>, target: &Raster<f32>, bilinear: bool) -> Raster<f32> {
    let (rows, cols) = target.shape();
    let tg = target.transform().to_gdal();
    let sg = src.transform().to_gdal();
    let sd = src.data();
    let (srows, scols) = src.shape();
    let mut out: Raster<f32> = target.with_same_meta(rows, cols);
    let od = out.data_mut();
    for r in 0..rows {
        for c in 0..cols {
            // centro del píxel target en mundo (convención pix2world directa)
            let x = tg[0] + c as f64 * tg[1];
            let y = tg[3] + r as f64 * tg[5];
            let sc = (x - sg[0]) / sg[1];
            let sr = (y - sg[3]) / sg[5];
            od[[r, c]] = if bilinear {
                bilin(sd, sr, sc, srows, scols)
            } else {
                let ri = sr.round() as isize;
                let ci = sc.round() as isize;
                if ri < 0 || ci < 0 || ri >= srows as isize || ci >= scols as isize {
                    f32::NAN
                } else {
                    sd[[ri as usize, ci as usize]]
                }
            };
        }
    }
    out
}

fn bilin(d: &ndarray::Array2<f32>, r: f64, c: f64, rows: usize, cols: usize) -> f32 {
    if r < 0.0 || c < 0.0 || r > (rows - 1) as f64 || c > (cols - 1) as f64 {
        return f32::NAN;
    }
    let (r0, c0) = (r.floor() as usize, c.floor() as usize);
    let (r1, c1) = ((r0 + 1).min(rows - 1), (c0 + 1).min(cols - 1));
    let (fr, fc) = ((r - r0 as f64) as f32, (c - c0 as f64) as f32);
    let (v00, v01, v10, v11) = (d[[r0, c0]], d[[r0, c1]], d[[r1, c0]], d[[r1, c1]]);
    if !(v00.is_finite() && v01.is_finite() && v10.is_finite() && v11.is_finite()) {
        return f32::NAN;
    }
    v00 * (1.0 - fr) * (1.0 - fc) + v01 * (1.0 - fr) * fc + v10 * fr * (1.0 - fc) + v11 * fr * fc
}

/// Ejecuta el fetch. `bbox_lonlat = [oeste, sur, este, norte]`.
/// `sensors` (`tm`|`etm`|`oli`|`all`) solo aplica a `landsat-c2-l2`.
#[allow(clippy::too_many_arguments)]
pub fn run(
    collection: &str,
    bbox_lonlat: [f64; 4],
    datetime: &str,
    max_items: usize,
    max_cloud: f64,
    sensors: &str,
    skip_l7_slc: bool,
    out_dir: &Path,
) -> anyhow::Result<usize> {
    let (band_keys, mask_key) = collection_assets(collection)?;
    if collection == "landsat-c2-l2" {
        anyhow::ensure!(
            matches!(sensors, "tm" | "etm" | "oli" | "all"),
            "--sensors debe ser uno de: tm, etm, oli, all (recibido '{sensors}')"
        );
    }
    std::fs::create_dir_all(out_dir)?;

    // El cap de paginación del cliente (default 100) debe superar a
    // max_items ANTES del filtro de nubes — si no, en series largas la
    // búsqueda se queda con la primera página (los items más recientes)
    // y descarta el resto en silencio.
    let client = StacClientBlocking::new(
        StacCatalog::PlanetaryComputer,
        StacClientOptions {
            max_items: (max_items.saturating_mul(10)).max(1000),
            ..StacClientOptions::default()
        },
    )
    .context("creando cliente STAC")?;

    let params = StacSearchParams {
        bbox: Some(bbox_lonlat.to_vec()),
        datetime: Some(datetime.to_string()),
        collections: Some(vec![collection.to_string()]),
        limit: Some(100),
        token: None,
    };
    let mut items = client.search_all(&params).context("búsqueda STAC")?;
    items.retain(|it| it.properties.eo_cloud_cover.unwrap_or(0.0) <= max_cloud);
    if collection == "landsat-c2-l2" {
        items.retain(|it| sensor_allowed(it.properties.platform.as_deref(), sensors));
        if skip_l7_slc {
            items.retain(|it| {
                let is_l7 = it
                    .properties
                    .platform
                    .as_deref()
                    .is_some_and(|p| p.contains("landsat-7"));
                let post_failure = it
                    .properties
                    .datetime
                    .as_deref()
                    .is_some_and(|dt| dt >= L7_SLC_FAILURE_DATE);
                !(is_l7 && post_failure)
            });
        }
    }
    items.sort_by(|a, b| a.properties.datetime.cmp(&b.properties.datetime));
    items.truncate(max_items);
    eprintln!(
        "{}: {} escenas tras filtro de nubes (≤{max_cloud}%)",
        collection,
        items.len()
    );

    let mut written = 0usize;
    let mut failed_scenes = 0usize;
    let mut skipped_existing = 0usize;
    for item in &items {
        let date = item
            .properties
            .datetime
            .as_deref()
            .unwrap_or("unknown")
            .replace(':', "-");
        let epsg = item
            .epsg()
            .with_context(|| format!("{}: item sin proj:epsg", item.id))?;

        let dn_offset = if collection == "sentinel-2-l2a" {
            let baseline = item
                .properties
                .extra
                .get("s2:processing_baseline")
                .and_then(|v| v.as_str());
            if baseline.is_none() {
                eprintln!(
                    "  {}: sin s2:processing_baseline, offset BOA decidido por fecha",
                    item.id
                );
            }
            let off = s2_dn_offset(baseline, item.properties.datetime.as_deref());
            if off != 0.0 {
                eprintln!(
                    "  {}: baseline {} → offset BOA −0.1 aplicado",
                    item.id,
                    baseline.unwrap_or("?")
                );
            }
            off
        } else {
            0.0
        };

        // bbox lon/lat → CRS nativo del item (esquinas → envolvente)
        let corners = [
            (bbox_lonlat[0], bbox_lonlat[1]),
            (bbox_lonlat[2], bbox_lonlat[1]),
            (bbox_lonlat[2], bbox_lonlat[3]),
            (bbox_lonlat[0], bbox_lonlat[3]),
        ];
        let utm = transform_points(&corners, 4326, epsg)
            .with_context(|| format!("EPSG:{epsg} no soportado para reproyección"))?;
        let bbox = BBox {
            min_x: utm.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
            min_y: utm.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
            max_x: utm.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
            max_y: utm.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        };

        // reanudación: si el par ms/mask ya existe, no re-descargar
        let stem = format!("{}_{}", &date[..10.min(date.len())], item.id);
        let ms_path = out_dir.join(format!("{stem}_ms.tif"));
        let mask_path = out_dir.join(format!("{stem}_mask.tif"));
        if ms_path.exists() && mask_path.exists() {
            eprintln!("  {}: ya existe, omitida", item.id);
            skipped_existing += 1;
            continue;
        }

        // leer las 5 bandas (solo la ventana, via range requests);
        // un error de red en una escena la omite, no aborta el batch
        let mut bands: Vec<Raster<f32>> = Vec::with_capacity(5);
        let mut failed = false;
        for key in band_keys {
            let Some(asset) = item.asset(key) else {
                eprintln!("  {}: sin asset '{key}', escena omitida", item.id);
                failed = true;
                break;
            };
            let href = client
                .sign_asset_href(&asset.href, collection)
                .context("firmando asset")?;
            let r: anyhow::Result<Raster<f32>> =
                CogReaderBlocking::open(&href, CogReaderOptions::default())
                    .with_context(|| format!("abriendo COG {key}"))
                    .and_then(|mut reader| {
                        reader
                            .read_bbox(&bbox, None)
                            .with_context(|| format!("leyendo ventana de {key}"))
                    });
            let mut r = match r {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("  {}: {e:#}, escena omitida", item.id);
                    failed = true;
                    break;
                }
            };
            for v in r.data_mut().iter_mut() {
                *v = to_reflectance(collection, *v, dn_offset);
            }
            r.set_nodata(None);
            bands.push(r);
        }
        if failed {
            failed_scenes += 1;
            continue;
        }

        // S2: B11 (20 m) a la grilla de 10 m de B03
        if collection == "sentinel-2-l2a" {
            let b11 = resample_to_grid(&bands[4], &bands[1], true);
            bands[4] = b11;
        }
        // recortar al shape mínimo común (grillas idénticas → no-op)
        let min_rows = bands.iter().map(|b| b.shape().0).min().unwrap();
        let min_cols = bands.iter().map(|b| b.shape().1).min().unwrap();
        if bands.iter().any(|b| b.shape() != (min_rows, min_cols)) {
            eprintln!(
                "  {}: bandas con shapes distintos, se recorta a {min_rows}×{min_cols}",
                item.id
            );
            for b in &mut bands {
                if b.shape() != (min_rows, min_cols) {
                    let view = b
                        .data()
                        .slice(ndarray::s![..min_rows, ..min_cols])
                        .to_owned();
                    let mut nb: Raster<f32> = b.with_same_meta(min_rows, min_cols);
                    *nb.data_mut() = view;
                    *b = nb;
                }
            }
        }

        // máscara (QA_PIXEL u16 / SCL u16→nearest a 10 m en S2)
        let mask_result: anyhow::Result<Raster<u16>> = (|| {
            let asset = item
                .asset(mask_key)
                .with_context(|| format!("sin asset de máscara '{mask_key}'"))?;
            // un fallo de firma es sistémico (token/credenciales), no algo
            // propio de esta escena: se propaga para abortar el batch igual
            // que en el loop de bandas, en vez de omitir escena por escena
            let href = client.sign_asset_href(&asset.href, collection)?;
            let mut reader = CogReaderBlocking::open(&href, CogReaderOptions::default())?;
            if collection == "sentinel-2-l2a" {
                let scl_f: Raster<f32> = reader.read_bbox(&bbox, None)?;
                let scl10 = resample_to_grid(&scl_f, &bands[1], false);
                let (rows, cols) = scl10.shape();
                let mut out: Raster<u16> = scl10.with_same_meta(rows, cols);
                let src = scl10.data();
                let dst = out.data_mut();
                for ((r, c), v) in src.indexed_iter() {
                    dst[[r, c]] = if v.is_finite() { *v as u16 } else { 0 };
                }
                Ok(out)
            } else {
                Ok(reader.read_bbox(&bbox, None)?)
            }
        })();
        let mut mask_raster = match mask_result {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  {}: {e:#}, escena omitida", item.id);
                failed_scenes += 1;
                continue;
            }
        };
        // la máscara debe compartir la grilla de las bandas: si estas se
        // recortaron al shape mínimo común, recortarla también — si no, el
        // par ms/mask sale con dimensiones distintas y revienta después en
        // `extract`, sobre un archivo que `fetch` reportó como exitoso
        if mask_raster.shape() != (min_rows, min_cols) {
            let (mr, mc) = mask_raster.shape();
            if mr < min_rows || mc < min_cols {
                eprintln!(
                    "  {}: máscara {mr}×{mc} menor que las bandas {min_rows}×{min_cols}, escena omitida",
                    item.id
                );
                failed_scenes += 1;
                continue;
            }
            let view = mask_raster
                .data()
                .slice(ndarray::s![..min_rows, ..min_cols])
                .to_owned();
            let mut nm: Raster<u16> = mask_raster.with_same_meta(min_rows, min_cols);
            *nm.data_mut() = view;
            mask_raster = nm;
        }

        // Escribir el par ms/mask de forma atómica: se escribe a `.tmp` y se
        // renombra al final (rename es atómico en el mismo filesystem). Sin
        // esto, un corte a mitad de la máscara dejaba ambos archivos en disco
        // y la reanudación —que solo comprueba existencia— fijaba para
        // siempre un par corrupto.
        let ms_tmp = ms_path.with_extension("tif.tmp");
        let mask_tmp = mask_path.with_extension("tif.tmp");
        let refs: Vec<&Raster<f32>> = bands.iter().collect();
        write_geotiff_stack(
            &refs,
            Some(&["blue", "green", "red", "nir", "swir1"]),
            &ms_tmp,
            &GeoTiffOptions::default(),
        )
        .with_context(|| format!("escribiendo {}", ms_tmp.display()))?;
        write_geotiff(&mask_raster, &mask_tmp, None)
            .with_context(|| format!("escribiendo {}", mask_tmp.display()))?;
        std::fs::rename(&mask_tmp, &mask_path)
            .with_context(|| format!("renombrando {}", mask_path.display()))?;
        std::fs::rename(&ms_tmp, &ms_path)
            .with_context(|| format!("renombrando {}", ms_path.display()))?;
        let (rows, cols) = bands[0].shape();
        eprintln!(
            "  {} -> {stem}_ms.tif ({rows}×{cols} px, EPSG:{epsg}, nubes {:.1}%)",
            item.id,
            item.properties.eo_cloud_cover.unwrap_or(f64::NAN)
        );
        written += 1;
    }

    if failed_scenes > 0 {
        eprintln!(
            "aviso: {failed_scenes} escena(s) omitidas por error de lectura/máscara \
             (reintentables: la reanudación las volverá a pedir)"
        );
    }
    // Un batch donde TODO falló (token expirado, red caída, endpoint movido)
    // terminaba con "0 escenas" y código de salida 0: un fetch nocturno
    // "exitoso" que no trajo nada. Se distingue de un batch ya completo, en
    // el que todas las escenas existían en disco.
    if written == 0 && skipped_existing == 0 && failed_scenes > 0 {
        anyhow::bail!(
            "ninguna de las {} escenas pudo descargarse ({failed_scenes} fallidas)",
            failed_scenes
        );
    }
    eprintln!("{written} escenas nuevas, {skipped_existing} ya existían, {failed_scenes} fallidas");
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_allowed_por_grupo() {
        assert!(sensor_allowed(Some("landsat-4"), "tm"));
        assert!(sensor_allowed(Some("landsat-5"), "tm"));
        assert!(!sensor_allowed(Some("landsat-7"), "tm"));
        assert!(sensor_allowed(Some("landsat-7"), "etm"));
        assert!(!sensor_allowed(Some("landsat-8"), "etm"));
        assert!(sensor_allowed(Some("landsat-8"), "oli"));
        assert!(sensor_allowed(Some("landsat-9"), "oli"));
        assert!(!sensor_allowed(Some("landsat-5"), "oli"));
        for p in [
            "landsat-4",
            "landsat-5",
            "landsat-7",
            "landsat-8",
            "landsat-9",
        ] {
            assert!(sensor_allowed(Some(p), "all"));
        }
        assert!(
            !sensor_allowed(None, "all"),
            "sin platform nunca pasa, ni con 'all'"
        );
    }

    #[test]
    fn l7_slc_failure_date_es_el_limite_correcto() {
        // referencia: fallo del SLC el 2003-05-31 (CoastSat skip_L7_SLC)
        assert_eq!(L7_SLC_FAILURE_DATE, "2003-05-31");
    }

    #[test]
    fn s2_offset_por_baseline() {
        // el baseline manda, no la fecha (escenas viejas reprocesadas)
        assert_eq!(
            s2_dn_offset(Some("04.00"), Some("2019-06-01T14:00:00Z")),
            -1000.0
        );
        assert_eq!(s2_dn_offset(Some("05.11"), None), -1000.0);
        assert_eq!(
            s2_dn_offset(Some("03.01"), Some("2023-06-01T14:00:00Z")),
            0.0
        );
        assert_eq!(s2_dn_offset(Some("02.14"), None), 0.0);
    }

    #[test]
    fn s2_offset_fallback_por_fecha() {
        // sin baseline (o ilegible) decide la fecha de adquisición
        assert_eq!(s2_dn_offset(None, Some("2022-01-24T23:59:59Z")), 0.0);
        assert_eq!(s2_dn_offset(None, Some("2022-01-25T00:00:01Z")), -1000.0);
        assert_eq!(
            s2_dn_offset(Some("N/A"), Some("2023-01-01T00:00:00Z")),
            -1000.0
        );
        assert_eq!(s2_dn_offset(None, None), 0.0);
    }

    #[test]
    fn reflectancia_s2_con_y_sin_offset() {
        // pre-04.00: DN 5000 → 0.5; post: DN 6000 − 1000 → 0.5
        assert!((to_reflectance("sentinel-2-l2a", 5000.0, 0.0) - 0.5).abs() < 1e-6);
        assert!((to_reflectance("sentinel-2-l2a", 6000.0, -1000.0) - 0.5).abs() < 1e-6);
        // DN 0 sigue siendo nodata aunque haya offset
        assert!(to_reflectance("sentinel-2-l2a", 0.0, -1000.0).is_nan());
        // landsat ignora el offset
        let l = to_reflectance("landsat-c2-l2", 40000.0, -1000.0);
        assert!((l - (40000.0 * 0.000_027_5 - 0.2)).abs() < 1e-6);
    }
}
