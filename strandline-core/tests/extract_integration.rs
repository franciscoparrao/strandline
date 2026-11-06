//! Test de integración del pipeline de extracción sobre un fixture real:
//! recorte de 96×96 px de la escena L8 2020-01-10 de Viña del Mar
//! (playa de Reñaca, ~50% agua), con su máscara QA_PIXEL.
//!
//! Es la única protección en CI del camino GeoTIFF → shoreline (las escenas
//! completas del spike viven fuera del repo). Los golden values se midieron
//! con la versión validada contra CoastSat (auditoría 2026-07).

use std::path::PathBuf;

use strandline_core::{extract_shoreline, ExtractParams};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn extract_without_classifier_matches_golden() {
    let shoreline = extract_shoreline(
        &fixture("vina_2020-01-10_96px_ms.tif"),
        Some(&fixture("vina_2020-01-10_96px_mask.tif")),
        &ExtractParams::default(),
    )
    .expect("extracción falló sobre el fixture");

    assert_eq!(shoreline.epsg, Some(32619));
    // 1 línea continua: la máscara de nubes limpia (blobs falsos de swash
    // < 40 px removidos) des-fragmentó la costa que antes salía en 2 trozos
    assert_eq!(shoreline.lines.len(), 1, "n líneas cambió");
    // umbral Otsu global + refinamiento por corredor (determinista)
    assert!(
        (shoreline.threshold - 0.2632).abs() < 5e-4,
        "umbral = {}",
        shoreline.threshold
    );
    // todas las coordenadas dentro del bounding box del recorte (UTM 19N,
    // y negativo por hemisferio sur en zona norte) con holgura de 100 m
    for line in &shoreline.lines {
        for &(x, y) in line {
            assert!((260000.0..280000.0).contains(&x), "x fuera de rango: {x}");
            assert!(
                (-3660000.0..-3640000.0).contains(&y),
                "y fuera de rango: {y}"
            );
        }
    }
}

#[test]
fn extract_coastsat_compat_matches_golden() {
    let params = ExtractParams {
        coastsat_compat: true,
        use_classifier: true,
        ..Default::default()
    };
    let shoreline = extract_shoreline(
        &fixture("vina_2020-01-10_96px_ms.tif"),
        Some(&fixture("vina_2020-01-10_96px_mask.tif")),
        &params,
    )
    .expect("extracción compat falló sobre el fixture");

    assert_eq!(shoreline.lines.len(), 1);
    // umbral del quirk (Otsu balanceado del NDWI, determinista sin RNG)
    assert!(
        (shoreline.threshold - (-0.1613)).abs() < 5e-4,
        "umbral compat = {}",
        shoreline.threshold
    );
    // Clasificación golden (conteos exactos: el MLP y el filtro de área son
    // deterministas).
    //
    // Los conteos de arena cambiaron de 354 a 352 al corregir la conversión
    // de `min_beach_area` a píxeles: `ceil`, como CoastSat
    // (`np.ceil(min_beach_area/pixel_size**2)`), en vez de `round`. Este
    // fixture es pansharpened de 15 m y el default propio del motor son
    // 500 m², así que 500/225 = 2.22 → round 2 vs ceil 3, y el filtro más
    // estricto elimina dos blobs de 2 px. Con los settings del estudio de
    // referencia (min_beach_area = 4500) el cociente es entero exacto
    // (20.0 a 15 m, 5.0 a 30 m) y ambas funciones coinciden: ninguna de las
    // validaciones contra CoastSat/pickles se ve afectada por el cambio.
    let cls = shoreline
        .classification
        .expect("sin resumen de clasificación");
    assert_eq!(cls.sand_px, 352);
    assert_eq!(cls.whitewater_px, 39);
    assert_eq!(cls.water_px, 4610);
}

/// Filtro de proximidad a nodata (paso 3 de `process_shoreline` de CoastSat,
/// 30 m): se punza en el fixture una franja de gap como las que dejan las
/// escenas ETM+ posteriores a la falla del corrector de línea de barrido
/// —Green, NIR y SWIR1 a cero a la vez, que es como CoastSat define nodata—
/// y se comprueba que los vértices pegados al borde de la franja se eliminan.
#[test]
fn nodata_filter_removes_vertices_along_a_scan_gap() {
    use surtgis_core::io::GeoTiffOptions;
    use surtgis_core::io::{read_geotiff_bands, write_geotiff_stack};

    let mut bands = read_geotiff_bands::<f32, _>(&fixture("vina_2020-01-10_96px_ms.tif"))
        .expect("leyendo fixture");
    let (rows, cols) = bands[0].shape();

    // franja vertical de nodata que cruza la escena (y por tanto la costa)
    let gap_col = cols / 2;
    for k in [1usize, 3, 4] {
        // Green, NIR, SWIR1 (0-based), las tres que mira `nodata_mask`
        let d = bands[k].data_mut();
        for r in 0..rows {
            d[[r, gap_col]] = 0.0;
        }
    }
    let tmp = std::env::temp_dir().join("strandline_nodata_gap_ms.tif");
    let refs: Vec<&_> = bands.iter().collect();
    write_geotiff_stack(&refs, None, &tmp, &GeoTiffOptions::default()).expect("escribiendo escena");

    let with_filter = extract_shoreline(
        &tmp,
        Some(&fixture("vina_2020-01-10_96px_mask.tif")),
        &ExtractParams::default(),
    )
    .expect("extracción con filtro");
    let without = extract_shoreline(
        &tmp,
        Some(&fixture("vina_2020-01-10_96px_mask.tif")),
        &ExtractParams {
            dist_nodata_m: 0.0,
            ..Default::default()
        },
    )
    .expect("extracción sin filtro");

    let n_pts = |s: &strandline_core::shoreline::Shoreline| -> usize {
        s.lines.iter().map(|l| l.len()).sum()
    };
    assert!(
        n_pts(&with_filter) < n_pts(&without),
        "el filtro debe eliminar vértices junto al gap: {} vs {}",
        n_pts(&with_filter),
        n_pts(&without)
    );

    // Ningún vértice sobreviviente debe quedar cerca del gap. La tolerancia
    // es de un píxel: la transformada de distancia se consulta en el píxel
    // MÁS CERCANO al vértice, no en su posición fraccional, así que el corte
    // efectivo puede quedar hasta media diagonal de píxel (~0.71 px) por
    // debajo de los 30 m nominales. Es la misma aproximación que ya usa el
    // buffer de nubes —donde a 300 m es inmaterial— y la diferencia que
    // queda frente a la distancia exacta de CoastSat.
    let gt = bands[0].transform().to_gdal();
    let gap_x = gt[0] + (gap_col as f64) * gt[1];
    let cell = gt[1].abs();
    for line in &with_filter.lines {
        for &(x, _) in line {
            assert!(
                (x - gap_x).abs() >= 30.0 - cell,
                "vértice a {:.1} m del gap sobrevivió al filtro",
                (x - gap_x).abs()
            );
        }
    }
    // y sin el filtro sí los hay (si no, el test no probaría nada)
    let close_without = without
        .lines
        .iter()
        .flatten()
        .filter(|&&(x, _)| (x - gap_x).abs() < 30.0)
        .count();
    assert!(
        close_without > 0,
        "sin filtro debería haber vértices junto al gap"
    );

    let _ = std::fs::remove_file(&tmp);
}

/// Con la propia línea extraída como referencia y un buffer de 300 m, la
/// extracción no cambia: el buffer contiene toda la costa y la muestra del
/// umbral sigue siendo esencialmente la misma escena. Con una referencia a
/// 5 km, fuera de la escena, no queda nada que contornear.
#[test]
fn extract_with_reference_buffer() {
    use strandline_core::reference::ReferenceLine;
    let ms = fixture("vina_2020-01-10_96px_ms.tif");
    let qa = fixture("vina_2020-01-10_96px_mask.tif");
    let base = extract_shoreline(&ms, Some(&qa), &ExtractParams::default()).unwrap();
    assert!(!base.lines.is_empty());

    let own = ReferenceLine {
        lines: base.lines.clone(),
        epsg: base.epsg,
    };
    let with_ref = extract_shoreline(
        &ms,
        Some(&qa),
        &ExtractParams {
            reference: Some(own),
            max_dist_ref_m: 300.0,
            ..Default::default()
        },
    )
    .unwrap();
    let n_pts = |s: &strandline_core::Shoreline| s.lines.iter().map(|l| l.len()).sum::<usize>();
    assert_eq!(with_ref.lines.len(), base.lines.len());
    // la muestra del umbral cambia (solo la franja), así que el umbral y los
    // vértices pueden moverse un poco; la línea debe quedar en el mismo lugar
    let max_shift = with_ref.lines[0]
        .iter()
        .map(|&p| {
            base.lines[0]
                .iter()
                .map(|&q| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt())
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max);
    assert!(max_shift < 15.0, "la línea se movió {max_shift:.1} m");
    assert!(n_pts(&with_ref) > n_pts(&base) / 2);

    let far = ReferenceLine {
        lines: base
            .lines
            .iter()
            .map(|l| l.iter().map(|&(x, y)| (x + 5000.0, y)).collect())
            .collect(),
        epsg: base.epsg,
    };
    let none = extract_shoreline(
        &ms,
        Some(&qa),
        &ExtractParams {
            reference: Some(far),
            ..Default::default()
        },
    );
    // sin píxeles en la franja, o no hay umbral o no hay líneas
    match none {
        Ok(s) => assert!(
            s.lines.is_empty(),
            "{} líneas fuera de la franja",
            s.lines.len()
        ),
        Err(e) => assert!(e.to_string().contains("valid"), "error inesperado: {e}"),
    }
}
