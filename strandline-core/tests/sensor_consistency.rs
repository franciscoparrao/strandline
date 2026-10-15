//! Consistencia entre sensores: la costa extraída de Landsat 7 (ETM+) y de
//! Landsat 8 (OLI) en fechas a un día de distancia debe caer en el mismo
//! lugar.
//!
//! Origen: en el estudio del gradiente cosísmico 2010 (60 sitios, escenas SR
//! `landsat-c2-l2` de Planetary Computer) la costa de L8 quedaba en mediana
//! +25.5 m mar adentro de la de L7 entre 2013 y 2021. La causa es el modo
//! `coastsat_compat` sobre reflectancia superficial: contornea el MNDWI con
//! un umbral sacado del Otsu del NDWI con NIR (el quirk de CoastSat), y sobre
//! SR la relación entre ambos índices cambia con el sensor. En el sitio de
//! este fixture el nivel MNDWI efectivo del modo compat es −0.02 en L7 y
//! +0.38 en L8 (medianas sobre 230 escenas). El cirrus del QA_PIXEL (solo en
//! OLI) y la máscara de nubes no influyen (verificado: desfase 26.1 m sin
//! cirrus y 26.5 m sin máscara).
//!
//! Fixture: 4 pares L7/L8 a 1 día (2015, 2017, 2019, 2021), recortes de
//! 69×68 px a 30 m en Algarrobo (33.36 S), con 33 transectos de 500 m en
//! EPSG:32719, referencia (OSM) en el chainage 250. Los pares se eligieron
//! por cobertura y no por desfase: son los 4 con más transectos válidos en
//! los tres modos (compat, Otsu, nivel 0) entre los 27 pares sin nubes de
//! 2013–2021. Sobre esos 27 pares la mediana agrupada del desfase L8−L7 es
//! +36.2 m (compat), +0.7 m (Otsu) y +4.6 m (nivel 0).
//!
//! Métrica: la del estudio original. Por transecto, chainage de cada escena
//! dentro de ±150 m de la referencia, diferencia L8 − L7, mediana sobre todos
//! los transectos de los 4 pares.
//!
//! El fixture NO se versiona (está en `.gitignore`, junto al resto del
//! material de Algarrobo). Sin él, los tests se saltan con un aviso por
//! stderr en vez de fallar.

use std::path::PathBuf;

use strandline_core::{
    compute_intersections, extract_shoreline, ExtractParams, IntersectionSettings, Transect,
};

const PAIRS: [(&str, &str); 4] = [
    (
        "2015-05-03_LE07_L2SP_001083_20150503_02_T1",
        "2015-05-04_LC08_L2SP_233083_20150504_02_T1",
    ),
    (
        "2017-06-09_LE07_L2SP_001083_20170609_02_T1",
        "2017-06-10_LC08_L2SP_233083_20170610_02_T1",
    ),
    (
        "2019-08-02_LE07_L2SP_001083_20190802_02_T1",
        "2019-08-03_LC08_L2SP_233083_20190803_02_T1",
    ),
    (
        "2021-09-08_LE07_L2SP_001083_20210908_02_T1",
        "2021-09-09_LC08_L2SP_233083_20210909_02_T1",
    ),
];

/// Chainage de la referencia oceánica sobre cada transecto (punto medio).
const REFERENCE_CHAINAGE_M: f64 = 250.0;
/// Semiancho de la ventana en torno a la referencia, como en el estudio.
const WINDOW_M: f64 = 150.0;
/// Las escenas L7 (path 001) y L8 (path 233) vienen en zonas UTM distintas;
/// se comparan en la del sitio.
const SITE_EPSG: u32 = 32719;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sensor_pairs")
        .join(name)
}

/// `false` (con aviso) si el fixture no está en este checkout.
fn fixture_available() -> bool {
    let ok = fixture("transects_epsg32719.csv").exists();
    if !ok {
        eprintln!("fixture sensor_pairs ausente (no versionado): test omitido");
    }
    ok
}

fn transects() -> Vec<Transect> {
    let text = std::fs::read_to_string(fixture("transects_epsg32719.csv")).unwrap();
    text.lines()
        .skip(1)
        .map(|l| {
            let v: Vec<f64> = l.split(',').skip(1).map(|x| x.parse().unwrap()).collect();
            Transect {
                origin: (v[0], v[1]),
                end: (v[2], v[3]),
            }
        })
        .collect()
}

/// Chainage por transecto de una escena, `NaN` si no cruza dentro de la
/// ventana.
fn chainages(scene: &str, params: &ExtractParams, ts: &[Transect]) -> Vec<f64> {
    let mut sl = extract_shoreline(
        &fixture(&format!("{scene}_ms.tif")),
        Some(&fixture(&format!("{scene}_mask.tif"))),
        params,
    )
    .unwrap_or_else(|e| panic!("extracción falló en {scene}: {e}"));
    sl.reproject(SITE_EPSG).unwrap();
    let line: Vec<(f64, f64)> = sl.lines.into_iter().flatten().collect();
    // una sola "shoreline" con todos los trozos, como hace `strandline
    // intersect` con los vértices de un mismo shoreline_id
    let cross = compute_intersections(&[line], ts, &IntersectionSettings::default());
    cross
        .iter()
        .map(|per| per[0])
        .map(|c| {
            if (c - REFERENCE_CHAINAGE_M).abs() <= WINDOW_M {
                c
            } else {
                f64::NAN
            }
        })
        .collect()
}

/// Diferencias L8 − L7 por transecto, agrupadas sobre los 4 pares.
fn l8_minus_l7(params: &ExtractParams) -> Vec<f64> {
    let ts = transects();
    let mut diffs = Vec::new();
    for (l7, l8) in PAIRS {
        let a = chainages(l7, params, &ts);
        let b = chainages(l8, params, &ts);
        diffs.extend(
            a.iter()
                .zip(&b)
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .map(|(x, y)| y - x),
        );
    }
    diffs
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// El modo recomendado para SR (Otsu refinado, sin clasificador) no debe
/// desplazar L8 respecto de L7 más de 5 m en mediana.
#[test]
fn l7_l8_offset_below_5m_in_sr_mode() {
    if !fixture_available() {
        return;
    }
    let diffs = l8_minus_l7(&ExtractParams::default());
    assert!(
        diffs.len() >= 30,
        "muy pocos transectos pareados: {}",
        diffs.len()
    );
    let n = diffs.len();
    let m = median(diffs);
    eprintln!("Otsu (SR): L8 − L7 = {m:.1} m sobre {n} transectos pareados");
    assert!(m.abs() < 5.0, "desfase L8 − L7 = {m:.1} m (límite 5 m)");
}

/// Control de sensibilidad del fixture: el modo compat sobre SR sí reproduce
/// el desfase. Si este test deja de fallar en el sentido esperado (desfase
/// < 15 m), el fixture ya no discrimina y el test anterior no prueba nada;
/// o bien el modo compat cambió y hay que revisar el aviso del CLI y la
/// documentación de `coastsat_compat`.
#[test]
fn coastsat_compat_on_sr_reproduces_sensor_offset() {
    if !fixture_available() {
        return;
    }
    let params = ExtractParams {
        use_classifier: true,
        coastsat_compat: true,
        ..Default::default()
    };
    let diffs = l8_minus_l7(&params);
    assert!(
        diffs.len() >= 20,
        "muy pocos transectos pareados: {}",
        diffs.len()
    );
    let n = diffs.len();
    let m = median(diffs);
    eprintln!("compat (SR): L8 − L7 = {m:.1} m sobre {n} transectos pareados");
    assert!(
        m > 15.0,
        "desfase L8 − L7 en compat = {m:.1} m; se esperaba > 15 m"
    );
}
