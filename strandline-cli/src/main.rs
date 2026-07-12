//! CLI de strandline. Subcomandos `extract` (shoreline) y `tide`
//! (predicción y corrección mareal).

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use strandline_core::qc::{self, QcCriteria};
use strandline_core::rates::{compute_rates, decimal_year_from_mjd, despike};
use strandline_core::tide::{self, harmonic::TidePredictor, TideCorrection};
use strandline_core::transects::{
    compute_intersections, generate_transects, transect_name, IntersectionSettings, Transect,
};
use strandline_core::{extract_shoreline, ExtractParams};

#[derive(Parser)]
#[command(name = "strandline", version, about = "Satellite-derived shoreline analysis engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extrae la shoreline subpixel de un GeoTIFF multiespectral.
    Extract {
        /// GeoTIFF multiespectral de entrada (bandas B,G,R,NIR,SWIR1).
        #[arg(long)]
        ms: PathBuf,
        /// Máscara QA_PIXEL opcional (Landsat Collection 2).
        #[arg(long)]
        mask: Option<PathBuf>,
        /// Banda Green (1-based).
        #[arg(long, default_value_t = 2)]
        green_band: usize,
        /// Banda NIR (1-based); usada por --coastsat-compat y el clasificador.
        #[arg(long, default_value_t = 4)]
        nir_band: usize,
        /// Banda SWIR1 (1-based).
        #[arg(long, default_value_t = 5)]
        swir1_band: usize,
        /// Largo mínimo de shoreline en metros.
        #[arg(long, default_value_t = 200.0)]
        min_length: f64,
        /// Umbral MNDWI fijo (default: Otsu automático).
        #[arg(long)]
        level: Option<f32>,
        /// Radio (px) del corredor para la segunda pasada de Otsu (0 = off).
        #[arg(long, default_value_t = 10)]
        refine_radius: usize,
        /// Usa el clasificador de píxeles (MLP portado de CoastSat) + Otsu
        /// balanceado sand/water en vez de Otsu global.
        #[arg(long)]
        classify: bool,
        /// Replica el quirk de umbral NDWI/MNDWI de CoastSat (implica --classify).
        #[arg(long)]
        coastsat_compat: bool,
        /// Área mínima (m²) para conservar un blob sand/water clasificado.
        #[arg(long, default_value_t = 500.0)]
        min_beach_area: f64,
        /// Buffer de exclusión alrededor de nubes (m); 0 = off.
        #[arg(long, default_value_t = 300.0)]
        dist_clouds: f64,
        /// Tamaño mínimo (px) de un blob de nube en la máscara; 0 = sin filtro.
        #[arg(long, default_value_t = 40)]
        cloud_min_size: usize,
        /// EPSG de salida (4326 o UTM WGS84 326xx/327xx). Con 4326 el
        /// GeoJSON es conforme a RFC 7946. Default: CRS nativo de la escena.
        #[arg(long)]
        output_epsg: Option<u32>,
        /// GeoJSON de salida.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Predice la marea (síntesis armónica nativa) para las fechas dadas y,
    /// opcionalmente, calcula la corrección horizontal de Vos et al. (2019).
    Tide {
        /// CSV de constituyentes locales (columnas: name, amplitude, phase_g).
        #[arg(long)]
        constituents: PathBuf,
        /// CSV con una columna de timestamps (UTC ISO) a predecir.
        #[arg(long)]
        dates: PathBuf,
        /// Nombre de la columna de timestamps en el CSV de fechas.
        #[arg(long, default_value = "date")]
        date_col: String,
        /// Nivel medio del mar respecto al datum (m).
        #[arg(long, default_value_t = 0.0)]
        mean_sea_level: f64,
        /// Pendiente de playa tan(β); si se da, agrega la corrección
        /// horizontal (m) a la salida.
        #[arg(long)]
        beach_slope: Option<f64>,
        /// Elevación del datum de referencia para la corrección (m).
        #[arg(long, default_value_t = 0.0)]
        reference_elevation: f64,
        /// CSV de salida (timestamp_mjd, tide_m[, correction_m]).
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Genera transectos perpendiculares (shore-normal) a una línea base.
    Transects {
        /// CSV de la línea base (columnas: easting, northing).
        #[arg(long)]
        baseline: PathBuf,
        /// Separación entre transectos a lo largo de la costa (m).
        #[arg(long, default_value_t = 100.0)]
        spacing: f64,
        /// Largo total de cada transecto (m).
        #[arg(long, default_value_t = 500.0)]
        length: f64,
        /// Componente Este de la dirección aproximada hacia el mar.
        #[arg(long, default_value_t = -1.0)]
        seaward_e: f64,
        /// Componente Norte de la dirección aproximada hacia el mar.
        #[arg(long, default_value_t = 0.0)]
        seaward_n: f64,
        /// CSV de salida (name, origin_e, origin_n, end_e, end_n).
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Calcula la intersección cross-shore de shorelines con transectos
    /// (versión vectorizada del compute_intersection_QC de CoastSat).
    Intersect {
        /// CSV de transectos (name, origin_e, origin_n, end_e, end_n).
        #[arg(long)]
        transects: PathBuf,
        /// CSV de shorelines (shoreline_id, easting, northing), en orden.
        #[arg(long)]
        shorelines: PathBuf,
        /// Semiancho de la franja alrededor del eje del transecto (m).
        #[arg(long, default_value_t = 25.0)]
        along_dist: f64,
        /// CSV de salida en formato largo (transect, shoreline_id, chainage_m).
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Calcula tasas de cambio costero DSAS (NSM/SCE/EPR/LRR/WLR) por
    /// transecto a partir de las intersecciones cross-shore.
    Rates {
        /// CSV de intersecciones (transect, shoreline_id, chainage_m) —
        /// típicamente la salida de `intersect`.
        #[arg(long)]
        intersections: PathBuf,
        /// CSV con fecha (y peso opcional) por shoreline
        /// (columnas: shoreline_id, date[, weight]).
        #[arg(long)]
        dates: PathBuf,
        /// Mínimo de observaciones válidas para calcular tasas.
        #[arg(long, default_value_t = 10)]
        min_valid: usize,
        /// Salto máximo (m) entre observaciones consecutivas antes de aplicar
        /// despiking; 0 = desactivado.
        #[arg(long, default_value_t = 0.0)]
        despike_max_change: f64,
        /// Mínimo de observaciones válidas para aplicar despiking.
        #[arg(long, default_value_t = 30)]
        despike_min_valid: usize,
        /// CSV opcional de corrección mareal por shoreline
        /// (columnas: shoreline_id, correction_m); se resta del chainage
        /// antes del despiking y las tasas (Vos et al. 2019).
        #[arg(long)]
        tide_correction: Option<PathBuf>,
        /// Aplica los 5 filtros QC calibrados y agrega columnas
        /// `excluded,reason` a la salida.
        #[arg(long)]
        qc: bool,
        /// |LRR| máximo (m/año) para el QC.
        #[arg(long, default_value_t = 5.0)]
        max_abs_lrr: f64,
        /// SCE máximo (m) para el QC.
        #[arg(long, default_value_t = 500.0)]
        max_sce: f64,
        /// Span temporal mínimo (años) para el QC.
        #[arg(long, default_value_t = 20.0)]
        min_years: f64,
        /// Observaciones válidas mínimas para el QC.
        #[arg(long, default_value_t = 50)]
        min_valid_qc: usize,
        /// R² mínimo del LRR para el QC.
        #[arg(long, default_value_t = 0.01)]
        min_r2: f64,
        /// CSV de salida con las tasas por transecto.
        #[arg(short, long)]
        output: PathBuf,
    },
}

/// Lee un CSV `shoreline_id, correction_m` a un mapa.
fn read_tide_correction(
    path: &PathBuf,
) -> anyhow::Result<std::collections::HashMap<String, f64>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo corrección mareal {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let ci = header.iter().position(|c| c == "correction_m").context("falta 'correction_m'")?;
    let mut map = std::collections::HashMap::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(ci) {
            continue;
        }
        if let Ok(v) = f[ci].parse::<f64>() {
            map.insert(f[idi].to_string(), v);
        }
    }
    Ok(map)
}

/// Fecha (año decimal) y peso de una shoreline, indexado por `shoreline_id`.
type DateWeights = std::collections::HashMap<String, (f64, f64)>;

/// Lee `shoreline_id, date[, weight]` y devuelve año decimal + peso por id.
fn read_date_weights(path: &PathBuf) -> anyhow::Result<DateWeights> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo fechas {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let di = header.iter().position(|c| c == "date").context("falta 'date'")?;
    let wi = header.iter().position(|c| c == "weight");

    let mut map = DateWeights::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(di) {
            continue;
        }
        let Some(mjd) = tide::parse_timestamp(f[di]) else {
            continue;
        };
        let weight = wi
            .and_then(|i| f.get(i))
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);
        map.insert(f[idi].to_string(), (decimal_year_from_mjd(mjd), weight));
    }
    Ok(map)
}

/// Lee transectos desde el CSV que escribe `write_transects_csv`.
fn read_transects_csv(path: &PathBuf) -> anyhow::Result<Vec<Transect>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo transectos {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    lines.next(); // header conocido: name,origin_e,origin_n,end_e,end_n
    let mut out = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() < 5 {
            continue;
        }
        out.push(Transect {
            origin: (f[1].parse()?, f[2].parse()?),
            end: (f[3].parse()?, f[4].parse()?),
        });
    }
    Ok(out)
}

/// Shorelines leídas: los ids (en orden de aparición) y los puntos de cada
/// una.
type Shorelines = (Vec<String>, Vec<Vec<(f64, f64)>>);

/// Lee shorelines agrupadas por `shoreline_id` (preserva el orden de
/// aparición de los ids y de los puntos dentro de cada uno).
fn read_shorelines_csv(path: &PathBuf) -> anyhow::Result<Shorelines> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo shorelines {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let ei = header.iter().position(|c| c == "easting").context("falta 'easting'")?;
    let ni = header.iter().position(|c| c == "northing").context("falta 'northing'")?;

    let mut ids: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut last_id: Option<String> = None;
    for l in lines {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(ei).max(ni) {
            continue;
        }
        let id = f[idi].to_string();
        if last_id.as_ref() != Some(&id) {
            ids.push(id.clone());
            groups.push(Vec::new());
            last_id = Some(id);
        }
        if let (Ok(e), Ok(n)) = (f[ei].parse::<f64>(), f[ni].parse::<f64>()) {
            groups.last_mut().unwrap().push((e, n));
        }
    }
    Ok((ids, groups))
}

/// Lee una línea base desde CSV con columnas `easting`, `northing`.
fn read_baseline(path: &PathBuf) -> anyhow::Result<Vec<(f64, f64)>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo baseline {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let ei = header
        .iter()
        .position(|c| c == "easting")
        .context("falta columna 'easting'")?;
    let ni = header
        .iter()
        .position(|c| c == "northing")
        .context("falta columna 'northing'")?;
    let mut out = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= ei.max(ni) {
            continue;
        }
        if let (Ok(e), Ok(n)) = (f[ei].parse::<f64>(), f[ni].parse::<f64>()) {
            out.push((e, n));
        }
    }
    Ok(out)
}

/// Escribe transectos a CSV con nombres canónicos T001, T002, …
fn write_transects_csv(transects: &[Transect], path: &PathBuf) -> anyhow::Result<()> {
    let mut out = String::from("name,origin_e,origin_n,end_e,end_n\n");
    for (i, t) in transects.iter().enumerate() {
        out.push_str(&format!(
            "{},{:.6},{:.6},{:.6},{:.6}\n",
            transect_name(i),
            t.origin.0,
            t.origin.1,
            t.end.0,
            t.end.1
        ));
    }
    std::fs::write(path, out).with_context(|| format!("escribiendo {}", path.display()))?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Extract {
            ms,
            mask,
            green_band,
            nir_band,
            swir1_band,
            min_length,
            level,
            refine_radius,
            classify,
            coastsat_compat,
            min_beach_area,
            dist_clouds,
            cloud_min_size,
            output_epsg,
            output,
        } => {
            let params = ExtractParams {
                green_band,
                nir_band,
                swir1_band,
                min_length_m: min_length,
                level,
                refine_radius_px: refine_radius,
                use_classifier: classify || coastsat_compat,
                coastsat_compat,
                min_beach_area_m2: min_beach_area,
                dist_clouds_m: dist_clouds,
                cloud_min_size_px: cloud_min_size,
            };
            let mut shoreline = extract_shoreline(&ms, mask.as_deref(), &params)
                .with_context(|| format!("extracting shoreline from {}", ms.display()))?;
            if let Some(epsg) = output_epsg {
                shoreline
                    .reproject(epsg)
                    .with_context(|| format!("reproyectando a EPSG:{epsg}"))?;
            }

            let scene = ms
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let gj = shoreline.to_geojson(&scene);
            std::fs::write(&output, serde_json::to_string(&gj)?)
                .with_context(|| format!("writing {}", output.display()))?;

            let [t_read, t_mndwi, t_classify, t_otsu, t_contour] = shoreline.timings_ms;
            eprintln!(
                "{}: {} líneas, umbral = {:.4}, EPSG = {:?}",
                scene,
                shoreline.lines.len(),
                shoreline.threshold,
                shoreline.epsg
            );
            if let Some(cls) = shoreline.classification {
                eprintln!(
                    "  clasificación: sand = {} px, whitewater = {} px, water = {} px",
                    cls.sand_px, cls.whitewater_px, cls.water_px
                );
            }
            eprintln!(
                "  tiempos [ms]: read = {t_read:.1}, mndwi = {t_mndwi:.1}, classify = {t_classify:.1}, otsu = {t_otsu:.1}, contour = {t_contour:.1}, total = {:.1}",
                t_read + t_mndwi + t_classify + t_otsu + t_contour
            );
        }
        Command::Tide {
            constituents,
            dates,
            date_col,
            mean_sea_level,
            beach_slope,
            reference_elevation,
            output,
        } => {
            let records = tide::read_constituents_csv(&constituents)
                .with_context(|| format!("leyendo constituyentes de {}", constituents.display()))?;
            let (predictor, unknown) = TidePredictor::from_records(&records, mean_sea_level);
            if !unknown.is_empty() {
                eprintln!(
                    "aviso: {} constituyente(s) desconocido(s) ignorado(s): {}",
                    unknown.len(),
                    unknown.join(", ")
                );
            }
            anyhow::ensure!(!predictor.is_empty(), "no hay constituyentes válidos");

            // fechas: parsear la columna de timestamps a MJD
            let text = std::fs::read_to_string(&dates)
                .with_context(|| format!("leyendo fechas de {}", dates.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            let header: Vec<String> = lines
                .next()
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            let ci = header.iter().position(|c| c == &date_col).ok_or_else(|| {
                anyhow::anyhow!("falta columna '{date_col}' en {}", dates.display())
            })?;
            let mjds: Vec<f64> = lines
                .filter_map(|l| l.split(',').nth(ci).map(str::trim).and_then(tide::parse_timestamp))
                .collect();
            anyhow::ensure!(!mjds.is_empty(), "no se parseó ninguna fecha");

            let tides = predictor.predict_many(&mjds);
            let correction = beach_slope
                .map(|slope| TideCorrection::new(slope, reference_elevation));

            let mut out = String::from("mjd,tide_m");
            if correction.is_some() {
                out.push_str(",correction_m");
            }
            out.push('\n');
            for (mjd, tide_level) in mjds.iter().zip(&tides) {
                out.push_str(&format!("{mjd:.6},{tide_level:.4}"));
                if let Some(c) = &correction {
                    out.push_str(&format!(",{:.4}", c.horizontal_shift(*tide_level)));
                }
                out.push('\n');
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;

            let (tmin, tmax) = tides.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), &t| {
                (mn.min(t), mx.max(t))
            });
            eprintln!(
                "{} fechas, {} constituyentes; marea [{:.3}, {:.3}] m -> {}",
                mjds.len(),
                predictor.len(),
                tmin,
                tmax,
                output.display()
            );
        }
        Command::Transects {
            baseline,
            spacing,
            length,
            seaward_e,
            seaward_n,
            output,
        } => {
            let line = read_baseline(&baseline)?;
            anyhow::ensure!(line.len() >= 2, "la línea base necesita ≥2 puntos");
            let transects = generate_transects(&line, spacing, length, (seaward_e, seaward_n));
            write_transects_csv(&transects, &output)?;
            eprintln!(
                "{} puntos de línea base -> {} transectos (spacing {} m, largo {} m) -> {}",
                line.len(),
                transects.len(),
                spacing,
                length,
                output.display()
            );
        }
        Command::Intersect {
            transects,
            shorelines,
            along_dist,
            output,
        } => {
            let ts = read_transects_csv(&transects)?;
            let (sl_ids, sls) = read_shorelines_csv(&shorelines)?;
            anyhow::ensure!(!ts.is_empty(), "no hay transectos");
            anyhow::ensure!(!sls.is_empty(), "no hay shorelines");

            let settings = IntersectionSettings {
                along_dist,
                ..Default::default()
            };
            let t0 = std::time::Instant::now();
            let cross = compute_intersections(&sls, &ts, &settings);
            let elapsed = t0.elapsed().as_secs_f64();

            let mut out = String::from("transect,shoreline_id,chainage_m\n");
            let mut n_valid = 0usize;
            for (ti, per_shore) in cross.iter().enumerate() {
                let tname = transect_name(ti);
                for (si, &ch) in per_shore.iter().enumerate() {
                    if ch.is_finite() {
                        n_valid += 1;
                        out.push_str(&format!("{},{},{:.4}\n", tname, sl_ids[si], ch));
                    }
                }
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            eprintln!(
                "{} transectos × {} shorelines: {} cruces válidos en {:.3} s -> {}",
                ts.len(),
                sls.len(),
                n_valid,
                elapsed,
                output.display()
            );
        }
        Command::Rates {
            intersections,
            dates,
            min_valid,
            despike_max_change,
            despike_min_valid,
            tide_correction,
            qc,
            max_abs_lrr,
            max_sce,
            min_years,
            min_valid_qc,
            min_r2,
            output,
        } => {
            let date_w = read_date_weights(&dates)?;
            let criteria = QcCriteria {
                max_abs_lrr,
                max_sce,
                min_years,
                min_valid: min_valid_qc,
                min_r2,
            };
            let tide_corr = match &tide_correction {
                Some(p) => read_tide_correction(p)?,
                None => std::collections::HashMap::new(),
            };
            // agrupar chainages por transecto, preservando (t_year, chainage, weight)
            let text = std::fs::read_to_string(&intersections)
                .with_context(|| format!("leyendo intersecciones {}", intersections.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            lines.next(); // header: transect,shoreline_id,chainage_m
            let mut by_transect: std::collections::BTreeMap<String, Vec<(f64, f64, f64)>> =
                std::collections::BTreeMap::new();
            for l in lines {
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                if f.len() < 3 {
                    continue;
                }
                let (Some(&(t_year, weight)), Ok(chainage)) =
                    (date_w.get(f[1]), f[2].parse::<f64>())
                else {
                    continue;
                };
                // corrección mareal: corrected = raw − correction (Vos et al.)
                let corrected = chainage - tide_corr.get(f[1]).copied().unwrap_or(0.0);
                by_transect
                    .entry(f[0].to_string())
                    .or_default()
                    .push((t_year, corrected, weight));
            }

            let mut out = String::from(
                "transect,n_valid,years,NSM_m,SCE_m,EPR_m_yr,LRR_m_yr,LRR_r2,LRR_ci95,WLR_m_yr,WLR_ci95",
            );
            if qc {
                out.push_str(",excluded,reason");
            }
            out.push('\n');
            let mut n_out = 0usize;
            let mut n_excluded = 0usize;
            for (tname, mut obs) in by_transect {
                obs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                let t: Vec<f64> = obs.iter().map(|o| o.0).collect();
                let mut y: Vec<f64> = obs.iter().map(|o| o.1).collect();
                let w: Vec<f64> = obs.iter().map(|o| o.2).collect();
                if despike_max_change > 0.0 {
                    y = despike(&y, despike_max_change, despike_min_valid);
                }
                if let Some(r) = compute_rates(&t, &y, Some(&w), min_valid) {
                    n_out += 1;
                    out.push_str(&format!(
                        "{},{},{:.4},{:.4},{:.4},{:.4},{:.6},{:.6},{:.6},{:.6},{:.6}",
                        tname,
                        r.n_valid,
                        r.years,
                        r.nsm,
                        r.sce,
                        r.epr,
                        r.lrr,
                        r.lrr_r2,
                        r.lrr_ci95,
                        r.wlr,
                        r.wlr_ci95
                    ));
                    if qc {
                        match qc::evaluate(&r, &criteria) {
                            Some(reason) => {
                                n_excluded += 1;
                                out.push_str(&format!(",True,{}", reason.label()));
                            }
                            None => out.push_str(",False,"),
                        }
                    }
                    out.push('\n');
                }
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            if qc {
                eprintln!(
                    "{} transectos con tasas, {} excluidos por QC ({} retenidos) -> {}",
                    n_out,
                    n_excluded,
                    n_out - n_excluded,
                    output.display()
                );
            } else {
                eprintln!("{} transectos con tasas -> {}", n_out, output.display());
            }
        }
    }
    Ok(())
}
