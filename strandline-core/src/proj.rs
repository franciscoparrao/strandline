//! Transformación de coordenadas UTM ↔ WGS84 nativa (sin PROJ).
//!
//! Implementa la proyección transversal de Mercator con las series de
//! Krüger/Karney hasta orden n⁴ (error << 1 mm para todo el dominio UTM),
//! suficiente para reproyectar shorelines entre zonas UTM y a WGS84 sin
//! arrastrar una dependencia de PROJ. Cubre los EPSG 326xx/327xx y 4326.
//!
//! Nota GEE: las escenas exportadas por Earth Engine en el hemisferio sur
//! pueden venir en zona *norte* (326xx) con northing negativo — la inversa
//! TM lo maneja naturalmente (latitud negativa), y es exactamente el caso
//! de las escenas del spike (32618/32619 con y < 0).

use crate::error::{Result, StrandlineError};

// WGS84
const A_SEMI: f64 = 6_378_137.0;
const F_FLAT: f64 = 1.0 / 298.257_223_563;
const K0: f64 = 0.9996;
const FALSE_EASTING: f64 = 500_000.0;
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

/// Parámetros UTM de un código EPSG: `(zona, hemisferio_norte)`.
/// `None` si el EPSG no es UTM/WGS84 soportado.
pub fn utm_params(epsg: u32) -> Option<(u8, bool)> {
    match epsg {
        32601..=32660 => Some(((epsg - 32600) as u8, true)),
        32701..=32760 => Some(((epsg - 32700) as u8, false)),
        _ => None,
    }
}

/// Meridiano central de una zona UTM (grados).
fn central_meridian_deg(zone: u8) -> f64 {
    (zone as f64) * 6.0 - 183.0
}

struct Series {
    big_a: f64,
    alpha: [f64; 4],
    beta: [f64; 4],
    e: f64,
    e2: f64,
}

fn series() -> Series {
    let n = F_FLAT / (2.0 - F_FLAT);
    let n2 = n * n;
    let n3 = n2 * n;
    let n4 = n3 * n;
    Series {
        big_a: A_SEMI / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0),
        alpha: [
            n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0,
            13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0,
            61.0 * n3 / 240.0 - 103.0 * n4 / 140.0,
            49561.0 * n4 / 161280.0,
        ],
        beta: [
            n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0,
            n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0,
            17.0 * n3 / 480.0 - 37.0 * n4 / 840.0,
            4397.0 * n4 / 161280.0,
        ],
        e: (F_FLAT * (2.0 - F_FLAT)).sqrt(),
        e2: F_FLAT * (2.0 - F_FLAT),
    }
}

/// WGS84 → UTM. `lon`/`lat` en grados; devuelve `(easting, northing)` en la
/// zona indicada. Con `north = true` y latitud sur el northing sale
/// negativo (convención GEE); con `north = false` se aplica el falso norte
/// de 10.000.000 m.
pub fn wgs84_to_utm(lon: f64, lat: f64, zone: u8, north: bool) -> (f64, f64) {
    let s = series();
    let lat_r = lat.to_radians();
    let dlon = (lon - central_meridian_deg(zone)).to_radians();

    // latitud conforme
    let sin_lat = lat_r.sin();
    let sigma = (s.e * (s.e * sin_lat).atanh()).sinh();
    let tan_lat = lat_r.tan();
    let t = tan_lat * (1.0 + sigma * sigma).sqrt() - sigma * (1.0 + tan_lat * tan_lat).sqrt();

    let xi_p = t.atan2(dlon.cos());
    let eta_p = (dlon.sin() / (t * t + dlon.cos() * dlon.cos()).sqrt()).asinh();

    let mut xi = xi_p;
    let mut eta = eta_p;
    for (j, &a) in s.alpha.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi += a * (k * xi_p).sin() * (k * eta_p).cosh();
        eta += a * (k * xi_p).cos() * (k * eta_p).sinh();
    }

    let easting = FALSE_EASTING + K0 * s.big_a * eta;
    let mut northing = K0 * s.big_a * xi;
    if !north {
        northing += FALSE_NORTHING_SOUTH;
    }
    (easting, northing)
}

/// UTM → WGS84. Devuelve `(lon, lat)` en grados.
pub fn utm_to_wgs84(easting: f64, northing: f64, zone: u8, north: bool) -> (f64, f64) {
    let s = series();
    let y = if north {
        northing
    } else {
        northing - FALSE_NORTHING_SOUTH
    };
    let xi = y / (K0 * s.big_a);
    let eta = (easting - FALSE_EASTING) / (K0 * s.big_a);

    let mut xi_p = xi;
    let mut eta_p = eta;
    for (j, &b) in s.beta.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi_p -= b * (k * xi).sin() * (k * eta).cosh();
        eta_p -= b * (k * xi).cos() * (k * eta).sinh();
    }

    let sinh_eta = eta_p.sinh();
    let cos_xi = xi_p.cos();
    let tau_p = xi_p.sin() / (sinh_eta * sinh_eta + cos_xi * cos_xi).sqrt();
    let lon = central_meridian_deg(zone) + sinh_eta.atan2(cos_xi).to_degrees();

    // latitud geodésica desde la conforme: Newton sobre τ (Karney 2011)
    let mut tau = tau_p;
    for _ in 0..5 {
        let sigma = (s.e * (s.e * tau / (1.0 + tau * tau).sqrt()).atanh()).sinh();
        let tau_p_i = tau * (1.0 + sigma * sigma).sqrt() - sigma * (1.0 + tau * tau).sqrt();
        let dtau = (tau_p - tau_p_i) * (1.0 + (1.0 - s.e2) * tau * tau)
            / ((1.0 - s.e2) * (1.0 + tau_p_i * tau_p_i).sqrt() * (1.0 + tau * tau).sqrt());
        tau += dtau;
        if dtau.abs() < 1e-15 {
            break;
        }
    }
    (lon, tau.atan().to_degrees())
}

/// Transforma puntos `(x, y)` entre dos EPSG soportados (UTM WGS84 326xx/
/// 327xx y 4326). Identidad si `from == to`. UTM→UTM pasa por WGS84.
pub fn transform_points(
    points: &[(f64, f64)],
    from_epsg: u32,
    to_epsg: u32,
) -> Result<Vec<(f64, f64)>> {
    if from_epsg == to_epsg {
        return Ok(points.to_vec());
    }
    let unsupported = |e: u32| {
        StrandlineError::UnsupportedCrs(format!(
            "EPSG:{e} — soportados: 4326 y UTM WGS84 (326xx/327xx)"
        ))
    };

    // a WGS84 (lon, lat)
    let wgs: Vec<(f64, f64)> = if from_epsg == 4326 {
        points.to_vec()
    } else {
        let (zone, north) = utm_params(from_epsg).ok_or_else(|| unsupported(from_epsg))?;
        points
            .iter()
            .map(|&(e, n)| utm_to_wgs84(e, n, zone, north))
            .collect()
    };

    // de WGS84 al destino
    if to_epsg == 4326 {
        return Ok(wgs);
    }
    let (zone, north) = utm_params(to_epsg).ok_or_else(|| unsupported(to_epsg))?;
    Ok(wgs
        .iter()
        .map(|&(lon, lat)| wgs84_to_utm(lon, lat, zone, north))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_utm_wgs84() {
        // punto del spike: 32619 con northing negativo (convención GEE)
        let (e0, n0) = (261_500.0, -3_652_000.0);
        let (lon, lat) = utm_to_wgs84(e0, n0, 19, true);
        assert!(lat < -30.0 && lat > -35.0, "lat = {lat}");
        assert!(lon < -70.0 && lon > -73.0, "lon = {lon}");
        let (e1, n1) = wgs84_to_utm(lon, lat, 19, true);
        assert!((e0 - e1).abs() < 1e-6, "Δe = {}", (e0 - e1).abs());
        assert!((n0 - n1).abs() < 1e-6, "Δn = {}", (n0 - n1).abs());
    }

    #[test]
    fn utm_params_mapping() {
        assert_eq!(utm_params(32619), Some((19, true)));
        assert_eq!(utm_params(32719), Some((19, false)));
        assert_eq!(utm_params(4326), None);
        assert_eq!(utm_params(3857), None);
    }

    #[test]
    fn unsupported_epsg_is_error() {
        let r = transform_points(&[(0.0, 0.0)], 3857, 4326);
        assert!(matches!(r, Err(StrandlineError::UnsupportedCrs(_))));
    }

    #[test]
    fn golden_against_pyproj() {
        // Referencias generadas con pyproj (PROJ) sobre puntos reales del
        // dominio del paper. Tolerancia: 1 mm en metros, ~1e-8° en grados
        // (≈ 1 mm). (from, to, x, y, x_ref, y_ref)
        let cases = [
            // escenas GEE en zona norte con northing negativo
            (32619u32, 4326u32, 261500.0, -3652000.0, -71.552187865, -32.980338828),
            (32618, 4326, 821047.5, -3652627.5, -71.565450932, -32.964828771),
            // el flujo real del paper: nativo GEE → 19S
            (32619, 32719, 261500.0, -3652000.0, 261500.0, 6348000.0),
            (32719, 4326, 262000.0, 6347000.0, -71.547100891, -32.989459499),
            (4326, 32719, -71.53, -33.0, 263626.455278445, 6345869.596473648),
        ];
        for (from, to, x, y, xr, yr) in cases {
            let out = transform_points(&[(x, y)], from, to).unwrap();
            let (ox, oy) = out[0];
            let tol = if to == 4326 { 1.2e-8 } else { 1e-3 };
            assert!(
                (ox - xr).abs() < tol && (oy - yr).abs() < tol,
                "{from}->{to} ({x},{y}): got ({ox:.9}, {oy:.9}), want ({xr}, {yr})"
            );
        }
    }
}
