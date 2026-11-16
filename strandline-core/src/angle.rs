//! Chequeo del ángulo de los transectos: cuánto se apartan los transectos
//! fijos de la normal local de la línea de costa instantánea.
//!
//! Una distancia medida a lo largo de un transecto que cruza la costa con un
//! ángulo `θ` respecto de su normal es la distancia perpendicular dividida por
//! `cos θ`. Si `θ` persiste en un transecto, la tasa hereda el factor cuando la
//! costa se mueve perpendicular a su orientación local.
//!
//! **Estimador.** Para cada transecto (recta de su primer a su segundo vértice)
//! y cada fecha, se busca el primer segmento de la línea de costa que lo cruza
//! (los dos parámetros en `[0, 1]`); la tangente local es el eje principal
//! (PCA) de los vértices del mismo tramo dentro de `±half_window` metros de
//! arco del cruce, y `θ = asin|û·t̂|`, con `û` la dirección del transecto. Los
//! tramos de una línea se separan donde un segmento supera
//! `max(100 m, 3 × mediana de los segmentos)`.
//!
//! **Nula.** `θ` está rectificado y el contorno está escalonado a escala de
//! píxel, así que el ruido del estimador infla el ángulo aunque el verdadero
//! sea cero. La nula aplica el mismo estimador a la línea de referencia desde
//! la que se generaron los transectos (ángulo verdadero cero), remuestreada a
//! la densidad de vértices de los contornos y cuantizada a la grilla de píxel
//! ([`null_line`]).
//!
//! Es un port del estimador de `spike/transect_angle/export_for_figure.py`,
//! validado contra su salida. Igual que el original, el segmento que une dos
//! tramos de una línea también puede contar como cruce.

use std::collections::HashMap;

use crate::sensor_offset::percentile;

/// Mínimo de vértices en la ventana para estimar la tangente.
const MIN_PTS: usize = 3;

/// Ángulos de un transecto, uno por fecha con cruce: `(índice de la fecha, θ°)`.
pub type AnglesByDate = Vec<(usize, f64)>;

fn norm(a: (f64, f64)) -> f64 {
    a.0.hypot(a.1)
}

fn sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}

fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

/// Límites de los tramos de una línea: índices `[0, ..., len]` donde empieza
/// cada tramo.
fn piece_bounds(p: &[(f64, f64)], gap: f64) -> Vec<usize> {
    let mut b = vec![0];
    for i in 1..p.len() {
        if norm(sub(p[i], p[i - 1])) > gap {
            b.push(i);
        }
    }
    b.push(p.len());
    b
}

/// Tangente unitaria (eje principal) alrededor del segmento `k`, o `None` si
/// el tramo o la ventana tienen menos de [`MIN_PTS`] vértices.
fn tangent_at(p: &[(f64, f64)], b: &[usize], k: usize, half_win: f64) -> Option<(f64, f64)> {
    // tramo j con b[j] <= k < b[j+1] (searchsorted side='right' − 1)
    let j = b.partition_point(|&x| x <= k) - 1;
    let (a, c) = (b[j], b[j + 1]);
    if c - a < MIN_PTS {
        return None;
    }
    let (mut lo, mut hi) = (k, k);
    let mut acc = 0.0;
    while lo > a && acc < half_win {
        acc += norm(sub(p[lo], p[lo - 1]));
        lo -= 1;
    }
    acc = 0.0;
    while hi + 1 < c && acc < half_win {
        acc += norm(sub(p[hi + 1], p[hi]));
        hi += 1;
    }
    let sel = &p[lo..=hi];
    if sel.len() < MIN_PTS {
        return None;
    }
    let n = sel.len() as f64;
    let mx = sel.iter().map(|q| q.0).sum::<f64>() / n;
    let my = sel.iter().map(|q| q.1).sum::<f64>() / n;
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for q in sel {
        let (dx, dy) = (q.0 - mx, q.1 - my);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    let (sxx, sxy, syy) = (sxx / n, sxy / n, syy / n);
    // autovalor mayor de [[sxx, sxy], [sxy, syy]] y su autovector
    let tr = sxx + syy;
    let disc = ((sxx - syy) * (sxx - syy) / 4.0 + sxy * sxy).sqrt();
    let l1 = tr / 2.0 + disc;
    if l1 <= 0.0 {
        return None;
    }
    let v = if sxy.abs() > 1e-300 {
        (l1 - syy, sxy)
    } else if sxx >= syy {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let nv = norm(v);
    Some((v.0 / nv, v.1 / nv))
}

/// Mide `θ` (grados) entre cada transecto y cada línea.
///
/// `transects` son `(nombre, vértices)`; se usa la recta del primer al segundo
/// vértice. `lines` son las líneas de costa por fecha, en el orden de las
/// fechas. Devuelve, por transecto con al menos un cruce, los pares
/// `(índice de la línea, θ)`.
pub fn measure(
    transects: &[(String, Vec<(f64, f64)>)],
    lines: &[Vec<(f64, f64)>],
    half_win: f64,
) -> HashMap<String, AnglesByDate> {
    struct Prep<'a> {
        di: usize,
        p: &'a [(f64, f64)],
        b: Vec<usize>,
    }
    let prep: Vec<Prep> = lines
        .iter()
        .enumerate()
        .filter(|(_, p)| p.len() >= 3)
        .map(|(di, p)| {
            let mut d: Vec<f64> = p.windows(2).map(|w| norm(sub(w[1], w[0]))).collect();
            let gap = 100f64.max(3.0 * percentile(&mut d, 50.0));
            Prep {
                di,
                p,
                b: piece_bounds(p, gap),
            }
        })
        .collect();

    let mut res = HashMap::new();
    for (name, tr) in transects {
        if tr.len() < 2 {
            continue;
        }
        let (o, e) = (tr[0], tr[1]);
        let d1 = sub(e, o);
        let l = norm(d1);
        if l == 0.0 {
            continue;
        }
        let u = (d1.0 / l, d1.1 / l);
        let mut acc = Vec::new();
        for pr in &prep {
            let p = pr.p;
            let mut hit = None;
            for k in 0..p.len() - 1 {
                let d2 = sub(p[k + 1], p[k]);
                let den = cross(d1, d2);
                if den == 0.0 {
                    continue;
                }
                let w = sub(o, p[k]);
                let mw = (-w.0, -w.1);
                let t = cross(mw, d2) / den;
                let s = cross(mw, d1) / den;
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
                    hit = Some(k);
                    break;
                }
            }
            let Some(k) = hit else { continue };
            let Some(tg) = tangent_at(p, &pr.b, k, half_win) else {
                continue;
            };
            let dot = (u.0 * tg.0 + u.1 * tg.1).abs().min(1.0);
            acc.push((pr.di, dot.asin().to_degrees()));
        }
        if !acc.is_empty() {
            res.insert(name.clone(), acc);
        }
    }
    res
}

/// Línea de la nula: `reference` remuestreada cada `step` metros de arco y
/// cuantizada a la grilla de `pixel` (en vértices alternos la x y la y), para
/// imitar el escalonado de un contorno de marching squares.
pub fn null_line(reference: &[(f64, f64)], step: f64, pixel: f64) -> Vec<(f64, f64)> {
    if reference.len() < 2 {
        return reference.to_vec();
    }
    let mut s = vec![0.0];
    for w in reference.windows(2) {
        s.push(s.last().unwrap() + norm(sub(w[1], w[0])));
    }
    let total = *s.last().unwrap();
    if total <= 0.0 {
        return reference.to_vec();
    }
    // np.arange(0, total, step) + np.interp
    let mut out = Vec::new();
    let mut seg = 0;
    let mut i = 0usize;
    loop {
        let t = i as f64 * step;
        if t >= total {
            break;
        }
        while seg + 1 < s.len() - 1 && s[seg + 1] < t {
            seg += 1;
        }
        // np.interp sobre s creciente (con tramos de largo 0 posibles)
        let (s0, s1) = (s[seg], s[seg + 1]);
        let f = if s1 > s0 { (t - s0) / (s1 - s0) } else { 0.0 };
        let (a, b) = (reference[seg], reference[seg + 1]);
        out.push((a.0 + f * (b.0 - a.0), a.1 + f * (b.1 - a.1)));
        i += 1;
    }
    for (k, q) in out.iter_mut().enumerate() {
        if k % 2 == 0 {
            q.0 = round_half_even(q.0 / pixel) * pixel;
        } else {
            q.1 = round_half_even(q.1 / pixel) * pixel;
        }
    }
    out
}

/// Redondeo al par más cercano en los empates, como `numpy.round`
/// (`f64::round_ties_even` exige Rust 1.77 y el MSRV es 1.75).
fn round_half_even(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

/// Resumen de un transecto.
#[derive(Debug, Clone)]
pub struct TransectAngle {
    pub transect: String,
    pub n_dates: usize,
    /// Mediana de `θ` entre fechas: la componente persistente.
    pub theta_pers: f64,
    /// Desviación absoluta mediana: la componente fluctuante.
    pub theta_mad: f64,
    /// Mediana en la primera y la segunda mitad de las fechas (`NaN` si una
    /// mitad tiene menos de 5).
    pub theta_h1: f64,
    pub theta_h2: f64,
}

impl TransectAngle {
    /// `1/cos θ_pers − 1`: inflación de la tasa si la costa se mueve
    /// perpendicular a su orientación local.
    pub fn inflation(&self) -> f64 {
        1.0 / self.theta_pers.to_radians().cos() - 1.0
    }
}

/// Resume cada transecto con al menos `min_dates` fechas. Las mitades se
/// cortan en la mediana del índice de fecha.
pub fn summarize(per: &HashMap<String, AnglesByDate>, min_dates: usize) -> Vec<TransectAngle> {
    let mut out: Vec<TransectAngle> = per
        .iter()
        .filter(|(_, acc)| acc.len() >= min_dates)
        .map(|(name, acc)| {
            let mut dis: Vec<f64> = acc.iter().map(|(d, _)| *d as f64).collect();
            let cut = percentile(&mut dis, 50.0);
            let mut vs: Vec<f64> = acc.iter().map(|(_, v)| *v).collect();
            let mut h1: Vec<f64> = acc.iter().filter(|(d, _)| (*d as f64) <= cut).map(|(_, v)| *v).collect();
            let mut h2: Vec<f64> = acc.iter().filter(|(d, _)| (*d as f64) > cut).map(|(_, v)| *v).collect();
            let med = percentile(&mut vs, 50.0);
            let mut dev: Vec<f64> = vs.iter().map(|v| (v - med).abs()).collect();
            let half = |h: &mut Vec<f64>| if h.len() >= 5 { percentile(h, 50.0) } else { f64::NAN };
            TransectAngle {
                transect: name.clone(),
                n_dates: acc.len(),
                theta_pers: med,
                theta_mad: percentile(&mut dev, 50.0),
                theta_h1: half(&mut h1),
                theta_h2: half(&mut h2),
            }
        })
        .collect();
    out.sort_by_key(|t| (transect_number(&t.transect), t.transect.clone()));
    out
}

/// Número de un transecto a partir de los dígitos de su nombre (`T042` → 42),
/// o `-1` si no tiene. Define la vecindad a lo largo de la costa.
pub fn transect_number(name: &str) -> i64 {
    let d: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    d.parse().unwrap_or(-1)
}

/// Correlación de Pearson; `NaN` con menos de `min_n` pares.
pub fn pearson(a: &[f64], b: &[f64], min_n: usize) -> f64 {
    let n = a.len();
    if n < min_n || n != b.len() {
        return f64::NAN;
    }
    let ma = a.iter().sum::<f64>() / n as f64;
    let mb = b.iter().sum::<f64>() / n as f64;
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    sab / (saa * sbb).sqrt()
}

/// Correlación de `θ_pers` entre transectos a `lag` números de distancia
/// (vecinos a lo largo de la costa); `NaN` con menos de 10 pares.
pub fn spatial_correlation(t: &[TransectAngle], lag: i64) -> f64 {
    let pos: HashMap<i64, f64> = t
        .iter()
        .map(|x| (transect_number(&x.transect), x.theta_pers))
        .collect();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for x in t {
        if let Some(v) = pos.get(&(transect_number(&x.transect) + lag)) {
            a.push(x.theta_pers);
            b.push(*v);
        }
    }
    pearson(&a, &b, 10)
}

/// Correlación de `θ` entre las dos mitades del registro; `NaN` con menos de
/// 10 transectos con ambas mitades.
pub fn temporal_correlation(t: &[TransectAngle]) -> f64 {
    let (a, b): (Vec<f64>, Vec<f64>) = t
        .iter()
        .filter(|x| x.theta_h1.is_finite() && x.theta_h2.is_finite())
        .map(|x| (x.theta_h1, x.theta_h2))
        .unzip();
    pearson(&a, &b, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(name: &str, o: (f64, f64), e: (f64, f64)) -> (String, Vec<(f64, f64)>) {
        (name.into(), vec![o, e])
    }

    /// Costa recta este-oeste (y = 0, vértices cada 10 m) y transectos
    /// norte-sur: perpendiculares, θ = 0. Un transecto girado 30° da 30°.
    #[test]
    fn angulo_conocido_sobre_costa_recta() {
        let line: Vec<(f64, f64)> = (0..200).map(|i| (i as f64 * 10.0, 0.0)).collect();
        let a = 30f64.to_radians();
        let ts = vec![
            tr("T1", (500.0, -100.0), (500.0, 100.0)),
            tr("T2", (1000.0 - 100.0 * a.sin(), -100.0 * a.cos()), (1000.0 + 100.0 * a.sin(), 100.0 * a.cos())),
        ];
        let r = measure(&ts, &[line], 100.0);
        assert!(r["T1"][0].1.abs() < 1e-9);
        assert!((r["T2"][0].1 - 30.0).abs() < 1e-9);
    }

    /// Un transecto que no alcanza la línea no tiene cruce.
    #[test]
    fn sin_cruce_no_hay_angulo() {
        let line: Vec<(f64, f64)> = (0..50).map(|i| (i as f64 * 10.0, 0.0)).collect();
        let r = measure(&[tr("T1", (100.0, 10.0), (100.0, 200.0))], &[line], 100.0);
        assert!(r.is_empty());
    }

    /// La nula de una referencia recta da ángulos chicos pero no nulos: el
    /// escalonado a escala de píxel basta para que el estimador rectificado
    /// devuelva unos grados.
    #[test]
    fn la_nula_tiene_piso_positivo() {
        // costa a 20° de la grilla
        let a = 20f64.to_radians();
        let reference: Vec<(f64, f64)> =
            (0..=100).map(|i| (i as f64 * 30.0 * a.cos(), i as f64 * 30.0 * a.sin())).collect();
        let nl = null_line(&reference, 15.0, 15.0);
        let n = (-a.sin(), a.cos());
        let ts: Vec<_> = (1..20)
            .map(|k| {
                let c = (k as f64 * 150.0 * a.cos(), k as f64 * 150.0 * a.sin());
                tr(&format!("T{k}"), (c.0 - 100.0 * n.0, c.1 - 100.0 * n.1), (c.0 + 100.0 * n.0, c.1 + 100.0 * n.1))
            })
            .collect();
        let r = measure(&ts, &[nl], 100.0);
        let mut v: Vec<f64> = r.values().map(|x| x[0].1).collect();
        assert!(!v.is_empty());
        let m = percentile(&mut v, 50.0);
        assert!(m > 0.0 && m < 10.0, "mediana de la nula {m}");
    }

    #[test]
    fn resumen_y_correlaciones() {
        // 12 transectos con θ creciente a lo largo de la costa, estables en el tiempo
        let mut per = HashMap::new();
        for k in 0..12 {
            let acc: AnglesByDate = (0..20).map(|d| (d, k as f64 + if d % 2 == 0 { 0.1 } else { -0.1 })).collect();
            per.insert(format!("T{k:03}"), acc);
        }
        let s = summarize(&per, 10);
        assert_eq!(s.len(), 12);
        assert_eq!(s[0].transect, "T000");
        assert!((s[5].theta_pers - 5.0).abs() < 1e-12);
        assert!(spatial_correlation(&s, 1) > 0.99);
        assert!(temporal_correlation(&s) > 0.99);
        assert!((s[0].theta_mad - 0.1).abs() < 1e-12);
    }
}
