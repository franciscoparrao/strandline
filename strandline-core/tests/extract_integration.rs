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
    assert_eq!(shoreline.lines.len(), 2, "n líneas cambió");
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
            assert!((-3660000.0..-3640000.0).contains(&y), "y fuera de rango: {y}");
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

    assert_eq!(shoreline.lines.len(), 2);
    // umbral del quirk (Otsu balanceado del NDWI, determinista sin RNG)
    assert!(
        (shoreline.threshold - (-0.1613)).abs() < 5e-4,
        "umbral compat = {}",
        shoreline.threshold
    );
    // clasificación golden (conteos exactos: el MLP y el filtro de área
    // son deterministas)
    let cls = shoreline.classification.expect("sin resumen de clasificación");
    assert_eq!(cls.sand_px, 350);
    assert_eq!(cls.whitewater_px, 39);
    assert_eq!(cls.water_px, 4606);
}
