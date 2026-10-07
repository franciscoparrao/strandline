//! Extracción subpixel de la línea de agua por **ajuste de perfil** a lo
//! largo de cada transecto (S1 del roadmap SOTA, auditoría 2026-07).
//!
//! En vez de contornear el MNDWI con marching squares (que hereda el sesgo
//! de *pixel-locking* de los métodos de contorno y usa solo 2 píxeles por
//! cruce), se muestrea el MNDWI bilinealmente a lo largo de la normal a la
//! costa y se ajusta un modelo de borde sigmoide
//!
//! ```text
//! MNDWI(x) ≈ a + b·tanh((x − x₀) / w)
//! ```
//!
//! por mínimos cuadrados no lineales (Levenberg-Marquardt). La posición de
//! la línea de agua es donde el modelo cruza el umbral, con incertidumbre
//! analítica del ajuste — cada cruce usa ~20 muestras del perfil en vez de
//! 2, lo que reduce la varianza y elimina el anclaje a la grilla. Es la
//! familia de métodos de SHOREX/SAET (Pardo-Pascual et al.), que reporta
//! ~3–5 m de error absoluto con Landsat vs ~7–10 m del enfoque de contorno.

use ndarray::Array2;
use surtgis_core::raster::{GeoTransform, Raster};

use crate::transects::{generate_transects, Transect};

/// Parámetros del ajuste de perfil.
#[derive(Debug, Clone, Copy)]
pub struct ProfileSettings {
    /// Paso de muestreo a lo largo del transecto (m). Default: 5.
    pub step_m: f64,
    /// Semiancho de la ventana de ajuste alrededor del cruce inicial (m).
    /// Default: 60 (≈ 4 píxeles Landsat).
    pub window_m: f64,
    /// Mínimo de muestras finitas en la ventana para intentar el ajuste.
    pub min_samples: usize,
    /// Iteraciones máximas de Levenberg-Marquardt.
    pub max_iter: usize,
}

impl Default for ProfileSettings {
    fn default() -> Self {
        Self {
            step_m: 5.0,
            window_m: 60.0,
            min_samples: 8,
            max_iter: 30,
        }
    }
}

/// Resultado del ajuste de perfil en un transecto.
#[derive(Debug, Clone, Copy)]
pub struct ProfileFit {
    /// Posición de la línea de agua: distancia desde `origin` a lo largo
    /// del transecto (m) donde el modelo cruza el umbral.
    pub chainage_m: f64,
    /// Coordenadas de mundo del cruce.
    pub world: (f64, f64),
    /// Incertidumbre 1σ de `chainage_m` por ruido del perfil (m), de la
    /// covarianza del ajuste.
    pub sigma_m: f64,
    /// Sensibilidad de la posición al umbral: `∂x/∂threshold` (m por unidad
    /// de MNDWI) evaluada en el cruce. Multiplica por la incertidumbre del
    /// umbral para obtener esa componente de σ.
    pub dx_dthreshold: f64,
    /// RMSE del ajuste (unidades de MNDWI).
    pub rmse: f64,
    /// Muestras finitas usadas.
    pub n_samples: usize,
}

/// Muestrea `raster` bilinealmente en coordenadas de píxel fraccionales
/// `(row, col)` según la convención CoastSat (el valor del píxel vive en
/// el punto GT·(col, row)). Devuelve `NaN` fuera de rango o si algún
/// vecino del stencil es no finito.
fn sample_bilinear(data: &Array2<f32>, row: f64, col: f64) -> f64 {
    let (rows, cols) = data.dim();
    if row < 0.0 || col < 0.0 || row > (rows - 1) as f64 || col > (cols - 1) as f64 {
        return f64::NAN;
    }
    let r0 = row.floor() as usize;
    let c0 = col.floor() as usize;
    let r1 = (r0 + 1).min(rows - 1);
    let c1 = (c0 + 1).min(cols - 1);
    let fr = row - r0 as f64;
    let fc = col - c0 as f64;
    let v00 = data[[r0, c0]] as f64;
    let v01 = data[[r0, c1]] as f64;
    let v10 = data[[r1, c0]] as f64;
    let v11 = data[[r1, c1]] as f64;
    if !(v00.is_finite() && v01.is_finite() && v10.is_finite() && v11.is_finite()) {
        return f64::NAN;
    }
    v00 * (1.0 - fr) * (1.0 - fc) + v01 * (1.0 - fr) * fc + v10 * fr * (1.0 - fc) + v11 * fr * fc
}

/// Convierte un punto de mundo a coordenadas de píxel fraccionales con la
/// convención CoastSat (inversa del geotransform sin offset de medio píxel;
/// asume raster north-up sin rotación).
#[inline]
fn world_to_pix(gt: &GeoTransform, x: f64, y: f64) -> (f64, f64) {
    let g = gt.to_gdal();
    ((y - g[3]) / g[5], (x - g[0]) / g[1]) // (row, col)
}

/// Perfil muestreado a lo largo de un transecto: distancias desde `origin`
/// (m) y valores del raster.
pub fn sample_profile(
    raster: &Raster<f32>,
    transect: &Transect,
    step_m: f64,
) -> (Vec<f64>, Vec<f64>) {
    let gt = raster.transform();
    let (ox, oy) = transect.origin;
    let (dx, dy) = (transect.end.0 - ox, transect.end.1 - oy);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / len, dy / len);
    let n = (len / step_m).floor() as usize + 1;

    let mut dist = Vec::with_capacity(n);
    let mut vals = Vec::with_capacity(n);
    for i in 0..n {
        let d = i as f64 * step_m;
        let (row, col) = world_to_pix(gt, ox + d * ux, oy + d * uy);
        dist.push(d);
        vals.push(sample_bilinear(raster.data(), row, col));
    }
    (dist, vals)
}

/// Media móvil NaN-aware de semiancho `half` (en muestras). Solo para la
/// inicialización del cruce — el ajuste usa siempre los datos crudos.
fn smooth(vals: &[f64], half: usize) -> Vec<f64> {
    let n = vals.len();
    let mut out = vec![f64::NAN; n];
    for (i, o) in out.iter_mut().enumerate() {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(n);
        let mut acc = 0.0;
        let mut cnt = 0usize;
        for &v in &vals[lo..hi] {
            if v.is_finite() {
                acc += v;
                cnt += 1;
            }
        }
        if cnt > 0 {
            *o = acc / cnt as f64;
        }
    }
    out
}

/// Encuentra el cruce inicial del umbral en el perfil: el **último** cruce
/// ascendente (tierra→agua yendo hacia el mar, con MNDWI textbook creciente
/// hacia el agua) del perfil **suavizado** — el suavizado evita que picos
/// de ruido en las colas creen cruces espurios que descentren la ventana
/// de ajuste. Devuelve la distancia interpolada linealmente.
fn initial_crossing(dist: &[f64], vals: &[f64], level: f64) -> Option<f64> {
    let sm = smooth(vals, 2);
    let mut last = None;
    for i in 1..sm.len() {
        let (y0, y1) = (sm[i - 1], sm[i]);
        if y0.is_finite() && y1.is_finite() && y0 < level && y1 >= level {
            let f = (level - y0) / (y1 - y0);
            last = Some(dist[i - 1] + f * (dist[i] - dist[i - 1]));
        }
    }
    last
}

/// Modelo de borde y jacobiano: `y = a + b·tanh((x−x0)/w)`.
#[inline]
fn model(p: &[f64; 4], x: f64) -> f64 {
    p[0] + p[1] * ((x - p[2]) / p[3]).tanh()
}

/// Norma infinito del gradiente `Jᵀr` del modelo en `p`. Vale ~0 en un punto
/// estacionario (mínimo local), que es la señal de que un ajuste sin pasos
/// aceptados igual convergió.
fn gradient_inf_norm(xs: &[f64], ys: &[f64], p: &[f64; 4]) -> f64 {
    let mut jtr = [0.0f64; 4];
    for (&x, &y) in xs.iter().zip(ys) {
        let u = (x - p[2]) / p[3];
        let th = u.tanh();
        let sech2 = 1.0 - th * th;
        let j = [1.0, th, -p[1] * sech2 / p[3], -p[1] * sech2 * u / p[3]];
        let r = y - model(p, x);
        for a in 0..4 {
            jtr[a] += j[a] * r;
        }
    }
    jtr.iter().fold(0.0f64, |acc, g| acc.max(g.abs()))
}

/// Ajuste Levenberg-Marquardt del modelo de borde. Devuelve
/// `(params, cov_x0, rmse)` o `None` si el sistema es singular **o si el
/// ajuste no converge**. `cov_x0` es la varianza del parámetro `x₀`.
///
/// "No converge" significa que ningún paso fue aceptado (λ creció hasta
/// romper, o se agotaron las iteraciones sin una sola mejora del SSE) y el
/// punto inicial no era ya estacionario. Sin esta distinción la función
/// devolvía `Some` con los parámetros **iniciales intactos** y una σ
/// calculada sobre ellos: un ajuste que nunca ocurrió, indistinguible para
/// el consumidor de uno legítimo — y esas σ alimentan los pesos del WLR.
fn fit_edge(
    xs: &[f64],
    ys: &[f64],
    init: [f64; 4],
    max_iter: usize,
) -> Option<([f64; 4], f64, f64)> {
    let n = xs.len();
    if n < 5 {
        return None;
    }
    let mut p = init;
    let mut lambda = 1e-3;
    let mut sse: f64 = xs
        .iter()
        .zip(ys)
        .map(|(&x, &y)| (y - model(&p, x)).powi(2))
        .sum();
    // gradiente en el punto inicial: si ya es ~0, el init es estacionario y
    // que no se acepte ningún paso es convergencia, no fallo
    let grad0 = gradient_inf_norm(xs, ys, &p);
    let scale0 = n as f64 * (sse / n as f64).sqrt().max(1e-12);
    let init_is_stationary = grad0 <= 1e-8 * scale0;
    let mut accepted_any = false;

    for _ in 0..max_iter {
        // jacobiano y gradiente
        let mut jtj = [[0.0f64; 4]; 4];
        let mut jtr = [0.0f64; 4];
        for (&x, &y) in xs.iter().zip(ys) {
            let u = (x - p[2]) / p[3];
            let th = u.tanh();
            let sech2 = 1.0 - th * th;
            let j = [1.0, th, -p[1] * sech2 / p[3], -p[1] * sech2 * u / p[3]];
            let r = y - model(&p, x);
            for a in 0..4 {
                jtr[a] += j[a] * r;
                for b in 0..4 {
                    jtj[a][b] += j[a] * j[b];
                }
            }
        }
        // (JᵀJ + λ·diag) δ = Jᵀr
        let mut m = jtj;
        for (a, row) in m.iter_mut().enumerate() {
            row[a] *= 1.0 + lambda;
        }
        let delta = solve4(m, jtr)?;
        let mut p_new = p;
        for a in 0..4 {
            p_new[a] += delta[a];
        }
        // w debe mantenerse positivo y razonable
        if p_new[3] <= 1e-6 {
            p_new[3] = 1e-6;
        }
        let sse_new: f64 = xs
            .iter()
            .zip(ys)
            .map(|(&x, &y)| (y - model(&p_new, x)).powi(2))
            .sum();
        if sse_new < sse {
            let rel = (sse - sse_new) / sse.max(1e-30);
            p = p_new;
            sse = sse_new;
            accepted_any = true;
            lambda = (lambda * 0.3).max(1e-12);
            if rel < 1e-10 {
                break;
            }
        } else {
            lambda *= 10.0;
            if lambda > 1e8 {
                break;
            }
        }
    }

    // el ajuste no ocurrió: `p` sigue siendo `init`
    if !accepted_any && !init_is_stationary {
        return None;
    }

    // covarianza de x0: (JᵀJ)⁻¹[2,2] · sse/(n−4)
    let mut jtj = [[0.0f64; 4]; 4];
    for &x in xs {
        let u = (x - p[2]) / p[3];
        let th = u.tanh();
        let sech2 = 1.0 - th * th;
        let j = [1.0, th, -p[1] * sech2 / p[3], -p[1] * sech2 * u / p[3]];
        for a in 0..4 {
            for b in 0..4 {
                jtj[a][b] += j[a] * j[b];
            }
        }
    }
    let inv22 = invert4_diag_entry(jtj, 2)?;
    let dof = (n as f64 - 4.0).max(1.0);
    let var_x0 = inv22 * sse / dof;
    Some((p, var_x0, (sse / n as f64).sqrt()))
}

/// Resuelve `M·x = b` (4×4) por eliminación gaussiana con pivoteo parcial.
fn solve4(mut m: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        let mut piv = col;
        for r in col + 1..4 {
            if m[r][col].abs() > m[piv][col].abs() {
                piv = r;
            }
        }
        if m[piv][col].abs() < 1e-30 {
            return None;
        }
        m.swap(col, piv);
        b.swap(col, piv);
        let pivot_row = m[col];
        for r in col + 1..4 {
            let f = m[r][col] / pivot_row[col];
            for (c, &pv) in pivot_row.iter().enumerate().skip(col) {
                m[r][c] -= f * pv;
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = [0.0f64; 4];
    for r in (0..4).rev() {
        let mut acc = b[r];
        for c in r + 1..4 {
            acc -= m[r][c] * x[c];
        }
        x[r] = acc / m[r][r];
    }
    Some(x)
}

/// Entrada diagonal `[k,k]` de la inversa de una matriz 4×4 simétrica
/// definida positiva, resolviendo `M·x = e_k`.
fn invert4_diag_entry(m: [[f64; 4]; 4], k: usize) -> Option<f64> {
    let mut e = [0.0f64; 4];
    e[k] = 1.0;
    let x = solve4(m, e)?;
    let v = x[k];
    if v.is_finite() && v >= 0.0 {
        Some(v)
    } else {
        None
    }
}

/// Ajusta el perfil de un transecto y devuelve el cruce subpixel del
/// `level`, o `None` si no hay cruce o el ajuste no es utilizable.
///
/// `seed`: posición inicial (chainage, m desde `transect.origin`) para
/// centrar la ventana de ajuste. Si es `Some`, se usa directamente —
/// típicamente el cruce del contorno Otsu/clasificador con el mismo
/// transecto (ve todo el 2D de la escena, así que no se deja engañar por
/// features espurias entre el origen y la costa real: estacionamientos,
/// sombras, agua interrumpida). Si es `None`, cae al buscador ingenuo
/// [`initial_crossing`] (el último cruce ascendente del perfil 1D
/// suavizado) — puede enganchar el cruce equivocado en frentes urbanos
/// anchos donde hay más de una transición tierra/agua antes de la costa.
pub fn fit_profile(
    raster: &Raster<f32>,
    transect: &Transect,
    level: f64,
    seed: Option<f64>,
    settings: &ProfileSettings,
) -> Option<ProfileFit> {
    let (dist, vals) = sample_profile(raster, transect, settings.step_m);
    match seed {
        None => {
            let x0_init = initial_crossing(&dist, &vals, level)?;
            try_fit_window(raster, transect, level, &dist, &vals, x0_init, settings)
        }
        Some(x0_seed) => {
            // con semilla (típicamente el cruce del contorno Otsu, que ve
            // la escena en 2D) el resultado nunca es peor que la semilla:
            // si el perfil 1D local no bracket-ea el nivel alrededor de
            // ella (frente urbano ambiguo, plateau que no vuelve a cruzar
            // el umbral — ver `naive_search_engancha_la_meseta_espuria`),
            // se reporta la semilla misma en vez de `None`. El ajuste
            // subpixel solo se usa cuando localmente es válido.
            try_fit_window(raster, transect, level, &dist, &vals, x0_seed, settings)
                .or_else(|| Some(coarse_fit_at(transect, x0_seed)))
        }
    }
}

/// Punto reportado directamente en `chainage_m` (sin refinamiento
/// subpixel), usado como respaldo cuando hay semilla pero el ajuste local
/// no converge o no bracketea el nivel. `sigma_m`/`dx_dthreshold`/`rmse`
/// quedan en `NaN` — no hay información de incertidumbre del ajuste, a
/// diferencia de un `ProfileFit` refinado.
fn coarse_fit_at(transect: &Transect, chainage_m: f64) -> ProfileFit {
    let (ox, oy) = transect.origin;
    let (dx, dy) = (transect.end.0 - ox, transect.end.1 - oy);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / len, dy / len);
    ProfileFit {
        chainage_m,
        world: (ox + chainage_m * ux, oy + chainage_m * uy),
        sigma_m: f64::NAN,
        dx_dthreshold: f64::NAN,
        rmse: f64::NAN,
        n_samples: 0,
    }
}

/// Intenta el ajuste sigmoide en la ventana centrada en `x0_init`. `None`
/// si no hay suficientes muestras, el ajuste no converge, no bracketea
/// `level`, o el cruce cae fuera de lo muestreado.
#[allow(clippy::too_many_arguments)]
fn try_fit_window(
    raster: &Raster<f32>,
    transect: &Transect,
    level: f64,
    dist: &[f64],
    vals: &[f64],
    x0_init: f64,
    settings: &ProfileSettings,
) -> Option<ProfileFit> {
    // ventana alrededor del cruce
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for (&d, &v) in dist.iter().zip(vals) {
        if v.is_finite() && (d - x0_init).abs() <= settings.window_m {
            xs.push(d);
            ys.push(v);
        }
    }
    if xs.len() < settings.min_samples {
        return None;
    }

    // inicialización: lados tierra/agua respecto del cruce
    let (mut lo, mut hi, mut n_lo, mut n_hi) = (0.0, 0.0, 0usize, 0usize);
    for (&x, &y) in xs.iter().zip(&ys) {
        if x < x0_init {
            lo += y;
            n_lo += 1;
        } else {
            hi += y;
            n_hi += 1;
        }
    }
    if n_lo == 0 || n_hi == 0 {
        return None;
    }
    let (m_lo, m_hi) = (lo / n_lo as f64, hi / n_hi as f64);
    let cell = raster.cell_size();
    let init = [
        (m_lo + m_hi) / 2.0,
        (m_hi - m_lo) / 2.0,
        x0_init,
        1.5 * cell,
    ];

    let (p, var_x0, rmse) = fit_edge(&xs, &ys, init, settings.max_iter)?;

    // el modelo debe cruzar el level: a−|b| < level < a+|b|
    let (a, b, x0, w) = (p[0], p[1], p[2], p[3]);
    let arg = (level - a) / b;
    if !arg.is_finite() || arg.abs() >= 1.0 {
        return None;
    }
    // posición donde el MODELO cruza el umbral (no necesariamente x0)
    let x_cross = x0 + w * arg.atanh();
    // el cruce debe quedar dentro de la ventana muestreada
    if x_cross < xs[0] - settings.step_m || x_cross > xs[xs.len() - 1] + settings.step_m {
        return None;
    }

    // pendiente del modelo en el cruce → sensibilidad al umbral
    let sech2 = 1.0 - arg * arg; // tanh(u)=arg en el cruce
    let dy_dx = b * sech2 / w;
    if dy_dx.abs() < 1e-12 {
        return None;
    }

    let (ox, oy) = transect.origin;
    let (dx, dy) = (transect.end.0 - ox, transect.end.1 - oy);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / len, dy / len);

    Some(ProfileFit {
        chainage_m: x_cross,
        world: (ox + x_cross * ux, oy + x_cross * uy),
        sigma_m: var_x0.sqrt(),
        dx_dthreshold: 1.0 / dy_dx,
        rmse,
        n_samples: xs.len(),
    })
}

/// Ajusta todos los transectos (en paralelo con la feature `parallel`).
///
/// `seeds`: una entrada por transecto (mismo orden/largo que `transects`);
/// `None` en una posición cae al buscador ingenuo para ese transecto. Ver
/// [`fit_profile`].
pub fn fit_profiles(
    raster: &Raster<f32>,
    transects: &[Transect],
    level: f64,
    seeds: &[Option<f64>],
    settings: &ProfileSettings,
) -> Vec<Option<ProfileFit>> {
    debug_assert_eq!(transects.len(), seeds.len());
    let run = |(t, &seed): (&Transect, &Option<f64>)| fit_profile(raster, t, level, seed, settings);
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        transects.par_iter().zip(seeds).map(run).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        transects.iter().zip(seeds).map(run).collect()
    }
}

/// Refina subpixel una polilínea de contorno (p. ej. la salida cruda de
/// marching squares, con pixel-locking) generando mini-transectos
/// perpendiculares locales a lo largo de ella — reusa [`generate_transects`]
/// tratando la propia línea como "línea base", así que cada mini-transecto
/// queda centrado en el punto re-muestreado por construcción — y ajustando
/// el borde sigmoide con semilla en ese centro.
///
/// Cada punto refinado reemplaza al re-muestreado original si el ajuste
/// local converge y bracketea `level`; si no, conserva la posición
/// sembrada sin refinar ([`fit_profile`] con semilla nunca devuelve
/// `None`) — el resultado nunca es más grosero que el contorno de entrada,
/// solo potencialmente más preciso.
///
/// `spacing_m`: separación entre puntos a refinar a lo largo del arco de
/// `line`. El semiancho de los mini-transectos generados es
/// `settings.window_m` — el mismo que usa el ajuste, así que la ventana de
/// muestreo siempre coincide exactamente con la ventana de ajuste (sin
/// riesgo de recortar el borde por un mini-transecto más corto que la
/// ventana). La dirección de la normal (qué lado es "positivo") es
/// arbitraria — el ajuste no depende de esa convención, solo la posición
/// del cruce en el mundo, que se recupera igual para cualquier signo.
pub fn refine_line_subpixel(
    raster: &Raster<f32>,
    line: &[(f64, f64)],
    level: f64,
    spacing_m: f64,
    settings: &ProfileSettings,
) -> Vec<(f64, f64)> {
    if line.len() < 2 {
        return line.to_vec();
    }
    let half = settings.window_m;
    let mini_transects = generate_transects(line, spacing_m, 2.0 * half, (1.0, 0.0));
    mini_transects
        .iter()
        .filter_map(|t| fit_profile(raster, t, level, Some(half), settings))
        .map(|f| f.world)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raster sintético con frontera vertical tanh en `x_edge` (mundo),
    /// celdas de 15 m, origen (0,0), north-up con y negativo (como GEE).
    fn synthetic_edge(x_edge: f64, w_m: f64, noise: f64, seed: u64) -> Raster<f32> {
        let (rows, cols) = (40, 40);
        let cell = 15.0;
        let mut rng = seed;
        let mut next = move || {
            // xorshift simple, determinista
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f64 / u64::MAX as f64) - 0.5
        };
        let mut data = Vec::with_capacity(rows * cols);
        for _r in 0..rows {
            for c in 0..cols {
                // convención CoastSat: el píxel (r,c) vive en x = c·cell
                let x = c as f64 * cell;
                let v = 0.1 + 0.5 * ((x - x_edge) / w_m).tanh() + noise * next();
                data.push(v as f32);
            }
        }
        let mut r = Raster::<f32>::from_vec(data, rows, cols).unwrap();
        r.set_transform(GeoTransform::from_gdal([0.0, cell, 0.0, 0.0, 0.0, -cell]));
        r.set_nodata(None);
        r
    }

    fn transect_at_row(row: usize) -> Transect {
        // horizontal, tierra en x=0 (oeste) → mar en x=585 (este)
        let y = -(row as f64) * 15.0;
        Transect {
            origin: (0.0, y),
            end: (585.0, y),
        }
    }

    #[test]
    fn recovers_subpixel_edge_position() {
        // frontera en x = 300 + fracciones de píxel; sin ruido el ajuste
        // debe recuperar la posición a << 0.1 px (1.5 m)
        for frac in [0.0, 0.25, 0.5, 0.75] {
            let x_true = 300.0 + frac * 15.0;
            let raster = synthetic_edge(x_true, 15.0, 0.0, 42);
            let fit = fit_profile(
                &raster,
                &transect_at_row(20),
                0.1, // nivel = punto medio del tanh → cruce en x_true
                None,
                &ProfileSettings::default(),
            )
            .expect("ajuste falló");
            assert!(
                (fit.chainage_m - x_true).abs() < 1.0,
                "frac {frac}: {} vs {x_true}",
                fit.chainage_m
            );
        }
    }

    #[test]
    fn sigma_grows_with_noise() {
        let x_true = 300.0;
        let quiet = fit_profile(
            &synthetic_edge(x_true, 15.0, 0.005, 7),
            &transect_at_row(20),
            0.1,
            None,
            &ProfileSettings::default(),
        )
        .unwrap();
        let noisy = fit_profile(
            &synthetic_edge(x_true, 15.0, 0.08, 7),
            &transect_at_row(20),
            0.1,
            None,
            &ProfileSettings::default(),
        )
        .unwrap();
        assert!(
            noisy.sigma_m > quiet.sigma_m,
            "σ ruidoso {} <= σ silencioso {}",
            noisy.sigma_m,
            quiet.sigma_m
        );
    }

    #[test]
    fn no_crossing_returns_none() {
        // nivel fuera del rango del perfil → sin cruce
        let raster = synthetic_edge(300.0, 15.0, 0.0, 1);
        let fit = fit_profile(
            &raster,
            &transect_at_row(20),
            5.0,
            None,
            &ProfileSettings::default(),
        );
        assert!(fit.is_none());
    }

    #[test]
    fn dx_dthreshold_matches_edge_width() {
        // para y = a + b·tanh((x−x0)/w), en el punto medio dy/dx = b/w
        // → dx/dthreshold = w/b = 15/0.5 = 30 m por unidad de MNDWI.
        // El muestreo bilineal convoluciona el borde con un kernel
        // triangular de 1 px, ensanchando el w efectivo ~10-15% (la
        // posición x₀ no se sesga: el kernel es simétrico) → se acepta
        // [28, 38].
        let raster = synthetic_edge(300.0, 15.0, 0.0, 3);
        let fit = fit_profile(
            &raster,
            &transect_at_row(20),
            0.1,
            None,
            &ProfileSettings::default(),
        )
        .unwrap();
        assert!(
            fit.dx_dthreshold > 28.0 && fit.dx_dthreshold < 38.0,
            "dx/dt = {}",
            fit.dx_dthreshold
        );
    }

    /// Perfil con una meseta espuria "tierra→casi-agua" cerca del origen
    /// (ej. estacionamiento/sombra/arena húmeda) que nunca vuelve a bajar
    /// del umbral antes del borde real más lejos — reproduce el caso real
    /// de Viña (adenda profile_sr): el buscador ingenuo [`initial_crossing`]
    /// solo ve UNA transición ascendente (la espuria, ~90 m) porque tras
    /// ella el perfil queda por encima del nivel sin volver a cruzar hacia
    /// arriba en el borde verdadero (~350 m) — de hecho ahí ni siquiera hay
    /// un cruce LOCAL de `level` (todo el entorno de 350 m ya está por
    /// encima de 0.3), así que ni el ajuste sembrado puede bracket-earlo:
    /// el caso ejercita el *fallback* a la semilla misma, no el
    /// refinamiento subpixel (ver `seed_recupera_borde_con_ajuste_valido`
    /// para ese caso).
    fn synthetic_false_plateau_then_edge(x_true: f64) -> Raster<f32> {
        let (rows, cols) = (40, 40);
        let cell = 15.0;
        let mut data = Vec::with_capacity(rows * cols);
        for _r in 0..rows {
            for c in 0..cols {
                let x = c as f64 * cell;
                let v = if x < 90.0 {
                    0.1
                } else {
                    // meseta espuria (asíntota izquierda 0.40) que sube
                    // suavemente a "agua" (asíntota derecha 0.70) en
                    // x_true — ambas asíntotas quedan por encima de 0.3,
                    // nunca pasa por debajo desde x=90 (a diferencia de
                    // usar amplitud 0.3 directa, que satura a [0.1,0.7] y
                    // sí vuelve a cruzar el nivel).
                    0.55 + 0.15 * ((x - x_true) / 15.0).tanh()
                };
                data.push(v as f32);
            }
        }
        let mut r = Raster::<f32>::from_vec(data, rows, cols).unwrap();
        r.set_transform(GeoTransform::from_gdal([0.0, cell, 0.0, 0.0, 0.0, -cell]));
        r.set_nodata(None);
        r
    }

    #[test]
    fn naive_search_engancha_la_meseta_espuria() {
        let raster = synthetic_false_plateau_then_edge(350.0);
        let fit = fit_profile(
            &raster,
            &transect_at_row(20),
            0.3,
            None,
            &ProfileSettings::default(),
        );
        // o no converge (0.3 nunca vuelve a cruzarse tras la meseta), o
        // converge cerca de la meseta espuria (~90 m) — en ningún caso
        // cerca del borde verdadero (350 m).
        if let Some(f) = fit {
            assert!(
                (f.chainage_m - 350.0).abs() > 50.0,
                "el buscador ingenuo no debería acertar el borde verdadero: {}",
                f.chainage_m
            );
        }
    }

    #[test]
    fn seed_cae_a_la_semilla_cuando_no_hay_cruce_local() {
        // 0.3 no bracket-ea el rango [0.4, 0.7] visible alrededor de 350 m
        // → el ajuste local no es válido y se reporta la semilla misma
        // (fallback), no `None`: el resultado sembrado nunca es peor que
        // el cruce Otsu que lo originó.
        let raster = synthetic_false_plateau_then_edge(350.0);
        let fit = fit_profile(
            &raster,
            &transect_at_row(20),
            0.3,
            Some(350.0), // cruce del contorno Otsu (2D, no ve la meseta como costa)
            &ProfileSettings::default(),
        )
        .expect("con semilla el resultado nunca debería ser None");
        assert!(
            (fit.chainage_m - 350.0).abs() < 1e-9,
            "chainage = {} vs 350.0 esperado (fallback exacto a la semilla)",
            fit.chainage_m
        );
        assert!(
            fit.sigma_m.is_nan(),
            "fallback no debería reportar σ del ajuste"
        );
    }

    #[test]
    fn seed_recupera_borde_con_ajuste_valido() {
        // borde sigmoide real que sí bracketea el nivel (0.3 dentro de
        // [0.0, 0.6]) a 350 m, con una semilla imprecisa (340 m, como
        // daría un cruce Otsu de resolución de píxel) — el refinamiento
        // subpixel debe acercarse más al borde real que la semilla misma.
        let raster = synthetic_edge(350.0, 15.0, 0.0, 11);
        let fit = fit_profile(
            &raster,
            &transect_at_row(20),
            0.1, // punto medio del tanh (a=0.1, b=0.5) → cruce en 350
            Some(340.0),
            &ProfileSettings::default(),
        )
        .expect("debería converger con una semilla cercana");
        assert!(
            !fit.sigma_m.is_nan(),
            "esperaba un ajuste refinado, no el fallback"
        );
        assert!(
            (fit.chainage_m - 350.0).abs() < 1.0,
            "chainage = {} vs 350.0 esperado",
            fit.chainage_m
        );
    }

    /// `fit_edge` devolvía `Some` con los parámetros **iniciales intactos**
    /// cuando LM no lograba aceptar ni un paso (λ crecía hasta romper): un
    /// ajuste que nunca ocurrió, con σ de apariencia legítima. Ahora eso es
    /// `None`, y el caso simétrico (init ya estacionario) sigue siendo un
    /// resultado válido.
    #[test]
    fn fit_edge_sin_pasos_aceptados_devuelve_none() {
        // Datos que el modelo a + b·tanh((x−x0)/w) no puede ajustar mejor
        // que su init desde un init muy alejado: ruido puro de alta
        // frecuencia, sin borde. Con `max_iter` = 1 se garantiza que si el
        // único paso se rechaza, no hubo ninguno aceptado.
        let xs: Vec<f64> = (0..40).map(|i| i as f64).collect();
        let ys: Vec<f64> = (0..40)
            .map(|i| if i % 2 == 0 { 1000.0 } else { -1000.0 })
            .collect();
        // init absurdo (w minúsculo, x0 fuera del rango) del que LM no puede
        // salir en un solo paso
        let init = [0.0, 1e-6, -1e6, 1e-6];
        assert!(
            fit_edge(&xs, &ys, init, 1).is_none(),
            "un ajuste sin ningún paso aceptado no debe reportarse como válido"
        );

        // control: un borde real con init razonable SÍ converge
        let xs: Vec<f64> = (0..40).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs
            .iter()
            .map(|&x| 0.1 + 0.5 * ((x - 20.0) / 4.0).tanh())
            .collect();
        let (p, var, rmse) = fit_edge(&xs, &ys, [0.0, 0.4, 17.0, 5.0], 60)
            .expect("un borde sigmoide limpio debe converger");
        assert!((p[2] - 20.0).abs() < 0.5, "x0 = {}", p[2]);
        assert!(var.is_finite() && rmse < 1e-3, "var = {var}, rmse = {rmse}");
    }

    /// El fallback sembrado debe seguir activándose (y marcando σ=NaN) ahora
    /// que `fit_edge` puede devolver `None` por no-convergencia, no solo por
    /// singularidad: el contrato "nunca peor que la semilla" se mantiene.
    #[test]
    fn fallback_a_la_semilla_sigue_activo_con_ajuste_no_convergente() {
        let raster = synthetic_false_plateau_then_edge(350.0);
        let fit = fit_profile(
            &raster,
            &transect_at_row(20),
            0.3,
            Some(350.0),
            &ProfileSettings::default(),
        )
        .expect("con semilla nunca debería ser None");
        assert!((fit.chainage_m - 350.0).abs() < 1e-9);
        assert!(fit.sigma_m.is_nan());
    }

    #[test]
    fn refine_line_subpixel_mejora_un_contorno_pixel_locked() {
        // borde vertical real en x=350 (uniforme en y, como synthetic_edge).
        // Se simula un contorno "crudo" (marching squares) con el sesgo de
        // pixel-locking típico: desplazado medio píxel (7.5 m) del borde
        // real, y constante a lo largo de y (sin la variación real de un
        // contorno diagonal, mismo espíritu que transect_at_row).
        let x_true = 350.0;
        let raster = synthetic_edge(x_true, 15.0, 0.0, 5);
        let raw_line: Vec<(f64, f64)> = (5..35)
            .map(|r| (x_true + 7.5, -(r as f64) * 15.0))
            .collect();

        let refined = refine_line_subpixel(
            &raster,
            &raw_line,
            0.1, // mismo nivel que recovers_subpixel_edge_position
            15.0,
            &ProfileSettings::default(),
        );

        assert!(!refined.is_empty(), "no debería vaciar la línea");
        let raw_err = 7.5_f64;
        for &(x, _y) in &refined {
            assert!(
                (x - x_true).abs() < raw_err,
                "punto refinado x={x} no mejora el crudo (offset {raw_err}) respecto a x_true={x_true}"
            );
        }
        // al menos la mayoría debería quedar en precisión subpixel real
        let n_close = refined
            .iter()
            .filter(|&&(x, _)| (x - x_true).abs() < 1.0)
            .count();
        assert!(
            n_close * 2 >= refined.len(),
            "esperaba que al menos la mitad de los puntos refinados quedaran <1 m del borde real"
        );
    }

    #[test]
    fn refine_line_subpixel_linea_corta_pasa_intacta() {
        let raster = synthetic_edge(300.0, 15.0, 0.0, 1);
        let short = vec![(100.0, 0.0)];
        let refined = refine_line_subpixel(&raster, &short, 0.1, 15.0, &ProfileSettings::default());
        assert_eq!(refined, short);
    }
}
