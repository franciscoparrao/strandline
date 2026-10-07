//! Comparación de los dos flujos de producción para obtener el chainage de
//! la línea de agua en un transecto (S1, roadmap SOTA):
//!
//! - **contorno + intersección**: marching squares sobre el MNDWI →
//!   `compute_intersections` con franja de ±25 m (el flujo CoastSat/DSAS);
//! - **ajuste de perfil**: `fit_profile` directo sobre el transecto.
//!
//! Campo sintético con frontera en posición subpixel conocida, ruido por
//! píxel realista y transecto **diagonal** a la grilla (el caso general).
//! Se barre la frontera a través de un píxel completo para exponer sesgos
//! de anclaje a la grilla (*pixel-locking*). El experimento es la base de
//! la figura de accuracy del paper.

use strandline_core::contour::find_contours;
use strandline_core::subpixel::{fit_profile, ProfileSettings};
use strandline_core::transects::{compute_intersections, IntersectionSettings, Transect};
use surtgis_core::raster::{GeoTransform, Raster};

const CELL: f64 = 15.0;
const ROWS: usize = 60;
const COLS: usize = 60;
const LEVEL: f64 = 0.1;
const EDGE_W: f64 = 15.0;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 as f64 / u64::MAX as f64) - 0.5
    }
}

/// Campo MNDWI sintético: frontera tanh vertical en `x_edge` (m) + ruido.
fn scene(x_edge: f64, noise: f64, seed: u64) -> Raster<f32> {
    let mut rng = Rng(seed.wrapping_mul(2654435761).wrapping_add(1));
    let mut data = Vec::with_capacity(ROWS * COLS);
    for _r in 0..ROWS {
        for c in 0..COLS {
            let x = c as f64 * CELL;
            let v = LEVEL + 0.5 * ((x - x_edge) / EDGE_W).tanh() + noise * rng.next();
            data.push(v as f32);
        }
    }
    let mut r = Raster::<f32>::from_vec(data, ROWS, COLS).unwrap();
    r.set_transform(GeoTransform::from_gdal([0.0, CELL, 0.0, 0.0, 0.0, -CELL]));
    r.set_nodata(None);
    r
}

/// Flujo contorno→intersección con la maquinaria real de producción.
fn contour_intersection_estimate(raster: &Raster<f32>, transect: &Transect) -> Option<f64> {
    let contours = find_contours(raster.data().view(), LEVEL);
    let gt = raster.transform().to_gdal();
    let shorelines: Vec<Vec<(f64, f64)>> = contours
        .into_iter()
        .map(|c| {
            c.into_iter()
                .map(|(r, col)| (gt[0] + col * gt[1], gt[3] + r * gt[5]))
                .collect()
        })
        .collect();
    let settings = IntersectionSettings::default(); // franja ±25 m, producción
    let out = compute_intersections(&shorelines, &[*transect], &settings);
    let v = out[0][0];
    v.is_finite().then_some(v)
}

#[test]
fn profile_fit_vs_contour_intersection_realistic() {
    let noise = 0.08; // σ del MNDWI en zona de swash
                      // transecto diagonal (~27° respecto a la grilla), tierra oeste → mar este
    let transect = Transect {
        origin: (75.0, -300.0),
        end: (595.0, -560.0),
    };
    let (ox, oy) = transect.origin;
    let (dx, dy) = (transect.end.0 - ox, transect.end.1 - oy);
    let len = (dx * dx + dy * dy).sqrt();
    let ux = dx / len;
    let settings = ProfileSettings::default();

    let mut err_contour = Vec::new();
    let mut err_profile = Vec::new();
    for step in 0..10 {
        let x_true = 300.0 + (step as f64 / 10.0) * CELL;
        // chainage verdadero: donde el transecto cruza el plano x = x_true
        let s_true = (x_true - ox) / ux;
        for seed in 0..25u64 {
            let raster = scene(x_true, noise, 1 + seed * 977 + step as u64 * 13);
            if let Some(s) = contour_intersection_estimate(&raster, &transect) {
                err_contour.push(s - s_true);
            }
            if let Some(fit) = fit_profile(&raster, &transect, LEVEL, None, &settings) {
                err_profile.push(fit.chainage_m - s_true);
            }
        }
    }
    assert!(
        err_contour.len() >= 220,
        "contour: {} casos",
        err_contour.len()
    );
    assert!(
        err_profile.len() >= 220,
        "profile: {} casos",
        err_profile.len()
    );

    let rmse = |e: &[f64]| (e.iter().map(|v| v * v).sum::<f64>() / e.len() as f64).sqrt();
    let bias = |e: &[f64]| e.iter().sum::<f64>() / e.len() as f64;
    let (rc, rp) = (rmse(&err_contour), rmse(&err_profile));
    let (bc, bp) = (bias(&err_contour), bias(&err_profile));
    eprintln!(
        "contorno+intersección: RMSE = {rc:.2} m, sesgo = {bc:+.2} m | \
         perfil: RMSE = {rp:.2} m, sesgo = {bp:+.2} m (n = {})",
        err_profile.len()
    );

    // Resultado medido (σ=0.08, 250 casos): contorno+intersección
    // RMSE ≈ 1.6 m con sesgo ≈ −0.8 m; perfil RMSE ≈ 0.5 m con sesgo
    // ≈ 0.0 m. El perfil debe seguir siendo claramente superior (≥1.7×
    // en RMSE, margen anti-flaky sobre el 3× medido) e insesgado.
    assert!(rp < 0.6 * rc, "perfil {rp:.2} m vs contorno {rc:.2} m");
    assert!(rp < CELL / 4.0, "RMSE perfil = {rp:.2} m");
    assert!(bp.abs() < 0.3, "sesgo perfil = {bp:.2} m");
}
