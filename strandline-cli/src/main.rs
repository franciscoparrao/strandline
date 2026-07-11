//! CLI de strandline. Subcomandos `extract` (shoreline) y `tide`
//! (predicción y corrección mareal).

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use strandline_core::tide::{self, harmonic::TidePredictor, TideCorrection};
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Extract {
            ms,
            mask,
            green_band,
            swir1_band,
            min_length,
            level,
            refine_radius,
            classify,
            coastsat_compat,
            min_beach_area,
            output,
        } => {
            let params = ExtractParams {
                green_band,
                swir1_band,
                min_length_m: min_length,
                level,
                refine_radius_px: refine_radius,
                use_classifier: classify || coastsat_compat,
                coastsat_compat,
                min_beach_area_m2: min_beach_area,
            };
            let shoreline = extract_shoreline(&ms, mask.as_deref(), &params)
                .with_context(|| format!("extracting shoreline from {}", ms.display()))?;

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
    }
    Ok(())
}
