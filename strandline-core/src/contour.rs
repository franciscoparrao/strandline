//! Marching squares con interpolación lineal subpixel.
//!
//! Réplica del algoritmo de `skimage.measure.find_contours`
//! (tabla de casos y orientación de segmentos idénticas, saddle points
//! resueltos como `fully_connected='low'`, el default de skimage que
//! usa CoastSat). Las celdas con alguna esquina no finita no generan
//! segmentos (equivalente al manejo de NaN de skimage).

use std::collections::HashMap;

use ndarray::ArrayView2;

/// Punto en coordenadas de imagen fraccionales `(row, col)`.
pub type Point = (f64, f64);

/// Polilínea de contorno en coordenadas de imagen `(row, col)`.
pub type Contour = Vec<Point>;

/// Fracción de cruce lineal entre dos valores de esquina.
#[inline]
fn frac(from: f64, to: f64, level: f64) -> f64 {
    if to == from {
        return 0.0;
    }
    (level - from) / (to - from)
}

/// Clave de hash para un punto (cuantizada para tolerar ruido de f64).
#[inline]
fn key(p: Point) -> (i64, i64) {
    const Q: f64 = (1u64 << 26) as f64;
    ((p.0 * Q).round() as i64, (p.1 * Q).round() as i64)
}

/// Extrae los contornos de nivel `level` sobre la grilla `z`.
///
/// Devuelve polilíneas en coordenadas `(row, col)` fraccionales, con la
/// misma posición subpixel que `skimage.measure.find_contours(z, level)`.
pub fn find_contours(z: ArrayView2<'_, f32>, level: f64) -> Vec<Contour> {
    let (rows, cols) = z.dim();
    if rows < 2 || cols < 2 {
        return Vec::new();
    }

    // 1. Generar segmentos orientados por celda (tabla de skimage).
    let mut segments: Vec<(Point, Point)> = Vec::new();
    for r0 in 0..rows - 1 {
        let r1 = r0 + 1;
        for c0 in 0..cols - 1 {
            let c1 = c0 + 1;
            let ul = z[[r0, c0]] as f64;
            let ur = z[[r0, c1]] as f64;
            let ll = z[[r1, c0]] as f64;
            let lr = z[[r1, c1]] as f64;
            if !(ul.is_finite() && ur.is_finite() && ll.is_finite() && lr.is_finite()) {
                continue;
            }

            let mut case = 0u8;
            if ul > level {
                case |= 1;
            }
            if ur > level {
                case |= 2;
            }
            if ll > level {
                case |= 4;
            }
            if lr > level {
                case |= 8;
            }
            if case == 0 || case == 15 {
                continue;
            }

            let top = || (r0 as f64, c0 as f64 + frac(ul, ur, level));
            let bottom = || (r1 as f64, c0 as f64 + frac(ll, lr, level));
            let left = || (r0 as f64 + frac(ul, ll, level), c0 as f64);
            let right = || (r0 as f64 + frac(ur, lr, level), c1 as f64);

            match case {
                1 => segments.push((top(), left())),
                2 => segments.push((right(), top())),
                3 => segments.push((right(), left())),
                4 => segments.push((left(), bottom())),
                5 => segments.push((top(), bottom())),
                6 => {
                    // saddle, fully_connected='low'
                    segments.push((right(), top()));
                    segments.push((left(), bottom()));
                }
                7 => segments.push((right(), bottom())),
                8 => segments.push((bottom(), right())),
                9 => {
                    // saddle, fully_connected='low'
                    segments.push((top(), left()));
                    segments.push((bottom(), right()));
                }
                10 => segments.push((bottom(), top())),
                11 => segments.push((bottom(), left())),
                12 => segments.push((left(), right())),
                13 => segments.push((top(), right())),
                14 => segments.push((left(), top())),
                _ => unreachable!(),
            }
        }
    }

    // 2. Ensamblar segmentos cabeza-cola en polilíneas.
    assemble(segments)
}

/// Une segmentos orientados (start → end) encadenando end == start.
fn assemble(segments: Vec<(Point, Point)>) -> Vec<Contour> {
    let n = segments.len();
    let mut by_start: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut by_end: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, seg) in segments.iter().enumerate() {
        by_start.entry(key(seg.0)).or_default().push(i);
        by_end.entry(key(seg.1)).or_default().push(i);
    }

    let mut used = vec![false; n];
    let mut contours = Vec::new();

    let take = |map: &mut HashMap<(i64, i64), Vec<usize>>,
                k: (i64, i64),
                used: &[bool]|
     -> Option<usize> {
        let bucket = map.get_mut(&k)?;
        while let Some(i) = bucket.pop() {
            if !used[i] {
                return Some(i);
            }
        }
        None
    };

    for start in 0..n {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain: std::collections::VecDeque<Point> = std::collections::VecDeque::new();
        chain.push_back(segments[start].0);
        chain.push_back(segments[start].1);

        // extender hacia adelante
        loop {
            let tail = *chain.back().unwrap();
            match take(&mut by_start, key(tail), &used) {
                Some(i) => {
                    used[i] = true;
                    chain.push_back(segments[i].1);
                }
                None => break,
            }
        }
        // extender hacia atrás
        loop {
            let head = *chain.front().unwrap();
            match take(&mut by_end, key(head), &used) {
                Some(i) => {
                    used[i] = true;
                    chain.push_front(segments[i].0);
                }
                None => break,
            }
        }
        contours.push(chain.into_iter().collect());
    }
    contours
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn single_vertical_interface() {
        // gradiente horizontal: nivel 0.5 cruza entre col 0 (0.0) y col 1 (1.0)
        let z = Array2::from_shape_fn((4, 2), |(_, c)| c as f32);
        let contours = find_contours(z.view(), 0.5);
        assert_eq!(contours.len(), 1);
        let c = &contours[0];
        assert_eq!(c.len(), 4);
        for p in c {
            assert!((p.1 - 0.5).abs() < 1e-12, "col = {}", p.1);
        }
    }

    #[test]
    fn closed_contour_around_peak() {
        let mut z = Array2::zeros((5, 5));
        z[[2, 2]] = 1.0f32;
        let contours = find_contours(z.view(), 0.5);
        assert_eq!(contours.len(), 1);
        let c = &contours[0];
        // contorno cerrado: primer y último punto coinciden
        let d0 = (c[0].0 - c.last().unwrap().0).abs() + (c[0].1 - c.last().unwrap().1).abs();
        assert!(d0 < 1e-12);
        // cruces a mitad de camino entre el peak y sus vecinos
        assert!(c
            .iter()
            .any(|p| (p.0 - 1.5).abs() < 1e-12 && (p.1 - 2.0).abs() < 1e-12));
    }

    #[test]
    fn nan_cells_skipped() {
        let mut z = Array2::from_shape_fn((4, 3), |(_, c)| c as f32);
        z[[1, 1]] = f32::NAN;
        let contours = find_contours(z.view(), 0.5);
        // las celdas que tocan el NaN no generan segmentos, el resto sí
        let total: usize = contours.iter().map(|c| c.len()).sum();
        assert!(total > 0);
        for c in &contours {
            for p in c {
                assert!(p.0.is_finite() && p.1.is_finite());
            }
        }
    }
}
