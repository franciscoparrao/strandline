# Strandline — Engine Rust de análisis de shorelines satelitales

## Qué es

Engine compilado de extracción y análisis de líneas de costa desde imágenes
satelitales. El "CoastSat/DSAS en Rust" de la familia de engines del autor
(SurtGIS, Smelt, Forge, Nowcast, Hydroflux, Anvil, Cantus, Criterium).

**Nombre**: `strandline` ("strandline" = línea de costa en inglés antiguo).
Verificado disponible en crates.io (2026-07-10; `strand` y `swash` tomados).

**Estado**: V1 COMPLETO (2026-07-11). Los 9 items del scope implementados y
validados end-to-end vs CoastSat/pyTMD/el pickle del paper. Ver "Estado del
V1" al final. Próximo: empaquetar (crates.io/Zenodo) + paper C&G.

## Motivación (pain-points vividos, no teóricos)

Documentados durante el paper de erosión costera de Chile
(`~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/`, submitted a
Geomorphology 2026-05-04):

1. **Lentitud**: `compute_intersection_QC` de CoastSat (loop Python) tomaba
   20+ min para 179 transectos × ~800 shorelines en Viña. La reimplementación
   vectorizada propia (`scripts/02_transects_rates.py`) corre en segundos.
   → Ese speedup es el germen del benchmark del paper.
2. **Dependency hell**: pyfes solo existe en conda-forge (hubo que migrar a
   pyTMD + GOT5.5); GDAL bindings frágiles; parche manual a
   `SDS_shoreline.py` (líneas 804-807, backend Agg vs showMaximized).
3. **No escala**: procesar 1000+ km de costa × 40 años requiere días en el
   pipeline Python. Un engine compilado con paralelismo (rayon) lo baja a
   minutos.

## Posicionamiento

- **Competidores**: CoastSat (kvos, Python, el estándar), CoastSeg (USGS,
  Python, wrapper de zoo/doodleverse). Ningún challenger compilado existe.
- **No competimos en descarga**: input = GeoTIFFs/COGs ya bajados (GEE,
  Planetary Computer, USGS). Strandline procesa, no descarga (scope V1).
- **Diferenciadores**: binario único sin dependencias, velocidad (benchmark
  vs CoastSat en las 6 zonas de Chile), corrección mareal integrada,
  intersección de transectos vectorizada nativa, tasas DSAS completas.

## Scope V1 (congelado — resistir scope creep)

Pipeline: GeoTIFF multiespectral → shoreline georeferenciada → tasas por transecto.

1. **I/O raster**: leer GeoTIFF/COG multibanda (reusar `surtgis-core`).
2. **Índice de agua**: MNDWI = (Green − SWIR1) / (Green + SWIR1).
3. **Clasificación de píxeles** (sand/water/whitewater/other): MLP pequeño o
   RF — reusar `smelt-ml`. CoastSat usa un MLP scikit-learn; sus pesos son
   portables como referencia.
4. **Umbral**: Otsu sobre MNDWI restringido a la zona sand+water clasificada.
5. **Extracción subpixel**: marching squares sobre el contorno del umbral
   (equivalente a `skimage.measure.find_contours`).
6. **Corrección mareal**: `z_corr = z + (tide − ref) / tan(beach_slope)`;
   niveles de marea desde CSV externo (SHOA) o constituyentes armónicos
   (evaluar port mínimo de GOT — NO reimplementar pyTMD completo en V1).
7. **Transectos**: generación automática perpendicular a línea base suavizada
   + intersección vectorizada shoreline×transecto.
8. **Tasas**: EPR, LRR, WLR, NSM, SCE con incertidumbre (σ por punto:
   georef + pixel + tidal).
9. **Filtros QC**: los 5 criterios calibrados en el paper (|LRR|, SCE, span,
   n, R²) como flags configurables.

**Fuera de V1**: descarga de imágenes, pansharpening, corrección de wave
setup, forecasting, GUI.

## Arquitectura propuesta

```
strandline/
├── strandline-core/     # lib: extracción, marea, transectos, tasas
├── strandline-cli/      # binario: subcomandos extract / transects / rates / qc
└── strandline-py/       # (V2) bindings PyO3 para interop con ecosistema CoastSat
```

Reuso del ecosistema propio:
- `surtgis-core` → I/O GeoTIFF, CRS/reproyección, raster ops
- `smelt-ml` → clasificador de píxeles + (V2) conformal prediction para incertidumbre
- `forge-core` (metalforge) → (V2) calibración de umbrales/parámetros
- Patrón de publicación conocido: crates.io + mdBook + Zenodo DOI (skill `/zenodo`)

## Spike de viabilidad (semana 1) — CRITERIO GO/NO-GO

**Objetivo**: MNDWI + Otsu + marching squares subpixel en Rust sobre escenas
Landsat 8 de Viña del Mar, comparado contra la shoreline CoastSat del mismo día.

**Criterio GO**: diferencia mediana < 5 m vs CoastSat (comparación
engine-vs-engine, no exactitud absoluta; el benchmark Vos 2023 reporta
~7–10 m de exactitud absoluta para el método).

**Pasos**:
1. Re-descargar 3–5 escenas L8 de Viña vía el pipeline existente
   (`propuesta-4-erosion-costera/scripts/01_pilot_vina.py` — el entorno GEE
   ya está autenticado allá, project ID 1052232428380). Las imágenes del
   piloto original NO se retuvieron en disco; los outputs procesados
   (shorelines de referencia) están en `propuesta-4-erosion-costera/output/`.
2. Guardar las shorelines CoastSat de esas fechas como ground truth GeoJSON.
3. `cargo new` + prototipo: leer GeoTIFF (surtgis-core), MNDWI, Otsu,
   marching squares con interpolación lineal subpixel.
4. Métrica: distancia media/mediana punto-a-línea entre ambas shorelines.
5. Reportar tiempo de ejecución vs CoastSat para las mismas escenas.

## Paper asociado (Opportunity #3 del análisis de gaps 2026-07-10)

- **Venue**: Computers & Geosciences (gap de venue: ~0 papers costeros
  recientes, pero publica exactamente el género software+benchmark+caso).
  Alternativas: EMS, ESIN. Reporte completo:
  `~/vault/journals/_opportunities/2026-07-10_15-57_line-erosion-costera.md`
- **Hook**: "CoastSat democratizó las shorelines satelitales pero no escala:
  engine compilado que procesa 40 años × 1000 km de costa en minutos, sin
  dependency hell, con corrección mareal integrada."
- **Benchmark plan**: exactitud (vs CoastSat, mismas escenas, 6 sitios de
  Chile) + velocidad (single-thread y multi-thread) + memoria + líneas de
  instalación (cargo install vs conda env). Caso de uso: re-procesar el
  dataset nacional del paper de Geomorphology.
- **Sin reloj de preprints**: no existe challenger compilado (verificado
  OpenAlex 2026-07-10) — se puede construir con calma, a diferencia de los
  gaps de forecasting.

## Referencias técnicas clave

- Vos et al. 2019, *CoastSat: A Google Earth Engine-enabled Python toolkit* —
  EMS. El método a portar (MNDWI + clasificación + subpixel contour).
- Vos et al. 2023, benchmark de satellite-derived shorelines (accuracy targets).
- Bib completo del dominio: `propuesta-4-erosion-costera/references/references.bib`.

## Relación con otros proyectos

- `~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/` — paper base
  (Geomorphology, en cola editorial), fuente de datos de test y de los
  pain-points.
- `~/proyectos/zenodo-gaps/propuesta-5-riesgo-costero-ijdrr/` — paper de
  riesgo IJDRR trabajado en paralelo; V2 de strandline podría alimentar su
  escalamiento nacional.
- `~/proyectos/surtgis/`, `~/proyectos/smelt/`, `~/proyectos/forge/` —
  crates a reusar.

## Inteligencia competitiva (2026-07-10)

Seminario MONCOSTA (U. Mayor / Centro Hémera, Idania Briceño) — bitácora en
`~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/references/seminario_moncosta_2026-07-08_bitacora.md`:
- Construyeron algoritmo propio de extracción + 3 herramientas de escritorio
  (Python) por "independencia tecnológica" → **valida el gap de tooling**.
- Su stack va camino a ser comercial/cerrado (venta de paquetes, acceso solo
  socios) → strandline open-source compilado queda en el cuadrante opuesto:
  argumento de diferenciación para el paper C&G.
- Features de ellos a considerar para V2 (NO V1): tasas por ventanas
  quinquenales, proyecciones a 10/50/70 años, formatos de fecha flexibles,
  dashboard para no-expertos.

## Estado / bitácora

- **2026-07-10**: Proyecto creado desde el análisis de gaps de la línea
  erosión costera. Nombre `strandline` verificado libre en crates.io.
- **2026-07-10 (noche)**: **SPIKE COMPLETADO — GO** ✅. Ver `spike/RESULTS.md`.
  - Workspace Rust creado: `strandline-core` (mndwi, otsu, marching squares
    subpixel réplica skimage, pipeline) + `strandline-cli` (`extract`).
    Depende de surtgis-core 0.18 (path dep). Compila, 5 tests unitarios OK.
  - 14 escenas L8 de Viña re-descargadas vía GEE (6 despejadas usadas);
    referencia CoastSat con `pan_off=True` en `spike/results/`.
  - **Exactitud**: mediana 0.38–0.47 m vs CoastSat con umbral igualado
    (criterio era < 5 m). Con Otsu global propio: 5.7–6.0 m.
  - **Velocidad**: ~18 ms/escena vs ~1.17 s/escena de CoastSat (~65×,
    single-thread, sin clasificador aún).
  - **Hallazgo**: `find_wl_contours2` de CoastSat (también en master
    upstream) contornea el MNDWI con el umbral Otsu del **NDWI** (columnas
    intercambiadas en `int_all[:,0]` vs el stack `(im_wi, im_mwi)`). El
    desplazamiento de ~5 m del modo standalone es ese quirk, no un error
    de strandline. Verificar en issues de CoastSat antes de publicarlo.
  - **Próximos pasos V1**: filtro morfológico de nubes en el modo sin
    clasificador (remove_small_objects min_size=40 sobre la máscara QA
    cruda, CFMASK marca swash como nube y fragmenta la costa); reproyección
    a EPSG de salida (escenas llegan en 32618/32619 según path).

- **2026-07-11**: **Clasificador de píxeles portado a Rust** (continuación
  del spike, ver `spike/RESULTS.md` actualizado). Módulos nuevos en
  `strandline-core`: `mlp` (forward pass del MLP de CoastSat,
  `NN_4classes_Landsat_new.pkl` → JSON embebido vía `include_str!`,
  52.9 KB, sin dependencia de sklearn), `features` (20 features réplica de
  `calculate_features`: bandas + 5 índices + 10 std locales 3×3 NaN-aware
  con reflect padding), `classify` (clasificación pixel a pixel +
  `remove_small_objects` 8-conectividad). `otsu::balanced_otsu` nuevo
  (Otsu con clases pesadas a igual peso, sin RNG). CLI: flags `--classify`,
  `--coastsat-compat` (replica el quirk NDWI/MNDWI), `--min-beach-area`.
  8 tests nuevos (13 total), todos pasando. **Resultado**: con
  `--coastsat-compat`, mediana **0.30–0.35 m** vs CoastSat — mejor que el
  0.38–0.47 m logrado antes con umbrales calculados a mano en Python, y
  ahora 100% autocontenido en el binario Rust. Validación del port: conteo
  de píxeles clasificados coincide con CoastSat real a <1.5% de diferencia
  en la escena de prueba. Costo (antes de paralelizar): el clasificador
  corría ~0.85-1.0 s/escena — comparable a CoastSat en vez de más rápido.

- **2026-07-11 (tarde)**: **Clasificador paralelizado con rayon**. Feature
  `parallel` en strandline-core (default; `--no-default-features` da binario
  determinista single-thread). Tres cambios: (1) `FeatureBank` que fusiona
  cómputo de features + predicción MLP en una pasada paralela, eliminando la
  matriz intermedia (n×20) de ~17 MB — mayor salto de escalado; (2) MLP con
  pesos en `Vec<f32>` plano recorridos en orden axpy (sin indexación 2D de
  ndarray en el loop caliente); (3) `local_std` con `par_chunks_mut`.
  Resultado: etapa classify de ~0.9 s (secuencial inicial) → ~0.15 s en 16
  cores; total ~0.17 s/escena = **~6× más rápido que CoastSat** (era apenas
  comparable). Exactitud sin cambios (0.30-0.35 m, GO). Resultado paralelo
  idéntico bit-a-bit al secuencial. 13 tests pasan en ambos modos. Cuello de
  botella restante: las 10 `local_std` corren en serie entre sí (cada una
  paralela por dentro).

- **2026-07-11 (tarde-2)**: **Corrección mareal (scope item 6) implementada**.
  Módulo `strandline-core::tide` autocontenido (no depende de transectos):
  - `tide::correct_chainage` / `TideCorrection`: fórmula de Vos et al. 2019
    `correction = (tide − ref)/beach_slope`, `corrected = raw − correction`.
    Validada **exacto** (error 0.0) contra `VINA_DEL_MAR_tidal_corrected.pkl`.
  - `tide::astro`: longitudes astronómicas medias (método Meeus), datetime→MJD.
    Validadas contra `pyTMD.astro.mean_longitudes` a <1e-4°.
  - `tide::harmonic`: **síntesis armónica nativa** desde constituyentes
    locales (13 principales: M2,S2,N2,K2,K1,O1,P1,Q1,Mf,Mm,Ssa,Sa,M4), con
    argumento de equilibrio Doodson (τ = tiempo lunar medio) + factores
    nodales de Schureman. El diferenciador que elimina el *dependency hell*
    de pyTMD/pyfes. **Validada contra pyTMD** (corrections='GOT'): V0 coincide
    a <0.01°; predicción total con constituyentes realistas de Chile central
    coincide a **1.15 mm** de error máximo.
  - `tide::read_csv` / `read_constituents_csv`: fuente de marea alternativa
    (SHOA/pyTMD/tide gauge) con parser de timestamp propio, sin deps.
  - CLI: subcomando `strandline tide` (predice marea + corrección horizontal).
  - Hallazgo de convención: los offsets de fase de los diurnos son K1 +90°,
    O1/P1/Q1 −90° (verificado contra el G de pyTMD); Schureman clásico para
    Mf/Mm difiere ~4% en f y P1 lo trata como solar puro, pero el error de
    predicción total queda en mm por el bajo peso de esos constituyentes.
  - NOTA importante: la precisión mm es del MOTOR dados los constituyentes
    correctos. La demo del CLI usa constituyentes aproximados (adivinados) →
    r=0.78 vs GOT5.5; para precisión real hay que cargar los constituyentes
    locales verdaderos (SHOA publica constantes armónicas de puertos chilenos,
    o extraer una vez de pyTMD). 26 tests totales, clippy limpio.
  - **Pendiente V1**: la corrección se integrará con los transectos (item 7)
    cuando existan — hoy `correct_chainage` opera sobre cualquier serie de
    cross-shore distances.

- **2026-07-11 (tarde-3)**: **Transectos (scope item 7) implementados**.
  Módulo `strandline-core::transects`:
  - `generate_transects`: perpendiculares (shore-normal) a una línea base —
    re-muestreo por longitud de arco, tangente por diferencias centrales,
    normal orientada al mar vía `seaward_hint`, transecto tierra→mar.
    Validado **exacto** contra el pickle del paper: 250 transectos generados
    desde la `reference_shoreline`, error máximo de coordenada **0.0000 m**.
  - `compute_intersections`: **intersección vectorizada shoreline×transecto
    con QC** — el germen del benchmark del paper C&G. Réplica del
    `compute_intersection_QC` de CoastSat (distancia perpendicular, franja
    `along_dist`, rotación a shore-normal = chainage, estadísticos NaN-aware,
    QC max_std/max_range/min_points con `multiple_inter` auto/max/nan).
    Paralelizado con rayon sobre transectos.
  - **Validación**: contra el algoritmo Python vectorizado del paper sobre
    200 shorelines sintéticas × 250 transectos — 49602 cruces, **0 mismatch
    de NaN** (el QC replica exacto), error máximo de chainage **0.1 mm**
    (redondeo del CSV).
  - **Benchmark de velocidad** (250 transectos × 200 shorelines): Python
    vectorizado del paper **11.63 s** → Rust **0.059 s** = **~197× más
    rápido**. Y el paper ya era "segundos vs 20+ min" del loop original de
    CoastSat, así que vs el `compute_intersection_QC` puro de CoastSat el
    speedup es de ~4 órdenes de magnitud. Esto es exactamente el argumento
    central del paper software+benchmark.
  - CLI: `strandline transects` (genera) y `strandline intersect` (cruza).
  - 31 tests totales, clippy limpio, pasa en paralelo y single-thread.

- **2026-07-11 (noche)**: **Tasas DSAS (scope item 8) implementadas**.
  Módulo `strandline-core::rates`:
  - `compute_rates`: NSM, SCE, EPR, LRR (regresión ordinaria con R² e IC95
    réplica de `scipy.stats.linregress`), WLR (mínimos cuadrados ponderados,
    sistema 2×2 cerrado, con IC95). Pesos por observación configurables
    (default del paper: 1/incertidumbre por satélite).
  - `despike`: despiking iterativo (réplica del paper, salto máximo
    configurable) que remueve picos/valles aislados.
  - `decimal_year_from_mjd`: conversión con la convención del paper (día
    calendario, 365.2425 días/año).
  - **Validación exacta contra el `rates_df` del pickle** (179 transectos):
    NSM/SCE/EPR a ~5e-5 (redondeo del CSV), LRR/R²/IC95 a ~5e-7, **WLR e IC95
    a ~1e-5**. El WLR coincidiendo confirma que la asignación de pesos por
    fecha (corte L5→L8 en 2013-04) reproduce los pesos por satélite del paper.
  - **Pipeline completo raw→despike→tasas**: reproduce el `rates_df` exacto
    (los 179 transectos, mismos outliers removidos por el despiking).
  - **Integración marea↔tasas validada exacto**: el flujo intersección →
    `tide::correct_chainage` → tasas reproduce el `rates_corrected` del paper
    (178 transectos) a LRR ~5e-7, WLR ~1.4e-5, NSM ~5e-5. Cierra el lazo con
    la corrección mareal del item 6.
  - CLI: `strandline rates` (con `--despike-max-change` y `--tide-correction`).
  - Hallazgo honesto: la corrección mareal no reduce el SCE agregado en Viña
    (~500 m, dominado por intersecciones múltiples, no por la marea de ±15 m);
    el efecto mareal se ve en las tasas por transecto, no en el envelope.
  - 37 tests totales, clippy limpio, pasa en paralelo y single-thread.

- **2026-07-11 (noche-2)**: **Filtros QC (scope item 9) implementados — V1
  COMPLETO**. Módulo `strandline-core::qc`:
  - `QcCriteria` + `evaluate`: los 5 criterios calibrados del paper aplicados
    **en orden** (la razón es el primero que falla): |LRR|≤5 m/año, SCE≤500 m,
    span≥20 años, n_valid≥50, R²≥0.01. `classify`: erosión/estable/acreción
    (umbral ±0.5).
  - **Validación exacta contra `change_rates_all_with_qc.csv`** (179
    transectos): **0 mismatch en excluded, 0 en reason**. Distribución de
    razones idéntica: SCE>500 (51), |LRR|>5 (36), R²<0.01 (19), n<50 (1),
    retenidos (72).
  - CLI: `strandline rates --qc` (agrega columnas excluded/reason, umbrales
    configurables).
  - **Fix necesario**: surtgis-core dejó de re-exportar `ndarray` (cambió su
    API); se agregó `ndarray = "0.16"` como dep directa de strandline-core
    (misma versión → tipos compatibles con `Raster::data()`) y se cambiaron
    los `surtgis_core::ndarray::` a `ndarray::` en 4 archivos.
  - 40 tests totales, clippy limpio, pasa en paralelo y single-thread.

## Estado del V1 (COMPLETO — 2026-07-11)

Los 9 items del scope V1 están implementados y **cada uno validado contra
CoastSat / pyTMD / el pickle del paper**:

| Item | Módulo | Validación |
|------|--------|------------|
| 1. I/O raster | (surtgis-core) | — |
| 2. MNDWI | `index` | — |
| 3. Clasificación píxeles | `mlp`+`features`+`classify` | <1.5% px vs CoastSat |
| 4. Umbral Otsu | `otsu` | — |
| 5. Extracción subpixel | `contour`+`shoreline` | 0.3 m mediana vs CoastSat |
| 6. Corrección mareal | `tide` | 1.15 mm vs pyTMD; exacto vs pickle |
| 7. Transectos | `transects` | exacto vs pickle; ~197× |
| 8. Tasas DSAS | `rates` | exacto vs rates_df |
| 9. Filtros QC | `qc` | exacto vs all_with_qc |

Pipeline de extremo a extremo: **GeoTIFF → extract → classify → transects →
intersect → (tide correct) → rates → qc**, todo en Rust, 40 tests, clippy
limpio, paralelo y single-thread.

**Próximos pasos** (post-V1): cargar constituyentes reales de Viña (SHOA)
para precisión mareal mm real; filtro de nubes en modo sin clasificador;
reproyección a EPSG de salida; empaquetar para publicación (crates.io +
mdBook + Zenodo DOI, skill `/zenodo`); escribir el paper de C&G con el
benchmark (software + accuracy vs CoastSat + velocidad).

- **2026-07-11 (noche-3)**: **Auditoría del motor + roadmap SOTA** — ver
  `AUDITORIA_MOTOR_2026-07.md`. Relectura completa con ojos frescos, 2 bugs
  **verificados empíricamente y corregidos** en el mismo commit:
  - **B1**: `generate_transects` hacía panic (índice fuera de rango) con
    línea base más corta que el spacing. Fix: tangente de respaldo desde la
    línea completa. Test nuevo; generación vs pickle sigue exacta.
  - **B2**: el constituyente **Sa tenía el argumento astronómico equivocado**
    (V0=h en vez de h−ps, convención GOT/pyTMD) → fase anual ~283° corrida,
    dato incorrecto silencioso. Quedó fuera del test original (testeaba Ssa
    pero no Sa). Fix: Doodson [0,0,1,0,-1] + Sa incorporado al test de
    predicción total (nueva referencia pyTMD, 13 constituyentes, <3 mm).
  - Golden tests del MLP agregados (4 vectores desde el sklearn original).
  - Backlog de robustez priorizado (R1–R7): timezone silencioso en CSV,
    dist_clouds faltante, NIR hardcodeado en compat, guardia de CRS,
    GeoJSON RFC 7946, taxonomía de errores.
  - **Propuestas SOTA** (ranked, en el doc): S1 subpixel por ajuste erf de
    perfil (anti pixel-locking, ~3-5 m absolutos, SHOREX-style — mejor
    razón novedad/esfuerzo), S2 streaming COG/STAC sin descarga
    (surtgis-cloud), S3 segmentación DL vía ONNX/tract, S4 beach slope
    satelital (CoastSat.slope), S5 σ por punto + conformal (smelt-ml) +
    tasas robustas, S6 Sentinel-2, S7 analítica temporal (STL/change-point/
    Getis-Ord), S8 wave runup, S9 PyO3+WASM. Secuencia sugerida: v0.2
    hardening → v0.3 SOTA-core (S1+S4+S5a) → v0.4 escala (S2+S6) → paper.
  - 42 tests, clippy limpio.

- **2026-07-12**: **v0.2 hardening (primera tanda) + validación en segundo
  sitio** — ver adenda de `AUDITORIA_MOTOR_2026-07.md`.
  - **R1 corregido**: `parse_timestamp` aplica los offsets de zona horaria
    (antes los ignoraba en silencio) y rechaza sufijos malformados.
  - **Fixture de integración en CI**: recorte 96×96 real de Reñaca (~200 KB)
    en `strandline-core/tests/fixtures/` + 2 tests golden del camino
    completo GeoTIFF → shoreline (sin y con clasificador).
  - **Constitución validada** (bahía curva, terremoto 2010, S2, QC
    relajado): generación de transectos exacta (176, err 0.0 m), tasas
    NSM/SCE/EPR/LRR exactas, QC 0 mismatch (93/81 idéntico), flujo mareal
    exacto (78/78). **Ningún bug nuevo del motor en geometría distinta.**
  - **Hallazgo E1** (del harness, no del motor): el WLR difería 0.56 m/año
    porque Constitución incluye Sentinel-2 (peso 1/7) y el satname no se
    retuvo en el pickle — el proxy de pesos por corte de fecha solo vale
    para sitios Landsat-only. Estimador demostrado correcto (numpy con
    pesos idénticos coincide a 5e-5). Trampa de uso documentada: en sitios
    multi-misión pasar pesos reales por satélite en la columna `weight`.
  - 46 tests (44 unitarios + 2 integración), clippy limpio.

- **2026-07-12 (2)**: **v0.2 COMPLETO — R2–R7 cerrados** (adenda 2 de la
  auditoría).
  - R6: variantes de error Io/Parse/UnsupportedCrs. R3: `--nir-band`.
    R7: doc de línea-infinita.
  - **R2**: módulo `cloud` — máscara limpia (remove_small_objects 4-conn,
    40 px, mata las falsas nubes de swash de CFMASK) + buffer `dist_clouds`
    300 m con EDT exacta (Felzenszwalb, validada vs fuerza bruta), partiendo
    polilíneas. **Exactitud vs CoastSat mejoró: mediana 0.30 → 0.20 m, media
    7.71 → 2.93 m, p90 → 1.20 m**; costa des-fragmentada (3-9 líneas vs
    9-37). Goldens del fixture actualizados (1 línea continua).
  - **R4+R5**: módulo `proj` — UTM↔WGS84 nativo (Karney n⁴, sin PROJ),
    validado vs pyproj a <1 mm incluyendo el caso GEE zona-norte/northing
    negativo. Guardia anti-CRS-geográfico. `Shoreline::reproject` +
    `--output-epsg` (4326 → GeoJSON RFC 7946; proyectado → member crs
    legacy). End-to-end: reproyección nativa reproduce los 0.20 m del flujo
    pyproj.
  - 53 tests (51 + 2 integración), clippy limpio, ambos modos.
  - v0.2 queda cerrado salvo proptest/fuzzing (opcionales). Siguiente:
    v0.3 SOTA-core (S1 subpixel erf + S4 slope satelital + S5a σ).

- **2026-07-12 (3)**: **v0.3 SOTA-CORE COMPLETO — S1+S4+S5a** (adenda 3 de
  la auditoría). El paper deja de ser "port rápido" y tiene método propio.
  - **S1** (`subpixel` + CLI `profile`): ajuste de perfil sigmoide
    (LM 4 parámetros) sobre el MNDWI muestreado bilinealmente por
    transecto. **3× menos RMSE (0.53 vs 1.61 m) y sesgo eliminado (+0.02
    vs −0.78 m)** vs el flujo contorno+intersección en el experimento
    sintético con maquinaria de producción real. La figura de accuracy
    del paper (test `pixel_locking.rs`).
  - **Hallazgo E2**: el perfil expone transectos torcidos que la
    intersección-franja tolera (transectos del paper: 18/250 cruces; flujo
    auto-referenciado extract→baseline→transects --smooth-window→profile:
    69/69). `smooth_polyline` agregado al generador.
  - **S4** (`slope` + CLI `slope`): Lomb-Scargle nativo + barrido de
    pendientes minimizando energía mareal residual (Vos 2020). Recupera
    slope a ±0.01 en sintético con muestreo Landsat; 0.08→0.0825 sobre las
    770 fechas reales + marea GOT real de Viña.
  - **Hallazgo E3 (límite cuantificado)**: con ruido ≥5 m el estimador se
    sesga hacia abajo; en Viña (micro-tidal) NO es identificable con
    solo-Landsat — requiere S2 o sitio macro-tidal (Chiloé). Consistente
    con la advertencia de Vos 2020. Resultado de paper.
  - **S5a**: σ por punto en `profile` (σ_fit del ajuste + georef + umbral
    vía ∂x/∂t=w/b del modelo). Alimenta los pesos reales del WLR.
  - Refactor: `select_level` compartido extract/profile (contorneo
    unificado a MNDWI textbook, geométricamente neutro: 0.20/0.19 m).
  - 60 tests (57 + 3 integración), clippy limpio, ambos modos.

- **2026-07-12 (4)**: **v0.4 ESCALA — S2 streaming + S6 Sentinel-2**
  (adenda 4 de la auditoría).
  - **S2** (`strandline fetch`, feature `cloud` default): búsqueda STAC en
    Planetary Computer + lectura de SOLO la ventana del AOI vía COG range
    requests (surtgis-cloud, firma SAS automática) → pares ms/mask
    compatibles con el pipeline. Demostrado: ventana de Viña = ~870 KB
    transferidos vs ~1 GB de escena; fetch→extract sin GEE ni descarga.
  - **Hallazgo E4**: el MLP de CoastSat NO transfiere a SR (entrenado con
    TOA de GEE) — en landsat-c2-l2 clasifica 68% como sand. Flujo MPC usa
    Otsu refinado: 9.35 m vs referencia CoastSat (comparación
    inter-producto SR-30m vs TOA-15m, ≈0.3 px, dentro de Vos 2023).
    Reentrenar sobre SR = trabajo del paper.
  - **S6**: sentinel-2-l2a con B11 20m→10m bilineal y máscara SCL
    (`--mask-kind scl`, `cloud_mask_from_scl` clases 3/8/9/10).
    Consistencia inter-misión S2(01-09) vs L8(01-10) desde MPC:
    **6.29 m** de mediana (1 día de diferencia, marea distinta). S6
    habilita el slope satelital en micro-tidal (E3). Limitación
    documentada: offset BOA post-2022 pendiente.
  - 60 tests, clippy limpio, compila con y sin feature `cloud`.

- **2026-07-12 (5)**: **Offset BOA de Sentinel-2 corregido** (cierre del
  backlog v0.4, adenda 5 de la auditoría). `fetch` aplica DN−1000 cuando
  el `s2:processing_baseline` del item STAC es ≥ 04.00 (BOA_ADD_OFFSET
  del 2022-01-25), con fallback por fecha de adquisición si la property
  falta — se decide por baseline y no por fecha porque ESA reprocesa
  escenas antiguas con baselines nuevos. **Validado empírico** con escena
  real 2024 de Viña (baseline 05.10): reflectancia de agua consistente
  con la escena control 2020 (MNDWI agua 0.445 vs 0.415); sin el fix el
  MNDWI de agua colapsaba a ~0.07 en silencio. Extract end-to-end OK
  sobre la escena 2024 (8 líneas, umbral 0.043). Desbloquea series S2
  densas que cruzan 2022 → experimento slope satelital en micro-tidal
  (E3+S6). 63 tests, clippy limpio.

- **2026-07-12 (6)**: **E3 SUPERADO — slope satelital en micro-tidal con
  S2 denso + hallazgo E5 corregido** (adenda 5 de la auditoría).
  - **Robustez de `fetch`**: cap de paginación STAC silencioso corregido
    (pedías 600 escenas, llegaban las 42 más recientes); escena con error
    HTTP transitorio se omite en vez de abortar el batch; reanudación
    (salta pares ms/mask existentes).
  - **Hallazgo E5 (bug del motor, corregido)**: `estimate_beach_slope`
    asumía chainage creciente con la marea (convención del pickle del
    paper, transectos mar→tierra); con los transectos nativos
    (tierra→mar) toda corrección agregaba energía y el barrido escapaba
    al borde (38/75 en el máximo). Fix: barrido pendiente × signo, campo
    `tide_sign` en la salida. La adenda 3 no lo vio porque usó las
    intersecciones del paper.
  - **Experimento** (flujo 100% nativo, 215 escenas S2 2019–2025 de Viña
    vía fetch, GOT5.5 real reinstalado y validado exacto vs pickle):
    regresión directa **0.0967**, espectral crudo **0.105**, espectral
    con despike 3-MAD **0.0725** — vs 0.10 de literatura del paper. La
    densidad S2 hace identificable el slope donde solo-Landsat falla
    (E3): resultado de paper. Artefactos en `spike/slope_s2/`.
  - 63 tests, clippy limpio.

- **2026-07-12 (7)**: **Ergonomía pre-release** (adenda 6 de la
  auditoría): dos asperezas que el propio experimento de (6) expuso.
  - `slope --despike-max-change`: reutiliza `rates::despike` (validado
    exacto vs el paper) sobre el chainage antes del barrido espectral —
    el experimento mostró que mejora la coherencia del signo (30/75 →
    44/75).
  - **Subcomando `baseline`** (nuevo módulo `strandline-core::baseline`):
    elige automáticamente la mejor línea base entre las escenas de
    `fetch`, reemplazando el script ad hoc que en (6) requirió
    intervención manual. Dos heurísticas ingenuas descartadas con datos
    reales antes del diseño final: "más vértices totales" (elige la
    escena más ruidosa) y "menos fragmentos sin más" (eligió una escena
    de 224 m/1 fragmento sobre alternativas de 9-16 km) — el criterio
    final es longitud del fragmento más largo penalizada por raíz
    cuadrada del número de fragmentos, más un segundo filtro de largo
    (`--min-length`) porque `extract_shoreline` no re-filtra por largo
    los trozos que corta el buffer de nubes (paridad CoastSat). Validado
    end-to-end: sobre la serie real de Viña eligió una escena de 9307 m/4
    fragmentos → 74 transectos, contra los 75 obtenidos fijando la
    escena a mano.
  - 69 tests (63 + 6 de `baseline`), clippy limpio.

- **2026-07-12 (8)**: **Los 6 casos del paper, validados** (adenda 7 de
  la auditoría) — Arica, Coquimbo, Valdivia y Chiloé cerrados sin ningún
  cambio de código, usando los `{SITE}_analysis.pkl` del paper (Viña y
  Constitución ya estaban validados desde V1/v0.2). Transectos exactos a
  micrómetros (100/100, 334/334, 334/334, 316/316); tasas NSM/SCE/EPR a
  ~5e-5 m, LRR/R²/CI95 a ~1e-6; **0 mismatches de QC en los 4 sitios**
  (259/1054 transectos evaluados). El motor generaliza sin ajuste a
  geometrías muy distintas (desierto, estuario, archipiélago
  macro-tidal) y a un salto coseísmico real (Illapel 2015, Coquimbo) sin
  caso especial en el código. WLR no exacto por falta de `satname` en
  estos pickles (mismo límite de datos que E1, no del estimador —
  mediana 0.01–0.29 m/año según mezcla Landsat/S2 del sitio). Con esto
  los 6 sitios del plan de benchmark del paper C&G quedan validados
  end-to-end. Artefactos en `spike/multisite/`. 69 tests (sin cambios).

- **2026-07-13**: **`fetch` cubre el archivo Landsat completo (TM/ETM+/
  OLI)** (adenda 8) — origen `TASK_fetch_full_landsat_archive.md`,
  bloqueaba el reanálisis multidecadal (1984–2025) sin GEE. El filtro
  hardcodeado a L8/L9 era lo único que faltaba: los nombres de asset y
  el escalado de reflectancia SR ya son idénticos para TM/ETM+/OLI
  (verificado contra el catálogo STAC real). Flags nuevos:
  `--sensors <tm|etm|oli|all>` (default `all` — cambio de comportamiento
  intencional) y `--skip-l7-slc` (excluye ETM+ post-falla del SLC,
  2003-05-31; default no excluir). Validado contra los 4 criterios del
  task: fetch de Viña 2005 devuelve 10 escenas L5/L7 (antes 0);
  `extract --coastsat-compat` sobre L5 fetch vs la misma fecha desde
  `ms.tif` de GEE en `/mnt/kingston` da mediana 15.7-16.6 m dentro del
  bbox común (comparación inter-producto SR-30m vs pansharpened-15m,
  sub-pixel); `--skip-l7-slc` excluye correctamente 7/7 escenas ETM+ de
  2005; sin regresión en OLI/S2. **Hallazgo lateral**: `/mnt/kingston`
  retiene el `VINA_DEL_MAR_output.pkl` crudo con `satname` (779 fechas)
  — resolvería el hallazgo E1 (WLR exacto) si se revisita; no se
  encontraron datos de entrenamiento reales del clasificador CoastSat en
  disco (polígonos etiquetados) — el reentrenamiento sobre SR para el
  estudio nacional sigue bloqueado en etiquetado nuevo, pausado por
  decisión explícita del usuario. 71 tests (69 + 2 de `fetch`), clippy
  limpio.

- **2026-07-16**: **Reentrenamiento del clasificador SR intentado vía
  pseudo-labeling automático — E4 CERRADO como limitación documentada,
  NO resuelto**. Retoma el hallazgo E4 (el MLP TOA no transfiere a SR,
  ~63-77% de píxeles mal clasificados como sand). Se investigó primero si
  existía un atajo: **no hay clasificador SR oficial en CoastSat/CoastSeg**
  (confirmado por el propio maintainer, issue #349) ni dataset público
  reusable para las 4 clases exactas (Coast Train es lo más cercano pero
  es RGB pansharpened, no SR, y no cubre Chile).
  - Se construyó el pipeline completo en `spike/classifier_sr/`:
    `label_sr_images.py` (adapta `SDS_classify.label_images` de CoastSat
    a los pares `*_ms.tif`/`*_mask.tif` de `fetch`, labeling interactivo
    real si se necesita a futuro), `pseudo_label_sr.py` (labels
    automáticas por reglas de alta confianza: MNDWI para water/land,
    NDVI para vegetación, brillo+proximidad al agua para sand, franja
    costera brillante para whitewater — decisión explícita del usuario de
    evitar clics manuales) y `train_sr_classifier.py` (reusa
    `load_labels`/`format_training_data` de CoastSat sin modificar,
    arquitectura real de producción `MLPClassifier(hidden_layer_sizes=
    (100,))` — se descubrió que difiere de la del notebook de ejemplo de
    CoastSat, que usa (100,50) y otra numeración de labels).
  - **Hallazgo del experimento**: sand/other NO es separable de forma
    confiable por reglas espectrales automáticas. Sin contraejemplos de
    tierra brillante lejos del agua, el modelo sobre-predice sand (22% en
    Viña); agregándolos, lo sub-predice a casi cero (0.3-1.9% en los 3
    sitios) porque en costas áridas (Arica) el suelo desértico inland es
    espectralmente ~idéntico a la arena de playa real — el péndulo entre
    iteraciones fue la señal de que el límite es real, no un bug de
    calibración de umbrales.
  - **Validación correcta usada** (no el % de clase, que no es la métrica
    que importa): se comparó la línea de costa extraída
    (`extract --classify`) contra el Otsu-sin-clasificador ya validado.
    **Viña (costa templada): 3.88 m de mediana, dentro de tolerancia** —
    el clasificador ahí es inocuo. **Arica (costa árida): 7.48 m mediana,
    12.81 m media, y 63% más de longitud de línea total (29.8 vs 18.3
    km)** — el clasificador introduce contornos espurios reales, no solo
    ruido cosmético en el porcentaje de clase.
  - **Decisión del usuario**: no integrar el clasificador SR reentrenado.
    El modo SR sigue usando el pipeline Otsu-refinado (ya validado
    ~9.35 m, adenda 4) sin excepción — más simple, sin el riesgo
    demostrado en costas áridas. `strandline-core/models/
    coastsat_landsat_mlp.json` queda sin tocar (se restauró tras el
    experimento). El código de `spike/classifier_sr/` se conserva sin
    integrar, como documentación del intento y método candidato para la
    sección de limitaciones del paper C&G ("se intentó pseudo-labeling
    automático para SR; funciona en costas templadas/húmedas pero no es
    confiable en costas áridas por indistinguibilidad espectral genuina
    sand/suelo-desértico — requeriría etiquetado manual real, pausado por
    costo/beneficio"). 71 tests sin cambios (no se tocó strandline-core
    ni strandline-cli en esta sesión).

- **2026-07-16 (2)**: **`profile` sembrado desde el contorno Otsu — arregla
  la falla del ajuste subpixel en frentes urbanos**. Mismo experimento
  `profile_sr` (comparar `profile` contra `extract+intersect` para SR,
  ground truth = `cross_distance` de los pickles del paper en fechas
  exactas coincidentes con escenas SR fetched): `profile` sin semilla daba
  mediana 153.73 m en Viña (vs 22.78 m de `extract+intersect`, el baseline
  actual) — el buscador ingenuo 1D (`initial_crossing`, último cruce
  ascendente del perfil suavizado) engancha una meseta espuria
  (estacionamiento/sombra/arena húmeda) que nunca vuelve a cruzar el
  umbral antes de la costa real, así que no hay una segunda transición que
  preferir. En Arica (costa simple, un solo borde agua-arena) sí mejoraba
  (11.15 → 8.62 m) — el método no está roto en sí, es frágil en geometría
  compleja.
  - **Fix**: `subpixel::fit_profile`/`fit_profiles` reciben una semilla
    opcional (`seed: Option<f64>`/`seeds: &[Option<f64>]`) que reemplaza
    al buscador ingenuo como centro de la ventana de ajuste. El CLI
    `profile` la calcula sembrando desde el cruce del contorno Otsu de
    `extract_shoreline` (mismos `ExtractParams`, mismo nivel) contra cada
    transecto vía `compute_intersections` — el contorno ve la escena
    completa en 2D, no confunde el blob aislado con el cuerpo de agua
    principal. Si el ajuste local no bracketea el nivel alrededor de la
    semilla (la meseta espuria: nunca vuelve a cruzar), se reporta la
    semilla misma (`sigma_m=NaN`) en vez de `None` — el resultado sembrado
    nunca es peor que el Otsu que lo originó. Sin semilla (`None`) el
    comportamiento es exactamente el de antes.
  - **Validación end-to-end** (mismo experimento): Viña 153.73 m →
    **24.57 m** (n pasó de 20 a 79, por encima de los 77 de `intersect`);
    Arica 8.62 m → 9.66 m pero con cobertura casi duplicada (259 → 397).
    **Global: 13.21 m (Otsu+intersect, baseline actual) → 11.66 m (profile
    sembrado) con ~32% más transectos cubiertos** — supera al baseline de
    producción, no solo empata.
  - 2 tests nuevos en `subpixel.rs` (meseta espuria + fallback a la
    semilla; refinamiento real con semilla imprecisa). 66 tests unitarios
    (64+2), pixel_locking + extract_integration OK, clippy limpio en el
    código tocado (lint preexistente no relacionado en `otsu.rs` por drift
    de versión de clippy, no tocado en esta sesión).
  - **Pendiente**: activar `profile` sembrado como default de
    `extract`/`baseline` para SR (hoy solo `profile` lo usa); evaluar si
    conviene igual para TOA (no debería empeorar, activa el mismo
    fallback-nunca-peor-que-semilla).

- **2026-07-17**: **`extract --subpixel-refine` y `baseline --subpixel-refine`
  agregados** (mismo mecanismo de la adenda 9, aplicado al contorno mismo
  en vez de a transectos externos: `subpixel::refine_line_subpixel` reusa
  `generate_transects` tratando la línea como su propia base, genera
  mini-transectos perpendiculares sembrados en cada punto pixel-locked, y
  ajusta con el mismo `fit_profile`+fallback ya validado). Off por default
  en ambos comandos — no toca la paridad exacta con CoastSat que varias
  validaciones del proyecto dependen (confirmado: `extract_integration`
  sigue pasando igual). 68 tests (66+2 nuevos), clippy limpio, smoke test
  manual sobre el fixture (1.59 m de desplazamiento mediano vs el contorno
  crudo, mismo N de líneas y umbral).
  - **Cierre del loop con el hallazgo E4** (adenda 4, 9.35 m documentado
    para el pipeline SR-sin-clasificador): se re-fetchearon las 6 escenas
    SR de Viña que coinciden en fecha exacta con `coastsat_reference.geojson`
    (ground truth real TOA) y se comparó `extract` con y sin
    `--subpixel-refine` con la misma metodología del spike original
    (distancia punto-a-línea simétrica, corredor 150 m). Reprodujo el
    9.35 m exacto para 2020-01-10 sin la flag, confirmando la metodología.
    **Con la flag: 9.35→9.18 m en esa escena; global (5 de 6 fechas, una
    excluida por 0 líneas — escena con umbral degenerado, no relacionado
    al cambio) mediana 11.41→11.28 m, media 18.90→17.39 m (~8% mejor),
    p90 38.34→32.70 m (~15% mejor)**. Mejora real pero modesta,
    concentrada en reducir los peores casos (p90/media) más que la
    mediana — consistente con que el techo lo pone el desajuste de
    resolución inter-producto (SR 30 m vs referencia TOA 15 m
    pansharpeada, ya documentado en la adenda 4): el refinamiento quita
    el sesgo de pixel-locking dentro de la resolución nativa SR, no
    puede recuperar información perdida por el píxel más grueso. Una
    escena (2020-01-26) empeoró levemente (12.54→12.79 m) — no
    sistemático. Script: `spike/profile_sr/recheck_e4_subpixel.py`.

  - **Extensión a los 4 sitios restantes del benchmark** (Coquimbo,
    Valdivia, Chiloé, Constitución) — **hallazgo: el patrón se invierte**
    respecto a Viña/Arica. Se comparó `otsu_intersect` (baseline) vs
    `extract --subpixel-refine` + intersect vs `profile` sembrado, contra
    `cross_distance_clean` real de los 4 pickles (`{SITE}_analysis.pkl`,
    33 fechas SR fetched con match exacto entre los 4 sitios):
    ```
                                  mediana   media    p90    n
    otsu_intersect (baseline)     13.98 m  49.79 m  80.74   1753
    extract --subpixel-refine     11.87 m  42.01 m  59.54   2489  <- gana
    profile sembrado              14.75 m  46.86 m  71.66   2009  <- pierde
    ```
    `extract --subpixel-refine` gana en los 4 sitios y globalmente
    (~15% mejor que el baseline, más cobertura). `profile` sembrado
    **pierde contra el baseline** en los 4 (Viña/Arica fue la excepción,
    no la regla). Explicación: los mini-transectos de
    `refine_line_subpixel` se generan FRESCOS por escena, perpendiculares
    al contorno real de ESA fecha — siempre bien alineados. Los
    transectos de `profile` son fijos, generados UNA vez desde el
    `reference_shoreline` histórico del sitio; en costas complejas/
    dinámicas (terrazas de Coquimbo, estuario de Valdivia, bahías
    macro-mareales de Chiloé, desembocadura de Constitución) esos
    transectos largos (500 m) ya no aproximan bien la normal local de la
    costa en una fecha dada, así que la ventana de ajuste de 60 m puede
    no bracket-ear el borde real o bracket-earlo de forma sesgada. En
    Viña (costa relativamente recta y estable) esa desalineación es
    chica y `profile` gana; en las otras 4, domina. **Recomendación
    revisada**: `extract --subpixel-refine` es la mejora robusta de
    default candidata para SR; `profile` sembrado sigue siendo mejor
    SOLO cuando los transectos existentes son geométricamente fieles a
    la costa actual (típicamente sitios simples o con baseline
    reciente). Script: `spike/profile_sr/run_experiment_multisite.py`.

  - **Bug real encontrado y corregido durante la validación**: al
    diagnosticar por qué Chiloé tenía mayoría de fechas "sin resultados
    utilizables" (11 de 15), se descartó nubosidad/nodata (escenas 99.7-
    99.8% de píxeles válidos) — la causa era `nd_index`
    (`strandline-core::index`, usado por MNDWI y las 5 features
    espectrales del clasificador) explotando numéricamente: Landsat
    Collection 2 SR permite reflectancia negativa (sombras/agua oscura
    tras la corrección atmosférica), y cuando `Green+SWIR1` cruza cerca de
    cero un puñado de píxeles (~0.02-0.03%) da valores de **cientos a
    miles** en vez de un índice normalizado válido, corrompiendo el
    histograma de `otsu_threshold` completo → umbral degenerado, 0 líneas
    extraídas silenciosamente. **25 de 60 escenas (42%) de los 4 sitios**
    tenían este problema.
    - **Fix**: `nd_index` rechaza (`NaN`) valores con `|resultado| >
      MAX_ABS_VALUE` (5.0), no exactamente `[-1,1]` — se intentó primero
      con `[-1,1]` (con y sin tolerancia de punto flotante) y ambas
      versiones rompían el golden `extract_coastsat_compat_matches_golden`
      (`water_px` 4610→4609): el fixture de integración TIENE un píxel
      real con SWIR1=-0.0028 que da -1.05/-1.22 en dos índices —
      moderadamente fuera de `[-1,1]` pero NO una explosión numérica, y
      CoastSat original (que no filtra esto en absoluto) también lo
      tolera. `5.0` separa con margen amplio el caso real-pero-moderado
      (toleral, fiel al port) del caso de explosión numérica genuina (se
      rechaza).
    - **Validado**: 72 tests (2 nuevos: `excursion_moderada_fuera_de_1_
      pasa_sin_filtrar` con el pixel exacto del fixture,
      `explosion_numerica_real_da_nan` con el patrón del bug real),
      `extract_integration` (los 2 goldens de paridad CoastSat) y
      `pixel_locking` OK, clippy limpio. **0 de 60 escenas degeneradas
      tras el fix** (antes 25/60). Re-corriendo el experimento multisitio
      con el fix, la cobertura subió de 33 a 56 fechas usadas (13-15 por
      sitio en vez de 4-10) y el resultado se **confirma y se fortalece**
      con la muestra más grande:
      ```
                                    mediana   media    p90     n
      otsu_intersect (baseline)     14.29 m  57.32 m  120.39   3112
      extract --subpixel-refine     11.78 m  48.46 m   81.04   4325  <- gana (~18% mejor mediana)
      profile sembrado              15.15 m  53.92 m   90.52   3523  <- sigue perdiendo
      ```
    - **No commiteado todavía** (index.rs). Este bug fix es independiente
      del trabajo de refinamiento subpixel de las adendas 9-10 y sería
      igual de válido si nunca se hubiera tocado `profile`/`extract`
      --subpixel-refine — se encontró como efecto colateral de la
      validación, no del refinamiento en sí.

- **2026-07-17 (2)**: **Investigación de estado del arte 2023-2026** (deep
  research, 107 agentes, 24 fuentes, 25 claims verificadas 3-voto) para
  actualizar el backlog S1-S9 de `AUDITORIA_MOTOR_2026-07.md` y contrastar
  el hallazgo de transectos dinámicos vs fijos de esta sesión.
  - **S3 (DL segmentation)**: CoastSeg/Doodleverse ya tiene SegFormer para
    el mismo esquema de 4 clases (water/whitewater/sediment/other), pero
    sus propios autores (Fitzpatrick et al. 2024, JOSS) dicen que no está
    validado en campo todavía. Sin ruta ONNX (persiste en TF/.h5) — la vía
    pura-Rust real es **Candle** (HuggingFace), no `tract` como decía el
    audit original.
  - **S5 (incertidumbre)**: conformal prediction sigue vigente; hay algo
    más nuevo — **GeoConformal Prediction** (Lou et al. 2025, AAG),
    ponderación geográfica, código abierto (`geoconformal` en Python),
    supera a bootstrap (93.67% vs 81% cobertura). Confianza media (voto
    2-1, no unánime).
  - **S7 (analítica temporal)**: **confirmado como práctica activa, no
    solo teoría** — Warrick, Vos y Buscombe (creadores de CoastSat/
    CoastSeg) publicaron 2025 (JGR) STL + análisis espectral + clustering
    sobre 7777 transectos/777km de California (22 años CoastSat): ~90% de
    los transectos tienen ciclos estacionales significativos, y la
    erosión NO es uniformemente invernal (59% invierno, >30% primavera/
    verano) — desafía el supuesto DSAS lineal. **PELT/BEAST específicamente
    sin evidencia de uso en SDS** (claims que lo vinculaban, refutadas).
  - **S8 (wave runup/setup)**: sigue **genuinamente abierto** — revisión
    sistemática 2026 (MDPI) documenta split metodológico activo sin
    consenso (corregir solo setup vs. setup+runup, depende del sitio). No
    hay librería estándar (`py-wave-runup` como "estándar" fue refutado).
  - **S6 (fusión multisensor)**: cuantificado — Vos et al. 2023 documenta
    shift sistemático hacia el mar de 4.7 m (CoastSat) / 8 m (SHOREX) al
    pasar Landsat→S2; S2 no supera consistentemente a Landsat (depende del
    sitio, peor en Duck, mejor en Narrabeen).
  - **Gap nuevo**: los estudios operacionales de mayor escala (Shoreline
    Monitor global, extensión Atlántico europeo 2024 de Castelle et al.)
    usan **transectos fijos con espaciado por latitud + suavizado espacial
    post-hoc**, no regeneración dinámica por escena — evidencia indirecta,
    no concluyente.
  - **Sobre el hallazgo propio de transectos dinámicos vs fijos** (adenda
    10): **no se encontró literatura que lo confirme ni lo contradiga**,
    ni en SHOREX/SAET (Pardo-Pascual) ni en otros grupos. La búsqueda
    explícitamente no logró acceder al corpus específico de Pardo-Pascual
    sobre este punto — laguna de evidencia, no confirmación de novedad.
    Pendiente: búsqueda dirigida antes de reclamarlo como aporte original
    en el paper C&G.
  - **Claims refutadas en verificación adversarial** (no citar): Rbeast/
    BEAST validado en SDS, `py-wave-runup` como estándar reproducible,
    sesgo SWIR-1↔wave-runup R²=0.69, bias de ~10m de S2 en el estudio
    europeo, "CoastSat sigue siendo MLP clásico confirma que S1 sigue
    abierto" (matiz, no simplemente falso).
  - **Decisión**: seguir con S7 (STL) primero — reusa lo ya construido
    (series por transecto), validado a escala por el propio grupo de
    referencia, sin dependencias pesadas nuevas.

- **2026-07-17 (3)**: **S7 implementado — descomposición STL de series de
  transecto**. Nuevo módulo `strandline-core::stl` + subcomando CLI
  `strandline stl`. Reusa la infraestructura de `rates` (mismo CSV de
  `intersections`/`dates`, `read_date_weights`, agrupación por transecto).
  - **Diseño**: las series satelitales son de muestreo irregular (nubes,
    revisita desigual Landsat/S2); STL clásico (Cleveland et al. 1990)
    asume muestreo regular. Se resamplea a grilla mensual (media de
    observaciones por mes calendario, interpolación lineal en huecos,
    recortada a `[primer, último]` mes con dato real — sin extrapolar en
    los bordes) antes de aplicar STL con periodo 12. Usa el crate `stlrs`
    0.4 (MIT/Unlicense, implementa el algoritmo real, publicado justo
    2026-07-07) en vez de reimplementar LOESS iterativo desde cero —
    criterio de reuso consistente con `ndarray`/`rayon` (dependencias
    externas para machinery estadística/numérica general, reimplementación
    propia solo donde es el diferenciador del proyecto, como `tide`).
  - **Salida**: CSV largo (`transect,month,trend_m,seasonal_m,residual_m,
    interpolated`) + resumen a stderr (n transectos descompuestos, amplitud
    estacional mediana). `StlResult` expone también `seasonal_variance_frac`
    (fracción de la varianza de corto plazo explicada por el estacional,
    proxy de "significancia estacional" en el espíritu de Warrick et al.
    2025).
  - **Validado**: 5 tests nuevos (recupera amplitud estacional conocida de
    una senoidal sintética con ruido y muestreo irregular tipo satélite;
    distingue una serie sin estacionalidad real, `<6 m` vs `>8 m` con
    estacionalidad real; casos borde de serie corta/vacía; interpolación de
    huecos mensuales). 77 tests totales (72+5), `extract_integration` y
    `pixel_locking` OK, clippy limpio. Smoke test manual con 2 transectos
    sintéticos (6 años, ~110 fechas irregulares): T001 con amplitud
    estacional real de 20 m recuperó 19.12 m; T002 sin estacionalidad
    (solo tendencia+ruido) dio 2.47 m — separación clara y correcta.
  - **Corrido sobre datos reales de Viña (2026-07-17, mismo día) — hallazgo
    honesto: Viña NO replica el patrón de Warrick et al. 2025**. Se
    construyeron `intersections.csv`/`dates.csv` desde
    `VINA_DEL_MAR_tidal_corrected.pkl` (179 transectos, 770 fechas reales,
    47303 observaciones) y se corrió `strandline stl` completo.
    - Con `cross_distance_uncorrected`: amplitud estacional mediana
      60.85 m (muy sesgada — media 163 m, p90 487 m), solo **9% de los
      transectos con `seasonal_variance_frac > 0.5`** (vs ~90% "con ciclo
      estacional significativo" en California). Con
      `cross_distance_corrected` (misma serie con corrección mareal
      aplicada): prácticamente igual (66.00 m mediana) — confirma el
      hallazgo ya documentado (V1) de que la marea no domina el SCE de
      Viña. Restringiendo a los 72 transectos QC-retenidos del paper
      (filtrando ruido/artefactos): mejora algo pero sigue lejos (47.31 m
      mediana, 11% con frac>0.5, 39% del retroceso máximo cae en invierno
      austral jun-ago vs ~59% invierno en California).
    - **Hipótesis ENSO probada y REFUTADA** (2026-07-17, mismo día): se
      cruzó el residuo STL (promedio across los 72 transectos QC-buenos
      por mes, misma lógica "along-coast mean" que usa el propio script
      del paper) contra el índice ONI real, usando el resultado ya
      guardado en `VINA_DEL_MAR_enso_analysis.pkl` (371 meses,
      `climate_merged`). **Correlación residuo-STL vs ONI: r=-0.038 en
      lag 0 (p=0.46, no significativo); mejor lag 12 meses da r=-0.109
      (p=0.04, apenas significativo) — explica como máximo 1.2% de la
      varianza del residuo**. El propio análisis ENSO del paper
      (correlación con la anomalía cruda, no con mi residuo STL) también
      es débil: r=-0.103, p=0.047, mejor lag -4 meses r=-0.129. **ENSO NO
      es el responsable de la baja `seasonal_variance_frac` de Viña** — la
      hipótesis inicial (variabilidad ENSO cayendo en el residuo) queda
      descartada por datos, no confirmada.
    - **Estado actual, honesto**: el módulo/CLI funcionan correctamente
      (validado sintético + corrió sin errores sobre 47K observaciones
      reales + la correlación en sí se calculó bien). Lo que queda sin
      explicar es POR QUÉ Viña no muestra el patrón estacional dominante
      de California — ni la marea (V1, ya descartada), ni el filtro QC,
      ni ENSO (recién descartado) lo explican. Candidatos no probados
      todavía: ruido de múltiples intersecciones por transecto (ya
      documentado como causa del SCE~500m), sesgo inter-sensor
      Landsat/Sentinel-2 (4.7-8 m sistemático, adenda de investigación
      SOTA), o variabilidad de tormentas de escala sub-mensual que ONI
      mensual no captura. **No asumir la explicación ENSO en el paper sin
      volver a probarla** — quedó descartada explícitamente, no
      confirmada.
    - **Candidato 1 probado — coherencia espacial del residuo entre
      transectos vecinos**: correlación pairwise de residuos entre los 72
      transectos QC-buenos, en función de la distancia (índice de
      transecto, ~100 m de spacing). Global: media r=0.059 (casi
      independiente). Pero transectos vecinos (distancia ≤2, ~200 m)
      correlacionan más (media r=0.193) que transectos lejanos (distancia
      ≥20, ~2 km): media r=0.053. Hay estructura espacial real (no es
      ruido puramente independiente por transecto), pero no distingue
      entre "proceso costero local real" y "artefacto compartido por
      escena" (transectos vecinos ven la misma imagen).
    - **Candidato 2 probado y CONFIRMADO (fuerte)** — ruido inter-sensor:
      se recuperó `VINA_DEL_MAR_output.pkl` crudo de `/mnt/kingston`
      (779 fechas con `satname`: L5=333, L7=75, L8=279, L9=92 — resuelve
      parcialmente el hallazgo E1 de la adenda 8). Se asignó el satélite
      dominante por mes y se comparó la varianza del residuo STL por
      sensor. **Test de Levene: L8 (n=120) std=5.04 m vs L5+L7+L9
      combinados (n=255) std=8.74 m — razón de varianza 3.01×, p=0.0001**
      (ANOVA de medias no significativo, p=0.42 — no es un sesgo
      sistemático de nivel, es una diferencia real de PRECISIÓN). L8 es
      ~3× más preciso que el resto. Esto **sí explica una fracción
      sustancial del residuo** — mucho más que ENSO (1.2%) — y es
      consistente con el hallazgo de la investigación SOTA del mismo día
      (Vos et al. 2023: sesgo sistemático inter-misión Landsat/Sentinel-2,
      4.7-8 m) extendido a diferencias INTRA-Landsat (L5/L7/L8/L9 entre
      sí, no solo Landsat-vs-S2).
    - **Conclusión de la cadena de hipótesis**: marea (descartada, V1) →
      ENSO (descartada, r²≤1.2%) → **ruido inter-sensor (confirmado,
      razón de varianza 3×, p=0.0001)**, con una componente de coherencia
      espacial de corto alcance sin resolver del todo (candidato 1).
      **Accionable**: el hallazgo E1 (WLR con pesos reales por satélite en
      vez del proxy por fecha de corte) queda validado como el próximo
      paso natural — ahora hay datos concretos (`/mnt/kingston` con
      satname) para calibrar pesos 1/std reales por sensor en vez de
      pesos uniformes o el proxy de corte L5→L8 usado hasta ahora.

- **2026-07-17 (4)**: **E1 cerrado — pesos WLR reales por satélite,
  implementados y validados contra el WLR real del paper**. Nuevo módulo
  `strandline-core::satellite_weights` + subcomando CLI `strandline
  satellite-weights` + flag `rates --satellite-weights`.
  - **Diseño**: *detrend* OLS por transecto, residuos agrupados por
    satélite pooled entre TODOS los transectos (más potencia estadística
    que calibrar transecto por transecto), peso = `1/varianza`. WLR es
    invariante a la escala de los pesos (`weighted_regression` en
    `rates.rs`: la pendiente y su IC95 solo dependen de proporciones
    relativas, se verificó algebraicamente) — no hace falta normalizar.
    CLI: `satellite-weights` toma `intersections` + `dates` (con columna
    `satname` nueva) y escribe `satname,n_obs,std_m,weight`; `rates
    --satellite-weights <csv>` reemplaza la columna `weight` de `dates`
    por el peso de la tabla cuando el satélite de la observación está en
    ella (fallback al peso de `dates`/1.0 si no — no rompe el flujo
    existente).
  - **Validado**: 4 tests nuevos (satélite más ruidoso recibe menor peso,
    ratio de varianza recuperado dentro de tolerancia amplia sobre datos
    sintéticos con tendencias distintas por transecto; transecto bajo el
    mínimo se ignora; tendencia fuerte no contamina el residuo; caso
    vacío). 81 tests totales (77+4), `extract_integration` y
    `pixel_locking` OK, clippy limpio.
  - **Validación real, la que importa**: se calibraron pesos sobre los
    770 días reales de Viña (satname real de `/mnt/kingston`) y se
    comparó el WLR resultante contra `rates_corrected` del pickle del
    paper (que sí usó satname real en su propio pipeline). **Mediana del
    error |WLR_calculado − WLR_paper| bajó de 0.0552 a 0.0231 m/año
    (más de 2× mejor) usando pesos reales en vez de uniformes, y el WLR
    con pesos reales quedó más cerca del paper en 137/178 transectos
    (77%)** — cierra el hallazgo E1 con evidencia directa, no solo
    plausibilidad.
  - **Nota honesta sobre la magnitud del efecto**: el ratio L8-vs-resto
    calibrado por `satellite-weights` (detrend OLS simple, por
    observación cruda, sin deseasonalizar) da std≈115m (L8) vs
    std≈127-137m (L5/L7/L9) — razón de varianza ~1.4×, notablemente más
    chico que el ~3× encontrado en la adenda (3) vía residuo STL
    (agregado mensual, promediado ACROSS transectos, con estacional ya
    removido). No es una discrepancia — son residuos distintos: el
    detrend simple por transecto retiene todo el ruido de intersecciones
    múltiples/georreferenciación local (dominante en magnitud, ya
    documentado como causa del SCE~500m), mientras que promediar
    across-transectos por mes cancela ese ruido idiosincrático y expone
    mejor el componente COMPARTIDO (inter-sensor). Ambos son válidos; el
    ratio más chico aquí es esperable dado qué residuo se usa, y aun así
    el efecto en el WLR final es real y medible (2× mejor vs el paper).
    Dirección consistente en ambos métodos: L8 siempre el más preciso.

- **2026-07-17 (5)**: **Extensión a los otros 3 sitios con satname en
  `/mnt/kingston` (Coquimbo, Constitución, Valdivia) — resultado MIXTO,
  no una mejora universal**. Reusando directamente `cross_distance_clean`
  de los `{SITE}_analysis.pkl` (sin re-extraer nada), se calibraron pesos
  y se comparó el WLR resultante contra `rates_all` (ground truth del
  paper) igual que con Viña:
  ```
                mediana error (pesos reales vs uniforme)   % transectos donde pesos reales ganan
  Viña          0.023 vs 0.055 m/año  (2.4× mejor)          77%  (ya reportado)
  Constitución  0.060 vs 0.315 m/año  (5.2× mejor)          88%
  Coquimbo      0.158 vs 0.024 m/año  (6.6× PEOR)           16%
  Valdivia      0.385 vs 0.291 m/año  (1.3× peor)           32%
  ```
  **Conclusión honesta: 2 de 4 sitios mejoran fuerte (Viña, Constitución),
  2 empeoran (Coquimbo, Valdivia)**. Se investigaron dos hipótesis para la
  asimetría, ninguna la explica limpiamente:
  - Presencia de terremoto (`earthquake_date`): Coquimbo (Illapel 2015) y
    Constitución (Maule 2010) TIENEN salto sísmico documentado — pero uno
    mejora mucho y el otro empeora mucho. No es el factor.
  - Densidad temporal (escenas/año): Coquimbo tiene la menor densidad
    (9.7/año vs 18-30/año en los otros 3) — podría explicar por qué el
    detrend OLS por transecto en Coquimbo específicamente es más ruidoso/
    menos confiable (menos puntos para estimar la tendencia), pero
    Valdivia tiene densidad similar a Viña (18.4 vs 18.9/año) y aun así
    empeora — no explica Valdivia.
  - **Causa real: sin identificar todavía.** El mecanismo (calibrar y
    aplicar pesos reales) funciona correctamente en los 4 sitios — el
    problema es que el detrend OLS *lineal simple* por transecto (sin
    deseasonalizar, sin manejar saltos no-lineales como sismos, pooled
    directo sobre observaciones crudas) no aísla de forma confiable el
    componente de precisión-por-sensor en todos los sitios; en algunos
    queda contaminado por otra cosa (posiblemente el mismo tipo de ruido
    de intersecciones múltiples que ya se sabe domina el SCE, o eventos
    reales no lineales mal atribuidos a un satélite por coincidencia
    temporal).
  - **Recomendación**: `rates --satellite-weights` es una herramienta
    correcta y validada (el mecanismo, los tests, el caso de Viña/
    Constitución), pero **no aplicar a ciegas** — validar sitio por sitio
    contra un WLR de referencia antes de confiar en el resultado
    calibrado, exactamente como se hizo acá. No asumir que "más real que
    el proxy" implica "siempre mejor" sin verificar.
  - Script ad hoc de esta validación (no guardado como archivo del
    repo, construido inline en la sesión): reusa `cross_distance_clean`
    + `{SITE}_output.pkl` de `/mnt/kingston` para satname + `rates_all`
    del `analysis.pkl` como ground truth — replicable si se retoma.

- **2026-07-18**: **Causa real de la asimetría, identificada — el "ground
  truth" contra el que se validaba es él mismo una heurística arbitraria,
  no una medición**. Se releyó el código Python original del paper
  (`scripts/02_transects_rates.py`, usado para Viña; `scripts/
  13_site_analysis.py`, usado genéricamente para Coquimbo/Constitución/
  Valdivia) y ambos calculan `WLR_m_yr` con la MISMA tabla de pesos fija,
  reciclada idéntica en los 6 sitios sin calibrar nada por sitio:
  ```python
  w = 1/15 si L5/L7, 1/10 si L8/L9, 1/7 si otro (S2)
  ```
  Reproducir esa fórmula exacta da error ~0.0000-0.0004 m/año contra
  `WLR_m_yr` en los 4 sitios — confirma que es literalmente la fórmula que
  generó el "ground truth", no una precisión real medida por sitio.
  - Comparando esa heurística contra la varianza real calibrada
    (`satellite_weights::calibrate`, satname real de `/mnt/kingston`):
    ```
                  orden real de std (m)                          ¿dirección coincide con heurística?   magnitud del ratio
    Viña          L8=115 < L9=128 < L7=135 < L5=137              sí                                     ~1.4x
    Constitución  L9=147 < L8=151 < L7=178 ≈ L5=178              sí                                     ~1.5x
    Coquimbo      L8=44  < L9=46  < L7=49  < L5=80               sí                                     ~3.3x  (grande)
    Valdivia      L5=126 < L8=147 < L9=153 < L7=171               NO — L5 es el más preciso, L7 el peor  ~1.8x
    ```
  - Se probó también si el problema era de forma funcional (`calibrate()`
    usa peso=1/varianza; el paper usa 1/σ). Cambiar a 1/σ mejora mucho
    Coquimbo (error 0.160→0.064 m/año) pero empeora Constitución
    (0.054→0.200) — es un contribuyente real pero no la causa raíz.
  - **Conclusión**: el calibrador no está mal — el punto de comparación sí
    es arbitrario. Calibrar con datos reales solo "gana" contra el
    heurístico cuando el ruido real por satélite del sitio coincide en
    dirección y magnitud con ese supuesto global de 15/15/10/10/7:
    - Viña/Constitución: coinciden en dirección y la magnitud real es
      modesta → calibrar ayuda.
    - Coquimbo: coincide en dirección pero la magnitud real (3.3×) excede
      mucho lo que la heurística asume → el peso cuadrático (1/var)
      sobrecorrige y se aleja del heurístico.
    - Valdivia: la dirección misma está invertida (L5 no es el peor sensor
      ahí) → cualquier calibración real diverge del heurístico casi por
      definición.
  - Dato adicional: el R² mediano de la tendencia lineal por transecto es
    bajísimo en los 4 sitios (0.02-0.06) — la mayor parte del "residuo"
    atribuido a satélite es en realidad ruido de otra fuente
    (intersecciones múltiples, marea), no precisión de sensor pura. Limita
    cuánto puede decir este método sobre precisión real por satélite.
  - **Recomendación revisada**: no perseguir el WLR heurístico del paper
    como ground truth para validar `--satellite-weights` — no es
    independientemente correcto, es la misma suposición reciclada en los 6
    sitios. La adenda anterior ("no aplicar a ciegas, validar sitio por
    sitio") sigue vigente, pero el criterio de validación debe cambiar: no
    "¿se acerca al WLR del paper?" sino algo que mida precisión real
    (p. ej. held-out split, o los tests sintéticos ya validados en
    `satellite_weights.rs`). Cierra la pregunta abierta de la asimetría —
    no queda como misterio pendiente.
  - Script ad hoc de esta sesión (no guardado como archivo del repo):
    reusa los mismos insumos de la adenda anterior + relectura de
    `scripts/02_transects_rates.py` y `scripts/13_site_analysis.py` para
    extraer la fórmula de pesos original del paper.

- **2026-07-18 (2)**: **Búsqueda dirigida al corpus Pardo-Pascual/UPV
  (SHOREX/SAET) sobre transectos dinámicos vs fijos — laguna cerrada, con
  matiz importante para el paper**. La investigación SOTA de la adenda de
  2026-07-17 (2) no había penetrado este corpus; ahora sí (búsqueda
  dirigida + snippets convergentes; los full-text de ScienceDirect/MDPI
  dan 403, pero el hallazgo central aparece consistente en múltiples
  fuentes independientes).
  - **Hallazgo central**: SHOREX (Pardo-Pascual et al. 2018, *Coastal
    Engineering*, "An efficient protocol for accurate and massive
    shoreline definition...") usa deliberadamente **una única línea de
    referencia aproximada FIJA para todo el conjunto de imágenes** —
    textual: "replacing the pixel level lines... by a *unique approximate
    line for the whole set of images*, which can be obtained either from a
    pre-existing cartographic source or from a coarse photo-interpretation
    on an orthophoto close in time to the studied period". Es el diseño
    OPUESTO al hallazgo propio de la adenda 10 (mini-transectos frescos por
    escena > transectos fijos). Pero es una **decisión de eficiencia**
    (evitar thresholding por imagen → co-registro + procesamiento masivo
    automático), no una afirmación de que la geometría fija sea más exacta.
  - **Matiz que evita sobre-reclamar**: la línea fija de SHOREX es una
    *semilla de extracción* (ventana de búsqueda donde el kernel subpixel
    encuentra la posición real por imagen), NO un transecto de cálculo de
    tasas estilo DSAS. Mi hallazgo es sobre el paso de tasas/refinamiento
    (CoastSat-lineage + `subpixel-refine` propio). Comparación análoga, no
    idéntica — no decir que SHOREX "está mal".
  - **El "adaptive" de UPV NO es la línea**: en Sánchez-García et al. 2019
    ("A New Adaptive Image Interpolation Method...", *Remote Sensing*
    11(16):1880) lo adaptativo es el **kernel/ventana de interpolación**
    (combinaciones de 16 o 36 píxeles de mayor variación radiométrica),
    reemplazando el stencil fijo 7×7 de Pardo-Pascual et al. 2012 (*RSE*).
    La geometría de la línea de referencia sigue siendo fija. Confirmado.
  - **El otro linaje (CoastSat/CoastSeg/Shoreline Monitor/Castelle 2024/
    USGS) también usa transectos fijos** ("Baseline and Transects", típico
    50 m) + suavizado espacial post-hoc para costas complejas — ya
    documentado en la adenda SOTA, reconfirmado aquí (2024-2026: CoastSeg,
    Long Island, Atlántico europeo, todos fijos; ninguna regeneración
    por-escena).
  - **Nadie aísla el error geométrico**: en ambos linajes el error en
    costa compleja se atribuye a *spectral mixing* (SHOREX: rocoso,
    vegetado, meso-macrotidal) o a intersecciones múltiples/no-linealidad
    (CoastSat), y se maneja con QC/suavizado — **NO** con regeneración de
    la geometría del transecto. Nadie compara fijo-vs-por-escena ni
    cuantifica la *desalineación shore-normal del transecto fijo* como
    fuente de error distinta. Esa es la laguna real.
  - **Framing correcto para el paper C&G** (reemplaza el pendiente de
    "buscar si existe literatura"): NO reclamar que "transectos frescos por
    escena" sea una idea nueva — la geometría de referencia fija es el
    estándar deliberado en ambos linajes. SÍ reclamar, correctamente
    acotado: *aislamos y cuantificamos una fuente de error geométrico (la
    desalineación shore-normal del transecto fijo) que los enfoques
    estándar de geometría fija arrastran en silencio, y mostramos que
    regenerar la geometría por escena la reduce en costas complejas/
    dinámicas*.
  - **Consistencia interna que fortalece el argumento**: el hallazgo propio
    (adenda 10) mostró que la geometría fija/sembrada solo GANA en Viña/
    Arica (costas rectas simples) y PIERDE en las 4 complejas. SHOREX
    valida su línea fija justo en playas microtidales rectas del
    Mediterráneo (Cala Millor, Valencia) — exactamente el tipo de costa
    donde mi hallazgo dice que la geometría fija está bien. Los dos
    resultados son consistentes; el aporte propio extiende el cuadro a
    costas complejas donde SHOREX no se validó primariamente. Honesto y
    defendible.
  - Referencias clave del corpus (para el .bib del paper): Pardo-Pascual
    et al. 2012 (*RSE*, subpixel 7×7 fijo); Sánchez-García et al. 2017 /
    2019 (*RS* 11(16):1880, kernel adaptativo); Pardo-Pascual et al. 2018
    (*Coastal Eng.*, SHOREX, línea única fija); Palomar-Vázquez et al.
    2023 (*RS* 15(12):3198, SAET); Sánchez-García/Cabezas-Rabadán et al.
    2023 (*Coastal Eng.*, assessment SAET).
