//! Índices espectrales normalizados de diferencia.

use ndarray::Array2;
use surtgis_core::raster::Raster;

/// Cota de magnitud para descartar un valor de índice normalizado de
/// diferencia como inválido. Un ND index "sano" cae en `[-1, 1]`, pero
/// reflectancias reales ligeramente negativas (sombras, agua oscura —
/// ocurre tanto en TOA de GEE como en Surface Reflectance) pueden empujar
/// el valor un poco más allá sin que el denominador esté genuinamente
/// cerca de cero; CoastSat original no filtra esto, y replicarlo mantiene
/// la fidelidad del port (ver el pixel real del fixture de integración,
/// SWIR1=-0.0028, que da -1.05/-1.22 en dos índices). Lo que sí hay que
/// cortar es la explosión numérica real cuando el denominador SÍ cruza
/// cerca de cero: en escenas SR reales (Landsat C2, negativos más
/// frecuentes) se observaron valores de **cientos a miles** — 25 de 60
/// escenas de 4 sitios (Coquimbo/Valdivia/Chiloé/Constitución, 2026-07-17)
/// con el umbral de Otsu degenerado por esto. `5.0` separa ambos casos con
/// margen amplio.
const MAX_ABS_VALUE: f32 = 5.0;

/// Índice normalizado de diferencia (a − b) / (a + b), réplica de
/// `SDS_tools.nd_index` de CoastSat.
///
/// Devuelve `NaN` en píxeles inválidos:
/// - alguna de las dos bandas no finita,
/// - ambas bandas exactamente 0 (relleno de borde de escena),
/// - denominador 0,
/// - píxel marcado en `cloud` (máscara de nubes, si se entrega — ver
///   [`crate::cloud::cloud_mask_from_qa`]),
/// - **`|resultado| > MAX_ABS_VALUE`**: ver la constante para el porqué
///   del umbral (no es simplemente `[-1,1]`, que CoastSat tampoco impone).
pub fn nd_index(a: &Raster<f32>, b: &Raster<f32>, cloud: Option<&Array2<bool>>) -> Raster<f32> {
    let (rows, cols) = a.shape();
    let mut out: Raster<f32> = a.with_same_meta(rows, cols);
    let av = a.data();
    let bv = b.data();
    let ov = out.data_mut();

    for r in 0..rows {
        for c in 0..cols {
            let x = av[[r, c]];
            let y = bv[[r, c]];
            let cloudy = cloud.map(|m| m[[r, c]]).unwrap_or(false);
            let denom = x + y;
            let value = (x - y) / denom;
            ov[[r, c]] = if !x.is_finite()
                || !y.is_finite()
                || (x == 0.0 && y == 0.0)
                || denom == 0.0
                || cloudy
                || !value.is_finite()
                || value.abs() > MAX_ABS_VALUE
            {
                f32::NAN
            } else {
                value
            };
        }
    }
    out.set_nodata(None);
    out
}

/// MNDWI = (Green − SWIR1) / (Green + SWIR1) (convención textbook).
///
/// Nota: CoastSat usa el signo opuesto internamente, (SWIR1 − Green) /
/// (SWIR1 + Green); ver `coastsat_mwi` para esa convención cuando se
/// necesita compatibilidad bit-a-bit (`ExtractParams::coastsat_compat`).
pub fn mndwi(
    green: &Raster<f32>,
    swir1: &Raster<f32>,
    cloud: Option<&Array2<bool>>,
) -> Raster<f32> {
    nd_index(green, swir1, cloud)
}

#[cfg(test)]
mod tests {
    use super::*;
    use surtgis_core::raster::GeoTransform;

    fn raster1x1(v: f32) -> Raster<f32> {
        let mut r = Raster::<f32>::from_vec(vec![v], 1, 1).unwrap();
        r.set_transform(GeoTransform::from_gdal([0.0, 30.0, 0.0, 0.0, 0.0, -30.0]));
        r.set_nodata(None);
        r
    }

    #[test]
    fn valor_normal_pasa() {
        // MNDWI típico de agua: green=0.08, swir1=0.02 -> (0.08-0.02)/0.10 = 0.6
        let a = raster1x1(0.08);
        let b = raster1x1(0.02);
        let out = nd_index(&a, &b, None);
        assert!((out.data()[[0, 0]] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn denominador_casi_cero_da_nan_no_valor_absurdo() {
        // caso real observado (Chiloé 2018-09-08): denominador cruza cero
        // por una reflectancia SWIR1 ligeramente negativa -> sin el
        // filtro, esto daba MNDWI ~ -1149 en vez de NaN.
        let green = raster1x1(0.02);
        let swir1 = raster1x1(-0.0198); // green+swir1 = 0.0002, casi cero
        let out = nd_index(&green, &swir1, None);
        assert!(
            out.data()[[0, 0]].is_nan(),
            "denominador cerca de cero debe dar NaN, no un valor absurdo"
        );
    }

    #[test]
    fn excursion_moderada_fuera_de_1_pasa_sin_filtrar() {
        // caso real del fixture de integración (vina_2020-01-10_96px,
        // pixel [73,42]): SWIR1=-0.0028 (sombra/agua oscura, reflectancia
        // negativa real, no ruido de redondeo) da SWIR-G=-1.05 y
        // SWIR-NIR=-1.22 — moderadamente fuera de [-1,1] pero muy lejos
        // del rango de la explosión numérica real (cientos/miles). Debe
        // pasar sin cambios: CoastSat original tampoco lo filtra, y
        // filtrarlo movería el golden de `extract_coastsat_compat_matches_golden`.
        let a = raster1x1(-0.002830582);
        let b = raster1x1(0.10803388);
        let out = nd_index(&a, &b, None);
        assert!(
            (out.data()[[0, 0]] - (-1.0538116)).abs() < 1e-4,
            "valor = {:?}",
            out.data()[[0, 0]]
        );
    }

    #[test]
    fn explosion_numerica_real_da_nan() {
        // el patrón real del bug (25/60 escenas SR de 4 sitios,
        // 2026-07-17): valores de cientos/miles cuando el denominador
        // cruza cerca de cero de verdad. abs() > MAX_ABS_VALUE (5.0) debe
        // rechazarse aunque el denominador no sea exactamente 0.0.
        let a = raster1x1(0.5);
        let b = raster1x1(-0.498); // denom=0.002, (0.5-(-0.498))/0.002 = 499
        let out = nd_index(&a, &b, None);
        assert!(out.data()[[0, 0]].is_nan());
    }
}
