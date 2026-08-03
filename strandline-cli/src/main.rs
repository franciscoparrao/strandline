//! CLI de strandline. Subcomandos `extract` (shoreline) y `tide`
//! (predicción y corrección mareal).

#[cfg(feature = "cloud")]
mod fetch;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use strandline_core::qc::{self, QcCriteria};
use strandline_core::rates::{compute_rates, decimal_year_from_mjd, despike};
use strandline_core::stl::decompose as stl_decompose;
use strandline_core::tide::{self, harmonic::TidePredictor, TideCorrection};
use strandline_core::transects::{
    compute_intersections, generate_transects, smooth_polyline, transect_name,
    IntersectionSettings, Transect,
};
use strandline_core::cloud::MaskKind;
use strandline_core::shoreline::prepare_scene;
use strandline_core::slope::{estimate_beach_slope, SlopeSettings};
use strandline_core::subpixel::{fit_profiles, refine_line_subpixel, ProfileSettings};
use strandline_core::{extract_shoreline, ExtractParams};

#[derive(Parser)]
#[command(name = "strandline", version, about = "Satellite-derived shoreline analysis engine")]
struct Cli {
    /// Trata como error cualquier fila de CSV descartada al leer (fecha no
    /// parseable, campos faltantes, valor no numérico). Por defecto se
    /// descartan pero SIEMPRE se reportan por stderr con su conteo.
    #[arg(long, global = true)]
    strict: bool,
    #[command(subcommand)]
    command: Command,
}

/// Contabilidad de filas descartadas al leer un CSV.
///
/// La política del motor es no perder observaciones en silencio: un CSV con
/// parte de las fechas en otro formato produciría tasas calculadas sobre un
/// subconjunto, reportadas como si fueran completas. Cada descarte se cuenta
/// por causa y se informa al cerrar la lectura; con `--strict` aborta.
#[derive(Debug)]
struct CsvDiscards {
    source: String,
    /// causa → (conteo, primer valor ofensor)
    by_cause: std::collections::BTreeMap<&'static str, (usize, String)>,
    total_rows: usize,
}

impl CsvDiscards {
    fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            by_cause: std::collections::BTreeMap::new(),
            total_rows: 0,
        }
    }

    fn row(&mut self) {
        self.total_rows += 1;
    }

    fn record(&mut self, cause: &'static str, offending: &str) {
        let e = self
            .by_cause
            .entry(cause)
            .or_insert_with(|| (0, offending.to_string()));
        e.0 += 1;
    }

    fn dropped(&self) -> usize {
        self.by_cause.values().map(|(n, _)| *n).sum()
    }

    /// Informa (o aborta con `strict`). Consume el acumulador para que no
    /// pueda olvidarse una lectura sin reportar.
    fn finish(self, strict: bool) -> anyhow::Result<()> {
        let dropped = self.dropped();
        if dropped == 0 {
            return Ok(());
        }
        let detail = self
            .by_cause
            .iter()
            .map(|(cause, (n, first))| format!("{cause}: {n} (primera: '{first}')"))
            .collect::<Vec<_>>()
            .join("; ");
        let msg = format!(
            "{}: se descartaron {dropped} de {} filas — {detail}",
            self.source, self.total_rows
        );
        if strict {
            anyhow::bail!("{msg} (--strict)");
        }
        eprintln!("aviso: {msg}");
        Ok(())
    }
}

/// Cabecera de CSV: separa por comas, recorta espacios y descarta el BOM
/// UTF-8 que Excel antepone (haría que la primera columna nunca matchee).
fn csv_header(line: &str) -> Vec<String> {
    line.trim_start_matches('\u{feff}')
        .split(',')
        .map(|s| s.trim().to_string())
        .collect()
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
        /// Tipo de banda de máscara: qa (Landsat QA_PIXEL) | scl (Sentinel-2 SCL).
        #[arg(long, default_value = "qa")]
        mask_kind: String,
        /// Refina subpixel el contorno (ajuste de perfil S1 perpendicular a
        /// la línea, sembrado en cada punto del propio contorno) en vez del
        /// pixel-locking crudo de marching squares. Default false: preserva
        /// paridad exacta con CoastSat para comparaciones/validación.
        #[arg(long)]
        subpixel_refine: bool,
        /// EPSG de salida (4326 o UTM WGS84 326xx/327xx). Con 4326 el
        /// GeoJSON es conforme a RFC 7946. Default: CRS nativo de la escena.
        #[arg(long)]
        output_epsg: Option<u32>,
        /// GeoJSON de salida.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Adquiere escenas desde Planetary Computer leyendo SOLO la ventana
    /// del área de interés (COG range requests, sin descargar escenas):
    /// produce pares *_ms.tif + *_mask.tif para extract/profile. Para
    /// landsat-c2-l2, cubre el archivo completo 1984–presente (TM/ETM+/
    /// OLI) — ver --sensors.
    #[cfg(feature = "cloud")]
    Fetch {
        /// Colección STAC: landsat-c2-l2 | sentinel-2-l2a.
        #[arg(long)]
        collection: String,
        /// Área de interés lon/lat: oeste,sur,este,norte.
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        bbox: Vec<f64>,
        /// Rango temporal RFC3339: inicio/fin (ej. 2020-01-01/2020-02-01).
        #[arg(long)]
        datetime: String,
        /// Máximo de escenas a traer.
        #[arg(long, default_value_t = 5)]
        max_items: usize,
        /// Nubosidad máxima de escena (%, eo:cloud_cover).
        #[arg(long, default_value_t = 20.0)]
        max_cloud: f64,
        /// Sensores Landsat a incluir (solo aplica a landsat-c2-l2):
        /// tm (L4/L5), etm (L7), oli (L8/L9), all. Con `all` se accede al
        /// archivo Landsat completo 1984–presente vía STAC/COG streaming,
        /// sin depender de GEE.
        #[arg(long, default_value = "all")]
        sensors: String,
        /// Excluye escenas Landsat-7 ETM+ posteriores a la falla del
        /// Scan Line Corrector (2003-05-31, franjas de datos faltantes).
        /// Por default se incluyen: `extract` ya tolera los huecos (DN 0
        /// → NaN) igual que hace CoastSat con `skip_L7_SLC=False`.
        #[arg(long)]
        skip_l7_slc: bool,
        /// Directorio de salida.
        #[arg(short, long)]
        out_dir: PathBuf,
    },
    /// Extrae la posición subpixel de la línea de agua por ajuste de
    /// perfil sigmoide en cada transecto (S1: menor varianza y sin sesgo
    /// de grilla vs contorno+intersección) con σ por punto (S5a).
    Profile {
        /// GeoTIFF multiespectral de entrada (bandas B,G,R,NIR,SWIR1).
        #[arg(long)]
        ms: PathBuf,
        /// Máscara QA_PIXEL opcional (Landsat Collection 2).
        #[arg(long)]
        mask: Option<PathBuf>,
        /// CSV de transectos (name, origin_e, origin_n, end_e, end_n) en el
        /// CRS nativo de la escena.
        #[arg(long)]
        transects: PathBuf,
        /// Umbral MNDWI fijo (default: mismo modo de umbral que `extract`).
        #[arg(long)]
        level: Option<f32>,
        /// Usa el clasificador de píxeles para el umbral.
        #[arg(long)]
        classify: bool,
        /// Replica el umbral del quirk de CoastSat (implica --classify).
        #[arg(long)]
        coastsat_compat: bool,
        /// Paso de muestreo del perfil (m).
        #[arg(long, default_value_t = 5.0)]
        step: f64,
        /// Semiancho de la ventana de ajuste (m).
        #[arg(long, default_value_t = 60.0)]
        window: f64,
        /// Tipo de banda de máscara: qa (Landsat QA_PIXEL) | scl (Sentinel-2 SCL).
        #[arg(long, default_value = "qa")]
        mask_kind: String,
        /// RMSE de georreferenciación (m) para la σ combinada.
        #[arg(long, default_value_t = 5.0)]
        georef_rmse: f64,
        /// Incertidumbre 1σ del umbral (unidades MNDWI) para la σ combinada.
        #[arg(long, default_value_t = 0.0)]
        threshold_sigma: f64,
        /// CSV de salida (transect, chainage_m, x, y, sigma_fit_m,
        /// sigma_total_m, dx_dthreshold, rmse, n).
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
    /// Elige automáticamente la mejor línea base entre varias escenas
    /// candidatas (p.ej. la salida de `fetch`) y la escribe en el formato
    /// que consume `transects --baseline`. Prioriza continuidad (menos
    /// polilíneas separadas = menos nubes/ruido) por sobre longitud total
    /// — la heurística ingenua "más vértices" elige la escena más
    /// fragmentada, no la más limpia.
    Baseline {
        /// Directorio con pares `*_ms.tif` + `*_mask.tif` (salida de
        /// `fetch` o layout equivalente).
        #[arg(long)]
        dir: PathBuf,
        /// Banda Green (1-based).
        #[arg(long, default_value_t = 2)]
        green_band: usize,
        /// Banda NIR (1-based).
        #[arg(long, default_value_t = 4)]
        nir_band: usize,
        /// Banda SWIR1 (1-based).
        #[arg(long, default_value_t = 5)]
        swir1_band: usize,
        /// Largo mínimo de línea en metros — descarta contornos de ruido
        /// (nubes, artefactos) antes de contar fragmentos por escena; con
        /// 0 la heurística se degrada (una escena casi toda nube puede
        /// "ganar" por tener pocos fragmentos diminutos).
        #[arg(long, default_value_t = 200.0)]
        min_length: f64,
        /// Umbral MNDWI fijo (default: Otsu automático).
        #[arg(long)]
        level: Option<f32>,
        /// Usa el clasificador de píxeles.
        #[arg(long)]
        classify: bool,
        /// Replica el quirk de umbral de CoastSat (implica --classify).
        #[arg(long)]
        coastsat_compat: bool,
        /// Buffer de exclusión alrededor de nubes (m); 0 = off.
        #[arg(long, default_value_t = 300.0)]
        dist_clouds: f64,
        /// Tipo de banda de máscara: qa (Landsat QA_PIXEL) | scl (Sentinel-2 SCL).
        #[arg(long, default_value = "qa")]
        mask_kind: String,
        /// Refina subpixel el contorno de la escena elegida antes de
        /// escribir el CSV (ver `extract --subpixel-refine`). Solo se
        /// aplica una vez, a la ganadora — no a todas las candidatas.
        #[arg(long)]
        subpixel_refine: bool,
        /// CSV de salida (easting, northing) para `transects --baseline`.
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
        /// Ventana de suavizado de la línea base por longitud de arco (m);
        /// 0 = sin suavizar. Recomendado ≥ 300 para líneas base ruidosas
        /// si los transectos alimentarán `profile`.
        #[arg(long, default_value_t = 0.0)]
        smooth_window: f64,
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
    /// Estima la pendiente de playa por transecto desde las propias series
    /// satelitales (S4: minimiza la energía mareal residual, Vos et al. 2020).
    Slope {
        /// CSV de intersecciones (transect, shoreline_id, chainage_m).
        #[arg(long)]
        intersections: PathBuf,
        /// CSV de fechas por shoreline (shoreline_id, date).
        #[arg(long)]
        dates: PathBuf,
        /// CSV de marea por shoreline (shoreline_id, tide_m).
        #[arg(long)]
        tides: PathBuf,
        /// Mínimo de observaciones válidas por transecto.
        #[arg(long, default_value_t = 30)]
        min_valid: usize,
        /// Salto máximo (m) entre observaciones consecutivas antes de aplicar
        /// despiking (mismo algoritmo que `rates`); 0 = desactivado. El
        /// experimento de Viña con S2 mostró que el ruido de intersección
        /// degrada la coherencia del signo estimado (30/75 → 44/75 con
        /// despike previo).
        #[arg(long, default_value_t = 0.0)]
        despike_max_change: f64,
        /// Mínimo de observaciones válidas para aplicar despiking.
        #[arg(long, default_value_t = 30)]
        despike_min_valid: usize,
        /// CSV de salida (transect, slope, peak_freq_cpd, energy_ratio, tide_sign, n).
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
        /// CSV opcional de pesos WLR por satélite (columnas: satname,
        /// weight) — típicamente la salida de `satellite-weights`. Si se
        /// da, `dates` debe tener una columna `satname` además de `date`;
        /// el peso real por satélite reemplaza a la columna `weight` de
        /// `dates` para las observaciones cuyo satélite esté en la tabla
        /// (cierra el hallazgo E1: proxy por fecha de corte -> pesos
        /// medidos).
        #[arg(long)]
        satellite_weights: Option<PathBuf>,
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
    /// Descompone la serie de chainage de cada transecto en tendencia +
    /// estacional + residuo (STL, Cleveland et al. 1990) — validado como
    /// práctica activa a escala por los creadores de CoastSat/CoastSeg
    /// (Warrick, Vos & Buscombe 2025, JGR). Resamplea a grilla mensual
    /// (interpolación lineal en huecos) antes de ajustar, con periodo 12.
    Stl {
        /// CSV de intersecciones (transect, shoreline_id, chainage_m) —
        /// típicamente la salida de `intersect`.
        #[arg(long)]
        intersections: PathBuf,
        /// CSV con fecha por shoreline (columnas: shoreline_id, date).
        #[arg(long)]
        dates: PathBuf,
        /// Mínimo de meses distintos con observación real (no interpolada)
        /// para intentar la descomposición.
        #[arg(long, default_value_t = 12)]
        min_months: usize,
        /// Lapso mínimo cubierto (años) — con periodo 12 hacen falta al
        /// menos ~2 ciclos anuales completos para separar estacional de
        /// tendencia sin degenerar.
        #[arg(long, default_value_t = 2.0)]
        min_span_years: f64,
        /// CSV de salida en formato largo (transect, month, trend_m,
        /// seasonal_m, residual_m, interpolated).
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Calibra pesos WLR reales por satélite desde datos observados
    /// (cierra el hallazgo E1): hace *detrend* OLS de cada transecto,
    /// agrupa los residuos por satélite pooled entre todos los
    /// transectos, y el peso es `1/varianza` — WLR es invariante a la
    /// escala de los pesos, así que no hace falta normalizar. Reemplaza
    /// el proxy histórico de pesos por fecha de corte (válido solo en
    /// sitios Landsat-only con un único salto de sensor) por una medición
    /// real de precisión por sensor.
    SatelliteWeights {
        /// CSV de intersecciones (transect, shoreline_id, chainage_m) —
        /// típicamente la salida de `intersect`.
        #[arg(long)]
        intersections: PathBuf,
        /// CSV con fecha y satélite por shoreline (columnas: shoreline_id,
        /// date, satname).
        #[arg(long)]
        dates: PathBuf,
        /// Mínimo de observaciones para intentar el detrend de un
        /// transecto; los que tienen menos se ignoran.
        #[arg(long, default_value_t = 10)]
        min_valid: usize,
        /// CSV de salida (satname, n_obs, std_m, weight) — usar
        /// directamente como `rates --satellite-weights`.
        #[arg(short, long)]
        output: PathBuf,
    },
}

/// Parsea el tipo de máscara desde el flag CLI.
fn parse_mask_kind(s: &str) -> anyhow::Result<MaskKind> {
    match s {
        "qa" => Ok(MaskKind::QaPixel),
        "scl" => Ok(MaskKind::Scl),
        other => anyhow::bail!("mask-kind inválido: '{other}' (usa qa|scl)"),
    }
}

/// Lee un CSV `shoreline_id, correction_m` a un mapa.
fn read_tide_correction(
    path: &PathBuf,
    strict: bool,
) -> anyhow::Result<std::collections::HashMap<String, f64>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo corrección mareal {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = csv_header(lines.next().unwrap_or_default());
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let ci = header.iter().position(|c| c == "correction_m").context("falta 'correction_m'")?;
    let mut d = CsvDiscards::new(path.display().to_string());
    let mut map = std::collections::HashMap::new();
    for l in lines {
        d.row();
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(ci) {
            d.record("campos faltantes", l);
            continue;
        }
        match f[ci].parse::<f64>() {
            Ok(v) if v.is_finite() => {
                map.insert(f[idi].to_string(), v);
            }
            _ => d.record("corrección no numérica", f[ci]),
        }
    }
    d.finish(strict)?;
    Ok(map)
}

/// Lee `shoreline_id, date, satname` — año decimal + satélite por id.
/// Usado por `satellite-weights` (calibración) y por `rates
/// --satellite-weights` (lookup del satélite de cada observación).
fn read_date_satnames(
    path: &PathBuf,
    strict: bool,
) -> anyhow::Result<std::collections::HashMap<String, (f64, String)>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo fechas/satélite {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = csv_header(lines.next().unwrap_or_default());
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let di = header.iter().position(|c| c == "date").context("falta 'date'")?;
    let si = header.iter().position(|c| c == "satname").context("falta 'satname'")?;

    let mut d = CsvDiscards::new(path.display().to_string());
    let mut map = std::collections::HashMap::new();
    for l in lines {
        d.row();
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(di).max(si) {
            d.record("campos faltantes", l);
            continue;
        }
        let Some(mjd) = tide::parse_timestamp(f[di]) else {
            d.record("fecha no parseable", f[di]);
            continue;
        };
        map.insert(f[idi].to_string(), (decimal_year_from_mjd(mjd), f[si].to_string()));
    }
    d.finish(strict)?;
    Ok(map)
}

/// Lee `satname, weight` (columnas adicionales ignoradas) a un mapa —
/// típicamente la salida de `satellite-weights`.
fn read_satellite_weight_table(
    path: &PathBuf,
    strict: bool,
) -> anyhow::Result<std::collections::HashMap<String, f64>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo tabla de pesos por satélite {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = csv_header(lines.next().unwrap_or_default());
    let si = header.iter().position(|c| c == "satname").context("falta 'satname'")?;
    let wi = header.iter().position(|c| c == "weight").context("falta 'weight'")?;
    let mut d = CsvDiscards::new(path.display().to_string());
    let mut map = std::collections::HashMap::new();
    for l in lines {
        d.row();
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= si.max(wi) {
            d.record("campos faltantes", l);
            continue;
        }
        // un peso 0/negativo/no finito rompe la regresión ponderada
        match f[wi].parse::<f64>() {
            Ok(w) if w.is_finite() && w > 0.0 => {
                map.insert(f[si].to_string(), w);
            }
            _ => d.record("peso inválido (debe ser finito y > 0)", f[wi]),
        }
    }
    d.finish(strict)?;
    Ok(map)
}

/// Fecha (año decimal) y peso de una shoreline, indexado por `shoreline_id`.
type DateWeights = std::collections::HashMap<String, (f64, f64)>;

/// Lee `shoreline_id, date[, weight]` y devuelve año decimal + peso por id.
fn read_date_weights(path: &PathBuf, strict: bool) -> anyhow::Result<DateWeights> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo fechas {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = csv_header(lines.next().unwrap_or_default());
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let di = header.iter().position(|c| c == "date").context("falta 'date'")?;
    let wi = header.iter().position(|c| c == "weight");

    let mut d = CsvDiscards::new(path.display().to_string());
    let mut map = DateWeights::new();
    for l in lines {
        d.row();
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(di) {
            d.record("campos faltantes", l);
            continue;
        }
        let Some(mjd) = tide::parse_timestamp(f[di]) else {
            d.record("fecha no parseable", f[di]);
            continue;
        };
        // columna ausente → peso 1.0 legítimo; columna presente pero
        // ilegible/no positiva → se reporta, no se asume 1.0 en silencio
        let weight = match wi.and_then(|i| f.get(i)) {
            None => 1.0,
            Some(s) => match s.parse::<f64>() {
                Ok(w) if w.is_finite() && w > 0.0 => w,
                _ => {
                    d.record("peso inválido (se usa 1.0)", s);
                    1.0
                }
            },
        };
        map.insert(f[idi].to_string(), (decimal_year_from_mjd(mjd), weight));
    }
    d.finish(strict)?;
    Ok(map)
}

/// Lee transectos desde el CSV que escribe `write_transects_csv`, devolviendo
/// también el nombre de cada uno.
///
/// Las columnas se resuelven **por nombre** como en el resto de los readers:
/// con acceso posicional, un CSV con columnas reordenadas producía transectos
/// con coordenadas intercambiadas. Y se preserva `name` porque los
/// consumidores renumeran `T001…` por posición: sobre un CSV editado (una
/// fila borrada) eso corre todos los nombres siguientes y atribuye las tasas
/// al transecto equivocado.
fn read_transects_csv(
    path: &PathBuf,
    strict: bool,
) -> anyhow::Result<(Vec<Transect>, Vec<String>)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo transectos {}", path.display()))?;
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());
    let header = csv_header(lines.next().map(|(_, l)| l).unwrap_or_default());
    let col = |name: &str| {
        header
            .iter()
            .position(|c| c == name)
            .with_context(|| format!("falta columna '{name}' en {}", path.display()))
    };
    let (ni, oe, on, ee, en) = (
        col("name")?,
        col("origin_e")?,
        col("origin_n")?,
        col("end_e")?,
        col("end_n")?,
    );
    let need = ni.max(oe).max(on).max(ee).max(en);

    let mut d = CsvDiscards::new(path.display().to_string());
    let mut out = Vec::new();
    let mut names = Vec::new();
    for (lineno, l) in lines {
        d.row();
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= need {
            d.record("campos faltantes", l);
            continue;
        }
        let num = |i: usize| -> anyhow::Result<f64> {
            f[i].parse::<f64>().with_context(|| {
                format!(
                    "valor no numérico en {} línea {}: '{}'",
                    path.display(),
                    lineno + 1,
                    f[i]
                )
            })
        };
        out.push(Transect {
            origin: (num(oe)?, num(on)?),
            end: (num(ee)?, num(en)?),
        });
        names.push(f[ni].to_string());
    }
    d.finish(strict)?;
    Ok((out, names))
}

/// Shorelines leídas: los ids (en orden de aparición) y los puntos de cada
/// una.
type Shorelines = (Vec<String>, Vec<Vec<(f64, f64)>>);

/// Lee shorelines agrupadas por `shoreline_id` (preserva el orden de
/// aparición de los ids y de los puntos dentro de cada uno).
fn read_shorelines_csv(path: &PathBuf, strict: bool) -> anyhow::Result<Shorelines> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo shorelines {}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = csv_header(lines.next().unwrap_or_default());
    let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
    let ei = header.iter().position(|c| c == "easting").context("falta 'easting'")?;
    let ni = header.iter().position(|c| c == "northing").context("falta 'northing'")?;

    let mut d = CsvDiscards::new(path.display().to_string());
    let mut ids: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut last_id: Option<String> = None;
    // los fragmentos se agrupan por corridas CONTIGUAS del mismo id: si el
    // CSV se reordena (Excel, `sort`), un id que reaparece tras haberse
    // cerrado indicaría fusión de fragmentos distantes con segmentos
    // fantasma que generan intersecciones espurias — se avisa
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reopened = 0usize;
    for l in lines {
        d.row();
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if f.len() <= idi.max(ei).max(ni) {
            d.record("campos faltantes", l);
            continue;
        }
        let id = f[idi].to_string();
        if last_id.as_ref() != Some(&id) {
            if !seen_ids.insert(id.clone()) {
                reopened += 1;
            }
            ids.push(id.clone());
            groups.push(Vec::new());
            last_id = Some(id);
        }
        match (f[ei].parse::<f64>(), f[ni].parse::<f64>()) {
            (Ok(e), Ok(n)) if e.is_finite() && n.is_finite() => {
                groups.last_mut().unwrap().push((e, n));
            }
            _ => d.record("coordenada no numérica", l),
        }
    }
    if reopened > 0 {
        eprintln!(
            "aviso: {} shoreline_id reaparecen tras cerrarse en {} — el CSV parece reordenado; \
             los fragmentos de una misma escena deben ir contiguos o se cruzarán segmentos inexistentes",
            reopened,
            path.display()
        );
    }
    d.finish(strict)?;
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
            mask_kind,
            subpixel_refine,
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
                mask_kind: parse_mask_kind(&mask_kind)?,
                subpixel_refine,
                subpixel_spacing_m: 15.0,
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
        #[cfg(feature = "cloud")]
        Command::Fetch {
            collection,
            bbox,
            datetime,
            max_items,
            max_cloud,
            sensors,
            skip_l7_slc,
            out_dir,
        } => {
            anyhow::ensure!(bbox.len() == 4, "bbox necesita 4 valores: oeste,sur,este,norte");
            let n = fetch::run(
                &collection,
                [bbox[0], bbox[1], bbox[2], bbox[3]],
                &datetime,
                max_items,
                max_cloud,
                &sensors,
                skip_l7_slc,
                &out_dir,
            )?;
            eprintln!("{n} escenas escritas en {}", out_dir.display());
        }
        Command::Profile {
            ms,
            mask,
            transects,
            level,
            classify,
            coastsat_compat,
            step,
            window,
            mask_kind,
            georef_rmse,
            threshold_sigma,
            output,
        } => {
            let params = ExtractParams {
                level,
                use_classifier: classify || coastsat_compat,
                coastsat_compat,
                mask_kind: parse_mask_kind(&mask_kind)?,
                ..Default::default()
            };
            let scene = prepare_scene(&ms, mask.as_deref(), &params)
                .with_context(|| format!("preparando escena {}", ms.display()))?;
            let (ts, ts_names) = read_transects_csv(&transects, cli.strict)?;
            anyhow::ensure!(!ts.is_empty(), "no hay transectos");

            // Semilla por transecto: el cruce del contorno Otsu/clasificador
            // (mismo `params`, así que mismo nivel que el ajuste subpixel)
            // con cada transecto. El contorno ve la escena completa en 2D,
            // así que no engancha features espurias entre el origen y la
            // costa real (estacionamientos, sombras, agua interrumpida) como
            // sí puede hacerlo el buscador ingenuo 1D — ver hallazgo de la
            // adenda profile_sr (Viña: perfil sin semilla erraba 150 m de
            // mediana por esto). Cuando el contorno no cruza un transecto
            // (QC excluido, fuera del footprint de la escena), ese
            // transecto cae al buscador ingenuo dentro de `fit_profiles`.
            let contour_shoreline = extract_shoreline(&ms, mask.as_deref(), &params)
                .with_context(|| format!("contorneando para semillas {}", ms.display()))?;
            let combined_line: Vec<(f64, f64)> =
                contour_shoreline.lines.into_iter().flatten().collect();
            let seed_raw = compute_intersections(&[combined_line], &ts, &IntersectionSettings::default());
            let seeds: Vec<Option<f64>> = seed_raw
                .iter()
                .map(|v| v.first().copied().filter(|x| x.is_finite()))
                .collect();

            let settings = ProfileSettings {
                step_m: step,
                window_m: window,
                ..Default::default()
            };
            let t0 = std::time::Instant::now();
            let fits = fit_profiles(&scene.mndwi, &ts, scene.level_textbook as f64, &seeds, &settings);
            let elapsed = t0.elapsed().as_secs_f64();

            let mut out = String::from(
                "transect,chainage_m,x,y,sigma_fit_m,sigma_total_m,dx_dthreshold,rmse,n\n",
            );
            let mut n_ok = 0usize;
            for (i, fit) in fits.iter().enumerate() {
                if let Some(f) = fit {
                    n_ok += 1;
                    // S5a: σ² = σ_fit² + σ_georef² + (∂x/∂t · σ_t)²
                    let sigma_total = (f.sigma_m.powi(2)
                        + georef_rmse.powi(2)
                        + (f.dx_dthreshold * threshold_sigma).powi(2))
                    .sqrt();
                    out.push_str(&format!(
                        "{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.6},{}\n",
                        ts_names
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| transect_name(i)),
                        f.chainage_m,
                        f.world.0,
                        f.world.1,
                        f.sigma_m,
                        sigma_total,
                        f.dx_dthreshold,
                        f.rmse,
                        f.n_samples
                    ));
                }
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            eprintln!(
                "{}/{} transectos con cruce (umbral = {:.4}, nivel textbook = {:.4}) en {:.3} s -> {}",
                n_ok,
                ts.len(),
                scene.raw_threshold,
                scene.level_textbook,
                elapsed,
                output.display()
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
            // se conserva el timestamp original junto al MJD: si alguna fila
            // se descarta, un join posicional contra el CSV de entrada
            // quedaría corrido en silencio
            let mut stamps: Vec<String> = Vec::new();
            let mut mjds: Vec<f64> = Vec::new();
            let mut dropped = 0usize;
            let mut first_bad: Option<String> = None;
            for l in lines {
                let raw = l.split(',').nth(ci).map(str::trim).unwrap_or("");
                match tide::parse_timestamp(raw) {
                    Some(m) => {
                        stamps.push(raw.to_string());
                        mjds.push(m);
                    }
                    None => {
                        dropped += 1;
                        first_bad.get_or_insert_with(|| raw.to_string());
                    }
                }
            }
            if dropped > 0 {
                eprintln!(
                    "aviso: {dropped} fila(s) descartadas por timestamp no parseable en {} (primera: '{}')",
                    dates.display(),
                    first_bad.as_deref().unwrap_or("")
                );
            }
            anyhow::ensure!(!mjds.is_empty(), "no se parseó ninguna fecha");

            let tides = predictor.predict_many(&mjds);
            let correction = beach_slope
                .map(|slope| TideCorrection::new(slope, reference_elevation))
                .transpose()?;

            let mut out = String::from("timestamp,mjd,tide_m");
            if correction.is_some() {
                out.push_str(",correction_m");
            }
            out.push('\n');
            for ((stamp, mjd), tide_level) in stamps.iter().zip(&mjds).zip(&tides) {
                out.push_str(&format!("{stamp},{mjd:.6},{tide_level:.4}"));
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
        Command::Baseline {
            dir,
            green_band,
            nir_band,
            swir1_band,
            min_length,
            level,
            classify,
            coastsat_compat,
            dist_clouds,
            mask_kind,
            subpixel_refine,
            output,
        } => {
            let params = ExtractParams {
                green_band,
                nir_band,
                swir1_band,
                min_length_m: min_length,
                level,
                refine_radius_px: 10,
                use_classifier: classify || coastsat_compat,
                coastsat_compat,
                min_beach_area_m2: 500.0,
                dist_clouds_m: dist_clouds,
                cloud_min_size_px: 40,
                mask_kind: parse_mask_kind(&mask_kind)?,
                subpixel_refine: false, // el contorno crudo decide la selección; se refina solo la ganadora
                subpixel_spacing_m: 15.0,
            };

            let mut ms_paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .with_context(|| format!("leyendo directorio {}", dir.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_ms.tif")))
                .collect();
            ms_paths.sort();
            anyhow::ensure!(!ms_paths.is_empty(), "sin escenas *_ms.tif en {}", dir.display());

            let mut candidates = Vec::new();
            let mut paths_by_label: std::collections::HashMap<String, (PathBuf, Option<PathBuf>)> =
                std::collections::HashMap::new();
            for ms in &ms_paths {
                let stem = ms.file_name().unwrap().to_string_lossy();
                let label = stem.strip_suffix("_ms.tif").unwrap_or(&stem).to_string();
                let mask = ms.with_file_name(format!("{label}_mask.tif"));
                let mask_opt = mask.exists().then_some(mask.as_path());
                match extract_shoreline(ms, mask_opt, &params) {
                    Ok(shoreline) => {
                        if let Some(cand) = strandline_core::baseline::evaluate_candidate(
                            label.clone(),
                            &shoreline.lines,
                            min_length,
                        ) {
                            eprintln!(
                                "  {label}: {} fragmentos, {:.0} m el más largo",
                                cand.n_fragments, cand.arc_length_m
                            );
                            paths_by_label.insert(label.clone(), (ms.clone(), mask_opt.map(PathBuf::from)));
                            candidates.push(cand);
                        } else {
                            eprintln!("  {label}: sin líneas extraídas, omitida");
                        }
                    }
                    Err(e) => eprintln!("  {label}: extract falló ({e:#}), omitida"),
                }
            }
            anyhow::ensure!(!candidates.is_empty(), "ninguna escena produjo una shoreline utilizable");

            let mut chosen = strandline_core::baseline::select_baseline(candidates)
                .context("selección de línea base falló inesperadamente")?;
            if subpixel_refine {
                let (ms, mask_opt) = paths_by_label
                    .get(&chosen.label)
                    .expect("la escena ganadora debe estar en paths_by_label");
                let scene = prepare_scene(ms, mask_opt.as_deref(), &params)
                    .with_context(|| format!("preparando {} para refinamiento subpixel", ms.display()))?;
                chosen.line = refine_line_subpixel(
                    &scene.mndwi,
                    &chosen.line,
                    scene.level_textbook as f64,
                    params.subpixel_spacing_m,
                    &ProfileSettings::default(),
                );
            }
            strandline_core::baseline::write_baseline_csv(&chosen.line, &output)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            eprintln!(
                "elegida: {} ({} fragmentos, {:.0} m{}) -> {}",
                chosen.label,
                chosen.n_fragments,
                chosen.arc_length_m,
                if subpixel_refine { ", refinada subpixel" } else { "" },
                output.display()
            );
        }
        Command::Transects {
            baseline,
            spacing,
            length,
            seaward_e,
            seaward_n,
            smooth_window,
            output,
        } => {
            let mut line = read_baseline(&baseline)?;
            anyhow::ensure!(line.len() >= 2, "la línea base necesita ≥2 puntos");
            if smooth_window > 0.0 {
                line = smooth_polyline(&line, smooth_window);
            }
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
            let (ts, ts_names) = read_transects_csv(&transects, cli.strict)?;
            let (sl_ids, sls) = read_shorelines_csv(&shorelines, cli.strict)?;
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
                // se usa el nombre que trae el CSV de transectos; renumerar
                // por posición corre las etiquetas si el CSV fue editado
                let tname = ts_names
                    .get(ti)
                    .cloned()
                    .unwrap_or_else(|| transect_name(ti));
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
        Command::Slope {
            intersections,
            dates,
            tides,
            min_valid,
            despike_max_change,
            despike_min_valid,
            output,
        } => {
            // fechas (MJD) por shoreline
            let date_w = read_date_weights(&dates, cli.strict)?;
            // marea por shoreline (reusa el lector de corrección: col tide_m)
            let text = std::fs::read_to_string(&tides)
                .with_context(|| format!("leyendo mareas {}", tides.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            let header = csv_header(lines.next().unwrap_or_default());
            let idi = header.iter().position(|c| c == "shoreline_id").context("falta 'shoreline_id'")?;
            let ti = header.iter().position(|c| c == "tide_m").context("falta 'tide_m'")?;
            let mut dt = CsvDiscards::new(tides.display().to_string());
            let mut tide_by_id = std::collections::HashMap::new();
            for l in lines {
                dt.row();
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                if f.len() <= idi.max(ti) {
                    dt.record("campos faltantes", l);
                    continue;
                }
                match f[ti].parse::<f64>() {
                    Ok(v) if v.is_finite() => {
                        tide_by_id.insert(f[idi].to_string(), v);
                    }
                    _ => dt.record("marea no numérica", f[ti]),
                }
            }
            dt.finish(cli.strict)?;

            // NOTA: read_date_weights devuelve años decimales; para el
            // análisis espectral se necesitan DÍAS → año·365.2425 sirve
            // como eje temporal consistente (unidades de días).
            let text = std::fs::read_to_string(&intersections)
                .with_context(|| format!("leyendo intersecciones {}", intersections.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            lines.next();
            let mut by_transect: std::collections::BTreeMap<String, Vec<(f64, f64, f64)>> =
                std::collections::BTreeMap::new();
            let mut d = CsvDiscards::new(intersections.display().to_string());
            for l in lines {
                d.row();
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                if f.len() < 3 {
                    d.record("campos faltantes", l);
                    continue;
                }
                let Some(&(t_year, _)) = date_w.get(f[1]) else {
                    d.record("shoreline_id sin fecha en --dates", f[1]);
                    continue;
                };
                let Some(&tide) = tide_by_id.get(f[1]) else {
                    d.record("shoreline_id sin marea en --tides", f[1]);
                    continue;
                };
                let Ok(ch) = f[2].parse::<f64>() else {
                    d.record("chainage no numérico", f[2]);
                    continue;
                };
                by_transect
                    .entry(f[0].to_string())
                    .or_default()
                    .push((t_year * 365.2425, ch, tide));
            }
            d.finish(cli.strict)?;

            let settings = SlopeSettings {
                min_valid,
                ..Default::default()
            };
            let mut out =
                String::from("transect,slope,peak_freq_cpd,energy_ratio,tide_sign,at_boundary,n\n");
            let mut n_ok = 0usize;
            let mut n_boundary = 0usize;
            let mut slopes = Vec::new();
            for (tname, mut obs) in by_transect {
                obs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                let t: Vec<f64> = obs.iter().map(|o| o.0).collect();
                let mut x: Vec<f64> = obs.iter().map(|o| o.1).collect();
                let h: Vec<f64> = obs.iter().map(|o| o.2).collect();
                if despike_max_change > 0.0 {
                    x = despike(&x, despike_max_change, despike_min_valid);
                }
                if let Some(est) = estimate_beach_slope(&t, &x, &h, &settings) {
                    n_ok += 1;
                    // los óptimos en el borde de la grilla no son estimaciones
                    // utilizables: se escriben marcados, pero no entran a la
                    // mediana que resume el sitio
                    if !est.at_boundary {
                        slopes.push(est.slope);
                    } else {
                        n_boundary += 1;
                    }
                    out.push_str(&format!(
                        "{},{:.4},{:.6},{:.4},{:+.0},{},{}\n",
                        tname,
                        est.slope,
                        est.peak_freq_cpd,
                        est.energy_ratio,
                        est.tide_sign,
                        est.at_boundary,
                        est.n
                    ));
                }
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            slopes.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let med = slopes.get(slopes.len() / 2).copied().unwrap_or(f64::NAN);
            eprintln!(
                "{} transectos con slope estimado (mediana tan(β) = {:.3} sobre {} interiores) -> {}",
                n_ok,
                med,
                slopes.len(),
                output.display()
            );
            if n_boundary > 0 {
                eprintln!(
                    "aviso: {n_boundary} transectos con el óptimo en el borde de la grilla \
                     (marcados at_boundary=true): señal mareal débil o transecto mal orientado, \
                     excluidos de la mediana"
                );
            }
        }
        Command::Rates {
            intersections,
            dates,
            min_valid,
            despike_max_change,
            despike_min_valid,
            tide_correction,
            satellite_weights,
            qc,
            max_abs_lrr,
            max_sce,
            min_years,
            min_valid_qc,
            min_r2,
            output,
        } => {
            let date_w = read_date_weights(&dates, cli.strict)?;
            let criteria = QcCriteria {
                max_abs_lrr,
                max_sce,
                min_years,
                min_valid: min_valid_qc,
                min_r2,
            };
            let tide_corr = match &tide_correction {
                Some(p) => read_tide_correction(p, cli.strict)?,
                None => std::collections::HashMap::new(),
            };
            // pesos reales por satélite (hallazgo E1): reemplazan la
            // columna `weight` de `dates` cuando el satélite de la
            // observación está en la tabla; si no, cae al peso de `dates`
            // (o 1.0 por default) sin romper el flujo existente.
            let (sat_by_shoreline, sat_weight_table) = match &satellite_weights {
                Some(p) => (read_date_satnames(&dates, cli.strict)?, read_satellite_weight_table(p, cli.strict)?),
                None => (std::collections::HashMap::new(), std::collections::HashMap::new()),
            };
            // agrupar chainages por transecto, preservando (t_year, chainage, weight)
            let text = std::fs::read_to_string(&intersections)
                .with_context(|| format!("leyendo intersecciones {}", intersections.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            lines.next(); // header: transect,shoreline_id,chainage_m
            let mut by_transect: std::collections::BTreeMap<String, Vec<(f64, f64, f64)>> =
                std::collections::BTreeMap::new();
            let mut d = CsvDiscards::new(intersections.display().to_string());
            let mut uncorrected = 0usize;
            for l in lines {
                d.row();
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                if f.len() < 3 {
                    d.record("campos faltantes", l);
                    continue;
                }
                let Some(&(t_year, weight)) = date_w.get(f[1]) else {
                    d.record("shoreline_id sin fecha en --dates", f[1]);
                    continue;
                };
                let Ok(chainage) = f[2].parse::<f64>() else {
                    d.record("chainage no numérico", f[2]);
                    continue;
                };
                let weight = sat_by_shoreline
                    .get(f[1])
                    .and_then(|(_, sat)| sat_weight_table.get(sat))
                    .copied()
                    .unwrap_or(weight);
                // corrección mareal: corrected = raw − correction (Vos et al.).
                // Una observación sin entrada en la tabla entraría SIN corregir
                // a la misma serie que las corregidas: en un sitio macro-tidal
                // eso son metros de sesgo indistinguibles del cambio real.
                let corrected = if tide_correction.is_some() {
                    match tide_corr.get(f[1]) {
                        Some(c) => chainage - c,
                        None => {
                            uncorrected += 1;
                            continue;
                        }
                    }
                } else {
                    chainage
                };
                by_transect
                    .entry(f[0].to_string())
                    .or_default()
                    .push((t_year, corrected, weight));
            }
            if uncorrected > 0 {
                let msg = format!(
                    "{uncorrected} observaciones sin entrada en la tabla de corrección mareal: se excluyeron \
                     (mezclarlas sin corregir sesga la serie)"
                );
                if cli.strict {
                    anyhow::bail!("{msg} (--strict)");
                }
                eprintln!("aviso: {msg}");
            }
            d.finish(cli.strict)?;

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
        Command::Stl {
            intersections,
            dates,
            min_months,
            min_span_years,
            output,
        } => {
            let date_w = read_date_weights(&dates, cli.strict)?;
            let text = std::fs::read_to_string(&intersections)
                .with_context(|| format!("leyendo intersecciones {}", intersections.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            lines.next(); // header: transect,shoreline_id,chainage_m
            let mut by_transect: std::collections::BTreeMap<String, Vec<(f64, f64)>> =
                std::collections::BTreeMap::new();
            let mut d = CsvDiscards::new(intersections.display().to_string());
            for l in lines {
                d.row();
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                if f.len() < 3 {
                    d.record("campos faltantes", l);
                    continue;
                }
                let Some(&(t_year, _weight)) = date_w.get(f[1]) else {
                    d.record("shoreline_id sin fecha en --dates", f[1]);
                    continue;
                };
                let Ok(chainage) = f[2].parse::<f64>() else {
                    d.record("chainage no numérico", f[2]);
                    continue;
                };
                by_transect.entry(f[0].to_string()).or_default().push((t_year, chainage));
            }
            d.finish(cli.strict)?;

            let mut out =
                // `year,month` explícitos además del año decimal: el join
                // contra series mensuales externas (ONI, oleaje) debe hacerse
                // por mes calendario, no por un flotante
                String::from(
                    "transect,year,month,decimal_year,trend_m,seasonal_m,residual_m,interpolated\n",
                );
            let mut n_ok = 0usize;
            let mut amplitudes = Vec::new();
            for (tname, mut obs) in by_transect {
                obs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                let t: Vec<f64> = obs.iter().map(|o| o.0).collect();
                let y: Vec<f64> = obs.iter().map(|o| o.1).collect();
                let Some(r) = stl_decompose(&t, &y, min_months, min_span_years) else {
                    continue;
                };
                n_ok += 1;
                amplitudes.push(r.seasonal_amplitude_m);
                for i in 0..r.months.len() {
                    let (yy, mm) = r.year_month[i];
                    out.push_str(&format!(
                        "{},{},{:02},{:.4},{:.4},{:.4},{:.4},{}\n",
                        tname,
                        yy,
                        mm,
                        r.months[i],
                        r.trend[i],
                        r.seasonal[i],
                        r.residual[i],
                        r.interpolated[i]
                    ));
                }
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            let median_amplitude = if amplitudes.is_empty() {
                0.0
            } else {
                amplitudes.sort_by(|a, b| a.partial_cmp(b).unwrap());
                amplitudes[amplitudes.len() / 2]
            };
            eprintln!(
                "{n_ok} transectos descompuestos, amplitud estacional mediana = {median_amplitude:.2} m -> {}",
                output.display()
            );
        }
        Command::SatelliteWeights {
            intersections,
            dates,
            min_valid,
            output,
        } => {
            let date_sat = read_date_satnames(&dates, cli.strict)?;
            let text = std::fs::read_to_string(&intersections)
                .with_context(|| format!("leyendo intersecciones {}", intersections.display()))?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            lines.next(); // header: transect,shoreline_id,chainage_m
            let mut by_transect: std::collections::HashMap<String, Vec<(f64, f64, String)>> =
                std::collections::HashMap::new();
            let mut d = CsvDiscards::new(intersections.display().to_string());
            for l in lines {
                d.row();
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                if f.len() < 3 {
                    d.record("campos faltantes", l);
                    continue;
                }
                let Some((t_year, sat)) = date_sat.get(f[1]) else {
                    d.record("shoreline_id sin fecha/satélite en --dates", f[1]);
                    continue;
                };
                let Ok(chainage) = f[2].parse::<f64>() else {
                    d.record("chainage no numérico", f[2]);
                    continue;
                };
                // el CSV de `intersect` marca con NaN lo que rechaza el QC;
                // `calibrate` las filtra, pero se contabilizan para que el
                // usuario sepa sobre cuántas observaciones se calibró
                if !chainage.is_finite() {
                    d.record("chainage no finito (QC)", f[2]);
                    continue;
                }
                by_transect
                    .entry(f[0].to_string())
                    .or_default()
                    .push((*t_year, chainage, sat.clone()));
            }
            d.finish(cli.strict)?;

            let weights = strandline_core::calibrate_satellite_weights(&by_transect, min_valid);
            anyhow::ensure!(
                !weights.is_empty(),
                "ningún satélite tuvo suficientes observaciones para calibrar (revisa --min-valid o los datos de entrada)"
            );
            let mut out = String::from("satname,n_obs,std_m,weight\n");
            for w in &weights {
                out.push_str(&format!("{},{},{:.4},{:.6}\n", w.satname, w.n_obs, w.std_m, w.weight));
            }
            std::fs::write(&output, out)
                .with_context(|| format!("escribiendo {}", output.display()))?;
            eprintln!("pesos calibrados para {} satélites -> {}", weights.len(), output.display());
            for w in &weights {
                eprintln!("  {}: n={} std={:.2}m peso={:.4}", w.satname, w.n_obs, w.std_m, w.weight);
            }
        }
    }
    Ok(())
}
