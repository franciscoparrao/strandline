//! Transectos costeros: generación perpendicular a una línea base e
//! intersección vectorizada shoreline×transecto.
//!
//! La intersección es el núcleo del benchmark que motivó strandline: el
//! `compute_intersection_QC` de CoastSat (loop Python) tardaba 20+ min para
//! 179 transectos × ~800 shorelines en Viña; la versión vectorizada corre
//! en segundos. Aquí se porta a Rust con paralelismo opcional (rayon).

mod intersect;
pub use intersect::{
    compute_intersections, IntersectionSettings, MultipleIntersection,
};

/// Un transecto costero: segmento desde `origin` (extremo tierra) hasta
/// `end` (extremo mar), en coordenadas proyectadas (mismo CRS que las
/// shorelines).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transect {
    /// Extremo hacia tierra (m).
    pub origin: (f64, f64),
    /// Extremo hacia el mar (m).
    pub end: (f64, f64),
}

impl Transect {
    /// Punto medio del transecto (sobre la línea base).
    pub fn midpoint(&self) -> (f64, f64) {
        (
            (self.origin.0 + self.end.0) / 2.0,
            (self.origin.1 + self.end.1) / 2.0,
        )
    }
}

/// Distancia euclidiana entre dos puntos.
#[inline]
fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Muestrea la polilínea `line` a intervalos regulares de `spacing` metros
/// de longitud de arco, devolviendo los puntos interpolados linealmente.
fn resample_by_arclength(line: &[(f64, f64)], spacing: f64) -> Vec<(f64, f64)> {
    // distancia acumulada
    let mut cum = vec![0.0f64];
    for w in line.windows(2) {
        cum.push(cum.last().unwrap() + dist(w[0], w[1]));
    }
    let total = *cum.last().unwrap();
    if total <= 0.0 {
        return line.to_vec();
    }
    let n = (total / spacing) as usize + 1;
    let mut out = Vec::with_capacity(n);
    let mut seg = 0usize;
    for i in 0..n {
        let target = if n > 1 {
            total * (i as f64) / ((n - 1) as f64)
        } else {
            0.0
        };
        while seg + 1 < cum.len() - 1 && cum[seg + 1] < target {
            seg += 1;
        }
        let (d0, d1) = (cum[seg], cum[seg + 1]);
        let frac = if d1 > d0 { (target - d0) / (d1 - d0) } else { 0.0 };
        let (p0, p1) = (line[seg], line[seg + 1]);
        out.push((
            p0.0 + frac * (p1.0 - p0.0),
            p0.1 + frac * (p1.1 - p0.1),
        ));
    }
    out
}

/// Genera transectos perpendiculares (shore-normal) a lo largo de la línea
/// base `reference_line`.
///
/// - `spacing`: separación entre transectos (m) a lo largo de la costa.
/// - `length`: largo total de cada transecto (m); el punto de la línea base
///   queda en el centro (`length/2` hacia tierra, `length/2` hacia el mar).
/// - `seaward_hint`: dirección aproximada hacia el mar (no necesita ser
///   unitaria). La normal de cada transecto se orienta para que su
///   componente en esta dirección sea positiva. Para la costa central de
///   Chile (mira al oeste) usar `(-1.0, 0.0)`.
///
/// Réplica del algoritmo del pipeline de referencia (tangente por
/// diferencias centrales sobre los puntos re-muestreados, normal rotada 90°
/// orientada al mar, transecto tierra→mar).
pub fn generate_transects(
    reference_line: &[(f64, f64)],
    spacing: f64,
    length: f64,
    seaward_hint: (f64, f64),
) -> Vec<Transect> {
    if reference_line.len() < 2 {
        return Vec::new();
    }
    let samples = resample_by_arclength(reference_line, spacing);
    let n = samples.len();
    let half = length / 2.0;
    let mut transects = Vec::with_capacity(n);

    for i in 0..n {
        // tangente local por diferencias centrales (forward/backward en bordes)
        let (de, dn) = if i == 0 {
            (samples[1].0 - samples[0].0, samples[1].1 - samples[0].1)
        } else if i == n - 1 {
            (
                samples[n - 1].0 - samples[n - 2].0,
                samples[n - 1].1 - samples[n - 2].1,
            )
        } else {
            (
                samples[i + 1].0 - samples[i - 1].0,
                samples[i + 1].1 - samples[i - 1].1,
            )
        };
        let tlen = (de * de + dn * dn).sqrt();
        if tlen == 0.0 {
            continue;
        }
        let (tx, ty) = (de / tlen, dn / tlen);
        // normal perpendicular a la tangente; elegir el sentido hacia el mar
        let (nx, ny) = (ty, -tx);
        let (nx, ny) = if nx * seaward_hint.0 + ny * seaward_hint.1 >= 0.0 {
            (nx, ny)
        } else {
            (-nx, -ny)
        };
        let c = samples[i];
        transects.push(Transect {
            origin: (c.0 - half * nx, c.1 - half * ny), // hacia tierra
            end: (c.0 + half * nx, c.1 + half * ny),    // hacia el mar
        });
    }
    transects
}

/// Nombre canónico de un transecto por índice (`T001`, `T002`, …), como en
/// el pipeline de referencia.
pub fn transect_name(index: usize) -> String {
    format!("T{:03}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_straight_line() {
        let line = vec![(0.0, 0.0), (100.0, 0.0)];
        let s = resample_by_arclength(&line, 25.0);
        assert_eq!(s.len(), 5); // 0,25,50,75,100
        assert!((s[1].0 - 25.0).abs() < 1e-9);
        assert!((s[4].0 - 100.0).abs() < 1e-9);
    }

    #[test]
    fn transect_perpendicular_to_straight_baseline() {
        // línea base recta hacia el norte; mar al oeste (-1,0)
        let line = vec![(1000.0, 0.0), (1000.0, 1000.0)];
        let ts = generate_transects(&line, 500.0, 400.0, (-1.0, 0.0));
        assert!(!ts.is_empty());
        // tangente = norte → normal shore-normal = este/oeste; con mar al
        // oeste, end debe estar al oeste (E menor) del origin
        let t = &ts[1];
        assert!(t.end.0 < t.origin.0, "end no apunta al mar");
        // el transecto debe ser horizontal (misma N) y de largo 400
        assert!((t.origin.1 - t.end.1).abs() < 1e-6);
        assert!((dist(t.origin, t.end) - 400.0).abs() < 1e-6);
    }
}
