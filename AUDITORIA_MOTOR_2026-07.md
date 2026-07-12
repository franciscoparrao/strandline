# Auditoría del motor Strandline — Julio 2026

**Versión auditada**: v0.1.0 (main @ 62373d7, V1 completo)
**Alcance**: ~3.600 LOC de Rust en 2 crates (strandline-core: 11 módulos;
strandline-cli). Auditoría en 5 dimensiones: correctness numérica, robustez
de entrada, paridad con CoastSat, cobertura de tests, performance.
**Método**: relectura completa del core con ojos frescos buscando bugs (no
asumiendo correcto lo validado); verificación empírica de 2 bugs sospechosos
ejecutando código y comparando contra pyTMD; generación de vectores golden
desde sklearn para el MLP.

---

## Resumen ejecutivo

El motor está en un estado de validación numérica **excepcional para su
edad**: cada etapa del pipeline tiene comparación golden contra su
implementación de referencia (0.3 m vs CoastSat en extracción, 1.15 mm vs
pyTMD en marea, 0 mismatch vs el pickle del paper en transectos/tasas/QC).
Ese es el patrón de test que la auditoría de SurtGIS identificó como el que
faltaba allá — aquí es la fortaleza fundacional.

El patrón débil es el **inverso al de SurtGIS**: la validación contra
referencia es ejemplar, pero la robustez ante entradas fuera del camino
feliz es inmadura. La auditoría encontró **2 bugs verificados
empíricamente** (1 panic, 1 dato incorrecto silencioso) y varios riesgos de
la misma clase:

1. **`generate_transects` hace panic** (índice fuera de rango) si la línea
   base es más corta que el spacing — verificado: `transects/mod.rs:107`,
   `index out of bounds` con línea de 50 m y spacing de 100 m.
2. **El constituyente Sa tiene el argumento astronómico equivocado**
   (silencioso): usa `V0 = h` cuando pyTMD/GOT usan `V0 = h − ps` —
   verificado: G_pyTMD = 6.31° vs mío 289.60° en MJD 58858.6, un error de
   fase de ~283° en la componente anual. Sa quedó fuera del test de
   predicción total (se testeó Ssa pero no Sa); exactamente el tipo de
   hueco que un test "por lista completa" habría cazado.

Ambos quedan corregidos en el mismo commit de esta auditoría, con tests.

---

## PARTE 1 — Bugs verificados

### B1. Panic en `generate_transects` con línea base corta ⚠️ VERIFICADO → CORREGIDO
`strandline-core/src/transects/mod.rs:107`.

`resample_by_arclength` con `spacing > largo_total` devuelve 1 punto; el
cálculo de tangente en `i == 0` accede a `samples[1]` → panic. Un usuario
con una playa de bolsillo (< 100 m) y los defaults del CLI recibe un crash.

**Fix aplicado**: con 1 solo punto re-muestreado, la tangente se toma de la
línea base completa (primer→último punto) y se emite 1 transecto. Test nuevo.

### B2. Argumento astronómico de Sa incorrecto (dato silencioso) ⚠️ VERIFICADO → CORREGIDO
`strandline-core/src/tide/harmonic.rs` (tabla `PRINCIPAL_CONSTITUENTS`).

Doodson de Sa estaba `[0,0,1,0,0]` (V0 = h); la convención GOT/pyTMD — la
misma con la que se calibraron los otros 12 constituyentes y contra la que
se valida el motor — es `[0,0,1,0,-1]` (V0 = h − ps). Con el argumento
equivocado, un usuario que cargue amplitud/fase de Sa desde un modelo
estándar obtiene la componente anual con fase esencialmente aleatoria
(error ~283°). Amplitud típica de Sa: 3–6 cm en Chile central → hasta
~0.5–1 m de error horizontal sistemático estacional con slope 0.10.

**Por qué el test no lo cazó**: el test de predicción total usaba 12
constituyentes y omitía justamente Sa. Meta-lección: los tests golden deben
cubrir la lista completa de lo que la tabla declara soportar.

**Fix aplicado**: Doodson corregido + Sa agregado al test de predicción
total (nueva referencia pyTMD con 13 constituyentes, error máx sigue < 3 mm).

---

## PARTE 2 — Riesgos de datos silenciosos y paridad (no corregidos aquí; backlog priorizado)

### R1. `parse_timestamp` ignora offsets de zona horaria no-UTC (M)
`tide/csv.rs::strip_timezone`. `2020-01-10T14:33:52-03:00` se interpreta
como UTC (documentado, pero silencioso). Error de 3 h de marea ≈ hasta
±0.3 m vertical ≈ ±3 m horizontal — invisible para el usuario.
**Fix propuesto**: aplicar el offset (es trivial: ya se parsea el sufijo) o
rechazar con error explícito los timestamps con offset ≠ 0.

### R2. Sin buffer de nubes (`dist_clouds`) en la extracción (M, paridad)
CoastSat elimina los puntos de shoreline a < 300 m de píxeles nube
(`process_shoreline`); strandline no. En escenas parcialmente nubladas los
bordes de nube pueden colarse como falsos positivos de costa. El spike no lo
vio porque usó solo escenas < 1% nube.
**Fix propuesto**: distancia-a-nube por chamfer/BFS sobre la máscara QA
(barato) + filtro de puntos del contorno.

### R3. Índice NIR hardcodeado en modo compat (B)
`shoreline.rs`: `let nir = &bands[3]` mientras `green_band`/`swir1_band` son
configurables — si el usuario redefine el orden de bandas, el modo
`--coastsat-compat` usa la banda equivocada sin aviso.
**Fix propuesto**: agregar `nir_band` a `ExtractParams`.

### R4. Sin guardia de CRS (M)
Las escenas del mismo sitio llegan en zonas UTM distintas (32618/32619,
observado en el spike). `intersect` asume shorelines y transectos en el
mismo CRS y `min_length_m` asume unidades métricas; con CRS geográfico o
mixto produce basura silenciosa.
**Fix propuesto**: reproyección integrada a un EPSG de salida (surtgis-core
la tiene) + rechazo explícito de CRS geográfico en `extract`.

### R5. GeoJSON no conforme a RFC 7946 (B)
Se escriben coordenadas proyectadas con el EPSG en `properties`. Pragmático,
pero QGIS y muchas librerías asumen WGS84. Ligado a R4.

### R6. Taxonomía de errores (B)
`StrandlineError::ShapeMismatch` se usa como cajón de sastre para errores de
CSV/parseo. Agregar variantes `Io`/`Parse` antes de publicar la API.

### R7. Intersección usa línea infinita, no segmento (documentar)
Réplica fiel de CoastSat (los puntos cuentan hasta `max_origin_dist` del
origen aunque estén más allá del extremo del transecto). No es bug, pero
sorprenderá a usuarios; documentar en el rustdoc de `IntersectionSettings`.

---

## PARTE 3 — Cobertura de tests

43 tests tras esta auditoría (40 previos + 3 nuevos). Fortaleza: golden
numérico contra referencia externa en otsu/contour/astro/harmonic/rates/qc.
Huecos (en orden de valor):

1. ~~`mlp`: sin vectores golden~~ → **corregido aquí** (4 vectores desde
   sklearn: water→3, whitewater→2, other→0 ×2).
2. `index::nd_index`: sin tests (fill zeros, -inf, QA bits).
3. `shoreline::extract_shoreline`: sin test de integración en CI — el
   fixture de escena vive fuera del repo. Propuesta: recorte GeoTIFF de
   64×64 px (~50 KB) commiteado + golden de umbral y n líneas.
4. `classify_pixels`: solo se testea `remove_small_objects`.
5. Property-based (proptest) para `contour` (todo segmento generado se
   ensambla; ningún punto fuera de la grilla) y `despike` (idempotencia).
6. Fuzzing de los parsers CSV (surtgis ya tiene la infraestructura).

---

## PARTE 4 — Performance

Estado actual medido: extracción sin clasificador ~18 ms/escena (~65× CoastSat);
con clasificador ~0.17 s (~6×); intersección ~197× vs Python vectorizado.
Sin regresiones detectadas en la relectura. Palancas restantes conocidas:

- Las 10 `local_std` de `FeatureBank::build` corren en serie entre sí
  (cada una paralela por dentro) — `rayon::join`/scope las solaparía.
- `nan_stats` ordena para la mediana (O(n log n)); `select_nth_unstable`
  la baja a O(n). Irrelevante hoy (60 ms totales), relevante a escala
  nacional (10⁴ transectos × 10³ shorelines).
- La lectura del GeoTIFF (~10-25 ms) domina el modo sin clasificador;
  streaming por strips de surtgis-core evitaría materializar bandas
  completas si algún día importa.

Ninguna es urgente: el presupuesto de velocidad del paper ya se cumple.

---

## PARTE 5 — Propuestas de avance nivel SOTA

Ordenadas por (impacto para el paper C&G) × (viabilidad con el ecosistema
propio). Las tres primeras son las que moverían a strandline de "CoastSat
compilado" a "estado del arte".

### S1. Extracción subpixel por ajuste de perfil (anti pixel-locking) ★★★
El marching squares sobre MNDWI hereda el sesgo de *pixel-locking* de los
métodos de contorno (los cruces se acumulan cerca de los centros/bordes de
píxel). El estado del arte (SHOREX/SAET, Pardo-Pascual & Almonacid-Caballer)
ajusta una función analítica (erf/sigmoide) al perfil del índice de agua a
lo largo de cada normal y reporta ~3–5 m RMSE absoluto con Landsat, contra
los ~7–10 m del enfoque CoastSat (Vos et al. 2023). strandline ya tiene las
normales (transectos) y los perfiles (la franja de intersección): el ajuste
erf por perfil es un mínimos-cuadrados 1D de 4 parámetros — barato, puro
Rust, paralelizable. **Es el avance con mejor razón novedad/esfuerzo y da
una figura de accuracy propia para el paper** (comparación de 3 métodos
sobre las mismas escenas: CoastSat, strandline-compat, strandline-erf).

### S2. Procesamiento cloud-native sin descarga (COG/STAC streaming) ★★★
`surtgis-cloud` ya tiene `cog_reader` y modelos STAC. Conectarlo permite
procesar directamente desde Planetary Computer/USGS sin bajar escenas:
`strandline extract --stac <query>` → el claim del paper ("40 años × 1000 km
en minutos") se vuelve demostrable end-to-end por cualquiera, sin GEE ni
descarga previa. Ningún challenger tiene esto; CoastSat depende de GEE.
Esfuerzo medio (la infraestructura existe), impacto de posicionamiento alto.

### S3. Segmentación DL opcional vía ONNX embebido ★★☆
El MLP de 20 features es 2018; el estado del arte de clasificación costera
son los modelos del Doodleverse (CoastSeg/Segmentation Zoo, Buscombe et al.).
Portar un modelo pequeño a ONNX y correrlo con `tract` (runtime ONNX puro
Rust, sin libtorch) mantiene el binario único sin dependency hell. Beneficio
real donde el MLP falla: agua turbia, espuma extensa, sombras de acantilado.
Riesgo: tamaño del binario y validación seria — proponer como feature
opcional (`--model zoo.onnx`), no default.

### S4. Pendiente de playa desde el satélite (port de CoastSat.slope) ★★☆
Hoy `beach_slope` es un valor a mano (0.10). Vos et al. (2020, GRL) estiman
la pendiente por transecto maximizando la coherencia espectral entre la
serie de shoreline y la marea. strandline tiene todas las piezas (series por
transecto + predictor de marea nativo): es un periodograma + búsqueda 1D.
Convierte la corrección mareal en autocalibrada por transecto — mejora
directa de las tasas del paper de Geomorphology al re-procesarlo.

### S5. Incertidumbre por punto y tasas robustas ★★☆
El scope V1 declaraba "σ por punto (georef + pixel + tidal)" y quedó
pendiente. Propuesta en dos niveles: (a) propagación analítica de σ a cada
chainage (incluye sensibilidad al umbral: ∂x/∂t ≈ 1/|∇MNDWI| a lo largo de
la normal — se obtiene gratis del perfil de S1) y WLR pesado por esa σ real
en vez del proxy por satélite; (b) conformal prediction con `smelt-ml`
(plan V2 existente) para intervalos sin supuestos distribucionales, y una
LRR robusta (Theil-Sen o Student-t) que reemplace el despiking ad-hoc por
un estimador con punto de quiebre conocido. Da la sección de "uncertainty
quantification" que los reviewers de C&G piden cada vez más.

### S6. Sentinel-2 y fusión multi-misión ★☆☆
Soporte S2 (10 m, bandas B2/B3/B4/B8/B11, s2cloudless) triplica la densidad
temporal post-2015. El benchmark de Vos et al. 2023 muestra sesgos
inter-sensor sistemáticos: la fusión requiere co-registro y corrección de
sesgo por misión (estimable con las escenas solapadas L8/S2). Necesario para
el claim de escala, pero es más volumen de trabajo que novedad.

### S7. Analítica temporal más allá de DSAS ★☆☆
Las tasas lineales son la línea base de 2009. Con las series por transecto
ya en memoria: descomposición estacional (STL), detección de puntos de
cambio (PELT/BEAST) para separar erosión crónica de eventos (marejadas,
El Niño — conecta con el análisis ENSO del paper de Geomorphology), y
hotspots espaciales (Getis-Ord Gi* a lo largo de la costa). Features de
dashboard tipo MONCOSTA pero open-source y con estadística seria.

### S8. Corrección de wave setup/runup ★☆☆
Stockdon et al. (2006) con olas de ERA5: `η ≈ f(H₀, L₀, β)`. Segundo orden
respecto a la marea en Chile central pero relevante en eventos; declarado
fuera de V1, correcto mantenerlo en V2. Depende de S4 (β por transecto).

### S9. Distribución: strandline-py (PyO3) + demo WASM ★★☆
Para adopción y para el paper: bindings PyO3 (drop-in donde hoy se usa
CoastSat para el análisis) y demo en navegador (surtgis ya tiene pipeline
WASM) con una escena precargada. El reviewer puede *ejecutar* el engine en
su browser — argumento de reproducibilidad inusualmente fuerte.

### Secuencia sugerida
1. **v0.2 (hardening)**: fixes de PARTE 2 (R1–R6) + fixture de integración
   + proptest. Cierra la deuda de robustez antes de que entren usuarios.
2. **v0.3 (SOTA core)**: S1 (erf subpixel) + S4 (slope satelital) + S5a
   (σ por punto). Con eso el paper tiene método propio, no solo port rápido.
3. **v0.4 (escala)**: S2 (STAC streaming) + S6 (Sentinel-2) → benchmark
   nacional 1000 km reproducible.
4. **Paper C&G** tras v0.3: software + 3-way accuracy + velocidad + caso
   Chile. S3/S7/S8/S9 según tiempo y feedback de reviewers.

---

## Fixes aplicados en este commit

- B1: `generate_transects` ya no hace panic con línea base corta (tangente
  de respaldo desde la línea completa) + test.
- B2: Doodson de Sa corregido a `[0,0,1,0,-1]` + Sa incorporado al test de
  predicción total con nueva referencia pyTMD (13 constituyentes, error máx
  < 3 mm).
- Tests golden del MLP: 4 vectores generados desde el sklearn original
  (water→3, whitewater→2, other→0 ×2).

---

## ADENDA (2026-07-12) — v0.2 hardening: R1, fixture CI y segundo sitio

### R1 corregido
`parse_timestamp` ahora **aplica** los offsets de zona (`Z`, `±HH:MM`,
`±HHMM`, `±HH`) y **rechaza** sufijos malformados en vez de interpretarlos.
Tests con offsets chilenos (−03/−04) y con minutos (+05:30).

### Fixture de integración en CI
Recorte real de 96×96 px de la escena L8 2020-01-10 (playa de Reñaca, ~50%
agua, ~200 KB) commiteado a `strandline-core/tests/fixtures/` + 2 tests de
integración con golden values (umbral, n líneas, conteos de clasificación
exactos, bounding box). Primer test en CI que ejercita el camino completo
GeoTIFF → shoreline.

### Validación en segundo sitio: Constitución (hardening empírico)
Bahía curva, desembocadura del Maule, terremoto 2010, **con Sentinel-2**
(53–63 fechas/año post-2016) y criterios QC relajados — geometría y
configuración totalmente distintas a Viña. Resultados contra
`CONSTITUCION_analysis.pkl` / `CONSTITUCION_tidal_corrected.pkl`:

| Etapa | Resultado |
|-------|-----------|
| Generación de transectos (176, costa curva) | **exacta** (err máx 0.0000 m) |
| NSM/SCE/EPR/LRR/R²/IC95 (174 transectos) | exactas (~5e-5 / ~5e-7) |
| QC (umbrales relajados vía flags del CLI) | **0 mismatch** (93 excl. / 81 ret. idéntico) |
| Flujo mareal → LRR corregido (78 transectos) | exacto (~5e-7) |
| WLR | ver E1 |

### E1. El proxy de pesos por fecha del WLR solo vale para sitios Landsat-only
El único desajuste del segundo sitio (WLR err máx 0.56 m/año) se diagnosticó
por completo: **el estimador es correcto** (recomputado en numpy con pesos
idénticos coincide con Rust a 5e-5, el redondeo de exportación); la
diferencia viene de que Constitución incluye Sentinel-2 (peso 1/7 en el
paper) y el `satname` no se retuvo en el pickle, así que el proxy
"1/15 antes de 2013-04, 1/10 después" usado por el harness de validación no
puede reproducir los pesos reales. No es un bug del motor —
`strandline rates` acepta el peso real por shoreline en la columna `weight`
del CSV de fechas — pero sí una **trampa de uso documentable**: en sitios
multi-misión el usuario debe pasar pesos por satélite reales, no confiar en
cortes temporales.

**Conclusión del hardening empírico**: el motor no tuvo ningún bug nuevo en
una geometría/configuración completamente distinta; el único hallazgo fue
del harness de validación. 46 tests (44 unitarios + 2 integración).

---

## ADENDA 2 (2026-07-12) — v0.2 completo: R2–R7 cerrados

Todo el backlog de robustez de la PARTE 2 quedó implementado:

- **R6**: variantes `Io`/`Parse`/`UnsupportedCrs` en `StrandlineError`;
  `ShapeMismatch` ya no se usa como cajón de sastre.
- **R3**: `nir_band` configurable en `ExtractParams` + `--nir-band`.
- **R7**: semántica de línea-infinita documentada en `IntersectionSettings`.
- **R2** (el grande): módulo `cloud` con máscara de nubes limpia
  (`remove_small_objects` 4-conectividad, min 40 px — elimina las falsas
  nubes de swash de CFMASK) usada por todo el pipeline, y buffer
  `dist_clouds` (default 300 m, paridad CoastSat) con EDT euclídea exacta
  (Felzenszwalb, O(n), validada contra fuerza bruta) que excluye puntos de
  shoreline cerca de nubes partiendo las polilíneas. **Efecto medido en las
  6 escenas del spike (modo compat)**: mediana 0.30 → **0.20 m**, media
  7.71 → **2.93 m**, p90 2.71 → **1.20 m**, y la costa quedó
  des-fragmentada (3–9 líneas/escena vs 9–37). El fixture de integración
  ahora produce 1 línea continua en vez de 2 trozos.
- **R4+R5**: módulo `proj` — transformación UTM↔WGS84 nativa (series de
  Krüger/Karney orden n⁴, sin dependencia de PROJ), **validada contra
  pyproj a <1 mm** en puntos del dominio del paper (incluye el caso GEE de
  zona norte con northing negativo). Guardia que rechaza rasters en CRS
  geográfico con error explícito. `Shoreline::reproject` + `--output-epsg`:
  con 4326 el GeoJSON es RFC 7946 conforme; proyectado lleva el member
  `crs` legacy. **Validación end-to-end**: extraer con `--output-epsg
  32719` (reproyección 100% Rust) reproduce la mediana de 0.20 m del flujo
  con pyproj.

53 tests (51 unitarios + 2 integración), clippy limpio, paralelo y
single-thread. Queda de la PARTE 3: fixture ✔; pendientes proptest/fuzzing.

---

## ADENDA 3 (2026-07-12) — v0.3 SOTA-core: S1 + S4 + S5a implementados

### S1 — Extracción subpixel por ajuste de perfil (`subpixel`)
Muestreo bilineal del MNDWI a lo largo de cada transecto + ajuste
Levenberg-Marquardt del modelo de borde `a + b·tanh((x−x₀)/w)`; la línea de
agua es el cruce del modelo con el umbral, con σ analítica y sensibilidad
al umbral (`∂x/∂t = w/b`). CLI: `strandline profile`.

**Evidencia (experimento sintético con la maquinaria real de producción en
ambos brazos, transecto diagonal, frontera barrida a través de 1 píxel,
ruido σ=0.08, 250 casos)**:

| Flujo | RMSE | Sesgo |
|-------|------|-------|
| contorno + intersección (CoastSat/DSAS) | 1.61 m | **−0.78 m** |
| **ajuste de perfil (S1)** | **0.53 m** | +0.02 m |

3× menos RMSE y sesgo eliminado. En la escena real de Viña:
|perfil−contorno| mediana 1.15 m — exactamente el orden del sesgo+ruido que
el sintético atribuye al contorno. (|contorno−CoastSat| = 0.02 m es réplica
del mismo algoritmo, no exactitud.)

**Hallazgo E2**: el método de perfil **expone transectos torcidos** que la
intersección por franja tolera en silencio — con los transectos del paper
(línea base zigzag por binning multivaluado) solo 18/250 perfiles cruzaban;
con el flujo auto-referenciado (extract → línea base propia →
`transects --smooth-window` → profile), **69/69**. Se agregó
`smooth_polyline` (media móvil por longitud de arco) al generador.

### S4 — Pendiente de playa satelital (`slope`)
Lomb-Scargle nativo + barrido de pendientes candidatas minimizando la
energía espectral mareal residual (principio de Vos et al. 2020 GRL).
CLI: `strandline slope`.

**Validación**: recupera el slope verdadero a ±0.01 en sintéticos con
muestreo Landsat + marea armónica propia + tendencia + ruido 3 m (test), y
a un paso de grilla (0.08 → 0.0825, 99% de energía mareal removida) sobre
las **770 fechas reales de Viña con la marea GOT5.5 real**.

**Hallazgo E3 (límite de aplicabilidad, cuantificado)**: con ruido
posicional ≥5 m el estimador se sesga hacia pendientes menores (5 m→0.065,
8 m→0.060, 12 m→0.053 para slope_true 0.08), y en los datos reales de Viña
(micro-tidal, señal mareal horizontal ~±3.7 m vs scatter Landsat ~10 m) los
óptimos caen en los bordes del rango → **no identificable con solo-Landsat
en micro-tidal**, consistente con la advertencia de Vos et al. (rango
mareal ≥~1 m). Aplicable con Sentinel-2 (S6) o en sitios macro-tidales
(Chiloé). Resultado de paper por sí mismo.

### S5a — σ por punto
Integrada en `profile`: `σ² = σ_fit² + σ_georef² + (∂x/∂t·σ_t)²` con
`σ_fit` de la covarianza del ajuste y `∂x/∂t` del propio modelo de borde
(`--georef-rmse`, `--threshold-sigma`). Los pesos reales para el WLR salen
de aquí (columna `weight` de `rates`).

60 tests (57 unitarios + 3 de integración), clippy limpio, ambos modos.

---

## ADENDA 4 (2026-07-12) — v0.4 escala: S2 (STAC/COG streaming) + S6 (Sentinel-2)

### S2 — Adquisición cloud-native sin descarga (`strandline fetch`)
Nuevo subcomando (feature `cloud`, default; desactivable para un binario
mínimo) sobre la infraestructura de `surtgis-cloud`: búsqueda STAC en
Planetary Computer (firma SAS automática) y lectura de **solo la ventana**
del área de interés vía HTTP range requests sobre los COGs. Produce pares
`*_ms.tif` (5 bandas orden CoastSat, reflectancia f32) + `*_mask.tif`
compatibles con todo el pipeline validado.

**Demostrado end-to-end**: la ventana de Viña de la escena
LC08_233083_20200110 son **~870 KB transferidos** (229×234 px) contra ~1 GB
de la escena completa; `fetch` → `extract --output-epsg 32719` corre sin
GEE ni descarga previa.

**Hallazgo E4**: el clasificador MLP portado de CoastSat **no transfiere a
reflectancia superficial** (fue entrenado con TOA de GEE): sobre
landsat-c2-l2 clasifica ~68% de la ventana como sand y el umbral compat
colapsa. Para el flujo MPC el modo correcto es el Otsu refinado
(radiométricamente agnóstico): mediana 9.35 m vs la referencia CoastSat —
que es una comparación **inter-producto** (SR 30 m vs TOA 15 m), ≈0.3 px,
dentro de la exactitud absoluta del método (~7–10 m, Vos 2023). Reentrenar
el clasificador sobre SR queda como trabajo para el paper.

### S6 — Sentinel-2 (`--collection sentinel-2-l2a`, `--mask-kind scl`)
B02/B03/B04/B08 a 10 m + B11 (20 m) re-muestreado bilinealmente a la grilla
de 10 m; máscara desde la SCL (clases 3/8/9/10 + filtro de blobs, módulo
`cloud::cloud_mask_from_scl`). ~~Limitación documentada: el offset BOA de
−0.1 del processing baseline 04.00 (escenas > 2022-01-25) no se aplica
aún.~~ **Cerrado (2026-07-12, adenda 5)**: offset aplicado según
`s2:processing_baseline` del item STAC (≥ 04.00 → DN−1000), con fallback
por fecha de adquisición si la property falta. Validado con escena real
2024 de Viña (baseline 05.10): reflectancia de agua consistente con la
escena 2020 (green 0.026 vs 0.020, MNDWI 0.445 vs 0.415) — sin el fix
habría quedado green≈0.126 y MNDWI≈0.07 (contraste colapsado en
silencio). 3 tests unitarios nuevos.

**Validación**: S2B 2020-01-09 vs L8 2020-01-10 (ambos SR de MPC, 1 día de
diferencia, marea distinta): **mediana 6.29 m** de consistencia
inter-misión — dentro de lo reportado por el benchmark de Vos 2023.
Sentinel-2 además habilita el slope satelital en micro-tidal (hallazgo E3
de la adenda 3): la densidad S2 baja el ruido efectivo de la serie.

60 tests, clippy limpio (2 warnings restantes son de surtgis-cloud
upstream), compila con y sin la feature `cloud`.
