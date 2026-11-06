//! Línea de referencia y su buffer: restringe la extracción a una franja
//! alrededor de una costa conocida (equivalente a `reference_shoreline` +
//! `max_dist_ref` de CoastSat).
//!
//! CoastSat (`create_shoreline_buffer`, `find_wl_contours2`) hace tres cosas:
//! marca los píxeles de la referencia, los dilata con un disco de radio
//! `ceil(max_dist_ref / tamaño de píxel)`, contornea el MNDWI solo dentro de
//! ese buffer y calcula el umbral con los píxeles del buffer dilatado 5
//! píxeles más. Aquí se replica con la transformada de distancia exacta del
//! buffer de nubes: un píxel está en un disco de radio `r` alrededor de un
//! píxel marcado si su distancia euclídea a él es `≤ r`, que es la definición
//! de `skimage.morphology.disk`.
//!
//! Una diferencia, a favor de la robustez: CoastSat marca solo los vértices
//! de la referencia, así que una referencia con vértices espaciados más que el
//! diámetro del buffer deja un buffer en cuentas. Aquí se marcan los
//! segmentos completos. Con una referencia densa (vértices a menos de un
//! píxel, lo habitual) los dos buffers coinciden.

use ndarray::Array2;
use surtgis_core::raster::GeoTransform;

use crate::cloud::distance_to_true_px;
use crate::error::Result;
use crate::proj::transform_points;

/// Píxeles extra con que CoastSat ensancha el buffer para tomar la muestra
/// del umbral (`morphology.disk(5)` en `find_wl_contours1/2`).
pub const THRESHOLD_EXTRA_PX: f64 = 5.0;

/// Una línea de referencia: una o más polilíneas en un CRS dado.
#[derive(Debug, Clone)]
pub struct ReferenceLine {
    /// Polilíneas (x, y).
    pub lines: Vec<Vec<(f64, f64)>>,
    /// EPSG de las coordenadas; `None` = el mismo CRS que la escena.
    pub epsg: Option<u32>,
}

/// Máscaras del buffer de referencia sobre la grilla de una escena.
#[derive(Debug, Clone)]
pub struct ReferenceBuffer {
    /// Píxeles a menos de `max_dist_ref` de la referencia: donde se contornea.
    pub contour: Array2<bool>,
    /// El buffer ensanchado [`THRESHOLD_EXTRA_PX`]: de donde sale la muestra
    /// del umbral.
    pub threshold: Array2<bool>,
    /// Píxeles de la grilla tocados por la referencia (0 = la referencia no
    /// cae dentro de la escena y el buffer queda vacío).
    pub n_reference_px: usize,
}

/// Construye el buffer de una referencia sobre la grilla `(rows, cols)` de
/// una escena con geotransform `gt`, tamaño de píxel `cell` y EPSG
/// `scene_epsg`. La referencia se reproyecta a la escena si declara otro EPSG.
pub fn reference_buffer(
    reference: &ReferenceLine,
    gt: &GeoTransform,
    shape: (usize, usize),
    cell: f64,
    scene_epsg: Option<u32>,
    max_dist_ref_m: f64,
) -> Result<ReferenceBuffer> {
    let (rows, cols) = shape;
    let g = gt.to_gdal();
    let mut marked = Array2::from_elem((rows, cols), false);
    let mut n_reference_px = 0usize;

    for line in &reference.lines {
        let pts = match (reference.epsg, scene_epsg) {
            (Some(from), Some(to)) if from != to => transform_points(line, from, to)?,
            _ => line.clone(),
        };
        // mundo → píxel con la convención de `pix2world` (geotransform GDAL
        // aplicado directo, sin offset de medio píxel; escenas sin rotación)
        let to_px = |(x, y): (f64, f64)| ((y - g[3]) / g[5], (x - g[0]) / g[1]);
        let mut mark = |r: f64, c: f64| {
            let (r, c) = (r.round(), c.round());
            if r >= 0.0 && c >= 0.0 && (r as usize) < rows && (c as usize) < cols {
                let cellref = &mut marked[[r as usize, c as usize]];
                if !*cellref {
                    *cellref = true;
                    n_reference_px += 1;
                }
            }
        };
        if pts.len() == 1 {
            let (r, c) = to_px(pts[0]);
            mark(r, c);
        }
        for w in pts.windows(2) {
            let (r0, c0) = to_px(w[0]);
            let (r1, c1) = to_px(w[1]);
            // pasos de medio píxel a lo largo del segmento
            let steps = ((r1 - r0).abs().max((c1 - c0).abs()) * 2.0).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                mark(r0 + t * (r1 - r0), c0 + t * (c1 - c0));
            }
        }
    }

    if n_reference_px == 0 {
        let empty = Array2::from_elem((rows, cols), false);
        return Ok(ReferenceBuffer {
            contour: empty.clone(),
            threshold: empty,
            n_reference_px,
        });
    }
    let dist = distance_to_true_px(&marked);
    let radius = (max_dist_ref_m / cell).ceil();
    Ok(ReferenceBuffer {
        contour: dist.mapv(|d| (d as f64) <= radius),
        threshold: dist.mapv(|d| (d as f64) <= radius + THRESHOLD_EXTRA_PX),
        n_reference_px,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gt_10m() -> GeoTransform {
        // origen (1000, 2000), píxeles de 10 m, norte arriba
        GeoTransform::new(1000.0, 2000.0, 10.0, -10.0)
    }

    #[test]
    fn buffer_alrededor_de_una_linea_vertical() {
        // línea vertical en x = 1250 (columna 25), de arriba abajo
        let r = ReferenceLine {
            lines: vec![vec![(1250.0, 2000.0), (1250.0, 1500.0)]],
            epsg: None,
        };
        let b = reference_buffer(&r, &gt_10m(), (50, 50), 10.0, None, 30.0).unwrap();
        assert_eq!(b.n_reference_px, 50, "un píxel por fila");
        // radio ceil(30/10) = 3 px: columnas 22..=28 dentro, 21 y 29 fuera
        assert!(b.contour[[20, 22]] && b.contour[[20, 28]]);
        assert!(!b.contour[[20, 21]] && !b.contour[[20, 29]]);
        // la muestra del umbral se ensancha 5 px: columnas 17..=33
        assert!(b.threshold[[20, 17]] && b.threshold[[20, 33]]);
        assert!(!b.threshold[[20, 16]] && !b.threshold[[20, 34]]);
    }

    #[test]
    fn referencia_fuera_de_la_escena_deja_el_buffer_vacio() {
        let r = ReferenceLine {
            lines: vec![vec![(9000.0, 9000.0), (9500.0, 9000.0)]],
            epsg: None,
        };
        let b = reference_buffer(&r, &gt_10m(), (50, 50), 10.0, None, 300.0).unwrap();
        assert_eq!(b.n_reference_px, 0);
        assert!(!b.contour.iter().any(|&v| v));
        assert!(!b.threshold.iter().any(|&v| v));
    }

    #[test]
    fn vertices_espaciados_no_dejan_un_buffer_en_cuentas() {
        // dos vértices a 400 m: CoastSat marcaría solo los extremos; aquí el
        // segmento completo queda dentro del buffer
        let r = ReferenceLine {
            lines: vec![vec![(1050.0, 1800.0), (1450.0, 1800.0)]],
            epsg: None,
        };
        let b = reference_buffer(&r, &gt_10m(), (50, 50), 10.0, None, 20.0).unwrap();
        for c in 5..=45 {
            assert!(b.contour[[20, c]], "columna {c} fuera del buffer");
        }
    }

    #[test]
    fn la_referencia_se_reproyecta_a_la_escena() {
        // misma línea en UTM 19S (32719) y en geográficas (4326): el buffer
        // debe tocar los mismos píxeles
        let gt = GeoTransform::new(260_000.0, 6_345_000.0, 30.0, -30.0);
        let utm = vec![(261_500.0, 6_344_700.0), (261_500.0, 6_343_500.0)];
        let lonlat = transform_points(&utm, 32719, 4326).unwrap();
        let a = reference_buffer(
            &ReferenceLine {
                lines: vec![utm],
                epsg: Some(32719),
            },
            &gt,
            (60, 100),
            30.0,
            Some(32719),
            90.0,
        )
        .unwrap();
        let b = reference_buffer(
            &ReferenceLine {
                lines: vec![lonlat],
                epsg: Some(4326),
            },
            &gt,
            (60, 100),
            30.0,
            Some(32719),
            90.0,
        )
        .unwrap();
        assert_eq!(a.contour, b.contour);
    }
}
