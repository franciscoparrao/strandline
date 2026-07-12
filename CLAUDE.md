# Strandline — Engine Rust de análisis de shorelines satelitales

## Qué es

Engine compilado de extracción y análisis de líneas de costa desde imágenes
satelitales. El "CoastSat/DSAS en Rust" de la familia de engines del autor
(SurtGIS, Smelt, Forge, Nowcast, Hydroflux, Anvil, Cantus, Criterium).

**Nombre**: `strandline` ("strandline" = línea de costa en inglés antiguo).
Verificado disponible en crates.io (2026-07-10; `strand` y `swash` tomados).

**Estado**: SPIKE DE VIABILIDAD (semana 1). Nada construido aún.

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
  - **Próximo V1**: item 8 (tasas DSAS: EPR/LRR/WLR/NSM/SCE con
    incertidumbre) — consume el `cross_distance` de la intersección, aquí se
    conecta también `tide::correct_chainage`.
