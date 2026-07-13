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
