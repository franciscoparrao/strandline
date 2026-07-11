//! CLI de strandline. Spike V1: subcomando `extract`.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
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
    }
    Ok(())
}
