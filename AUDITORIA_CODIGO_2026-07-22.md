# Auditoría de código — strandline (2ª pasada, 2026-07-22)

> **ESTADO: los 5 HIGH y los 8 MEDIUM están CORREGIDOS y validados**
> (2026-07-22 los 5 HIGH + 6 MEDIUM; 2026-08-03 los dos restantes, M4 y M5,
> que requerían re-validar resultados ya citados). 103 tests verdes (era 89) en modo paralelo y
> single-thread, clippy limpio, goldens de paridad intactos salvo uno
> actualizado a propósito (ver M3a). Verificaciones end-to-end con el binario
> real, no solo unit tests. Detalle por hallazgo abajo, marcado `[CORREGIDO]`
> / `[PENDIENTE]`.
>
> **Ninguna cifra publicada del paper cambia**: la única corrección de
> paridad que altera output (`ceil` vs `round`) es un no-op con los settings
> del estudio de referencia (4500 m² → cociente entero exacto a 15 y 30 m);
> solo afecta al default propio del motor (500 m²) a 15 m, que es el fixture
> de CI.

**Scope**: workspace completo (`strandline-core` 5.981 líneas, `strandline-cli`
1.873 líneas). Segunda auditoría con ojos frescos tras
`AUDITORIA_MOTOR_2026-07.md` (2026-07-11); cubre todo lo agregado desde
entonces (cloud, proj, subpixel, slope, stl, satellite_weights, baseline,
fetch). Metodología: clippy + suite (ground truth), grafo de dependencias,
5 revisiones profundas paralelas por subsistema con **verificación posterior
de cada hallazgo mayor contra el código** (y contra la fuente real de
CoastSat `SDS_shoreline.py` para los de paridad). Un hallazgo se verificó
además **empíricamente** (probe de determinismo ejecutado 3×).

## Resumen

- **Critical: 0 | High: 5 | Medium: 8 | Low: ~18** (deduplicados por causa raíz)
- Calidad general: **Buena→Excelente en el núcleo numérico validado; la deuda
  se concentra en las fronteras de entrada (CSV/CLI) y en los módulos
  post-V1 que no pasaron por la validación exhaustiva contra pickles**
  (satellite_weights, stl, slope, subpixel, fetch).
- Estado base: 89 tests verdes, clippy limpio (1 warning preexistente en
  `otsu.rs` por drift de versión; 2 en surtgis-cloud, fuera de scope), 0
  TODOs fósiles, 2 `#[allow]` justificados, dependencias todas en uso, grafo
  de módulos unidireccional sin ciclos.

## Lo que está bien (preservar)

1. **Paridad como contrato verificable**: la tabla de marching squares es
   transcripción exacta de skimage (verificada caso por caso, incluidas
   sillas y `fully_connected='low'`); los empates de Otsu usan el mismo
   primer-argmax; las divergencias deliberadas (guard `MAX_ABS_VALUE`, Otsu
   balanceado sin RNG) están documentadas en el código con su justificación.
2. **Validación armónica con tolerancias honestas**: `harmonic.rs` separa V0
   (<0.01°) de factores nodales con justificación cuantitativa por
   constituyente, y el bug B2 (Sa) quedó fosilizado como comentario para no
   reintroducirse. Las series de Karney y el Newton inverso son correctos.
3. **Disciplina numérica en kernels**: acumulación f64 en `std_at`/histograma
   Otsu, clamp de varianza (evita el bug de NaN que CoastSat sí padece), EDT
   de Felzenszwalb con test contra fuerza bruta, LM robusto a NaN por
   construcción (paso patológico → SSE NaN → rechazo → λ×10).
4. **Tests que codifican modos de falla reales**: la meseta espuria de Viña,
   la inversión E5, las heurísticas de baseline descartadas — memoria
   institucional ejecutable, no goldens decorativos. Readers CSV por nombre
   de columna con `anyhow::Context` en casi todo el CLI.

---

## Hallazgos HIGH

### [H1] [CORREGIDO] Política sistémica de descarte/default silencioso en la frontera CSV del CLI
- **Ubicaciones**: `strandline-cli/src/main.rs:461-466, 493-500, 522-528,
  553-565, 613-625, 651-657` (readers) y loops de consumo `1143-1157` (slope),
  `1237-1258` (rates), `1333-1343` (stl), `1397-1411` (satellite-weights).
  Casos agravados: corrección mareal faltante → `unwrap_or(0.0)`
  (`main.rs:1253`); peso no parseable → `unwrap_or(1.0)` (`main.rs:560-563`,
  y `"inf"`/`"nan"` parsean como válidos); timestamps no parseables en `tide`
  descartados sin eco del original (`main.rs:903-923`, desalinea joins por
  orden de fila).
- **Dimensión**: D5 Error handling.
- **Impacto**: fila mala, fecha en otro formato, o id sin match → la
  observación desaparece o toma un default **sin contador ni warning**. Un
  `dates.csv` con 200 de 770 fechas en formato distinto produce tasas sobre
  el subconjunto restante reportadas como completas. En Chiloé (macro-tidal),
  mezclar observaciones corregidas y sin corregir (0.0 silencioso) mete
  metros de ruido indistinguible. Es la misma clase de bug que R1.
- **Fix**: helper único de lectura CSV con contadores de descartes por causa
  + `eprintln!("X de Y filas descartadas: …")` + flag `--strict` que aborte.
  Un solo helper cierra toda la familia.

### [H2] [CORREGIDO] NaN envenena la calibración de pesos por satélite, en silencio
- **Archivo**: `strandline-core/src/satellite_weights.rs:74-111` (+ CLI
  `main.rs:1402-1406` que no filtra).
- **Dimensión**: D5 / correctitud.
- **Cadena verificada**: el CSV de `intersect` contiene NaN **por diseño**
  (marca de QC) → `"NaN".parse::<f64>()` es `Ok(NaN)` → `ols` produce slope
  NaN (stt finito, el guard no atrapa) → TODOS los residuos del transecto son
  NaN → contaminan el pool de cada satélite presente → `var.is_nan()` →
  `continue` → satélites desaparecen de la tabla → `rates
  --satellite-weights` hace fallback silencioso a peso 1.0. El usuario cree
  que calibró y corre con pesos uniformes.
- **Fix**: filtrar `is_finite()` en t/y al construir la serie (y saltar el
  residuo correspondiente), con warning si se descartó algo.

### [H3] [CORREGIDO] Tasas NaN pasan los 5 criterios de QC y se clasifican "Stable"
- **Archivos**: `strandline-core/src/rates.rs:130-135` +
  `strandline-core/src/qc.rs:72-86, 100-108`.
- **Dimensión**: D5 / correctitud.
- **Verificado**: `compute_rates` filtra solo `chainage[i].is_finite()` — un
  NaN en `times_years` entra al fit y vuelve NaN lrr/years/r2. En `evaluate`
  toda comparación con NaN es false → el transecto **pasa los 5 criterios**
  (retenido); `classify(NaN)` → `Stable`. Degradación silenciosa en la
  dirección peligrosa: retener basura.
- **Fix**: filtrar también `times_years[i].is_finite()`; en `evaluate`,
  cortar primero si `!rates.lrr.is_finite()`.

### [H4] [CORREGIDO] fetch: integridad del batch no garantizada
- **Archivo**: `strandline-cli/src/fetch.rs:259-266, 278-280, 291-307,
  314-333 vs 335-363, 357-375` + `main.rs:764-774`.
- **Dimensión**: D5 / robustez operacional.
- **Cuatro defectos verificados**: (a) fallo total del batch (token SAS
  expirado, red caída) → todas las escenas se omiten y el proceso sale con
  **exit code 0** y "0 escenas"; (b) la **reanudación fija para siempre un
  par corrupto**: chequea solo `exists()` y escribe ms antes que mask — un
  SIGKILL a mitad de la mask deja ambos archivos presentes y toda corrida
  futura la salta ("ya existe") con la máscara truncada; además el skip suma
  `written += 1` (conteo inflado); (c) el recorte a shape mínimo común se
  aplica a las bandas pero **no a la máscara Landsat** → par ms/mask con
  dimensiones inconsistentes que revienta en `extract` después; (d) el mismo
  `sign_asset_href` aborta el batch en el loop de bandas (`?`) pero omite la
  escena en el closure de máscara — dos políticas para el mismo error
  sistémico.
- **Fix**: escribir a `*.tmp` + rename atómico (mask primero, ms al final);
  exit ≠ 0 si `written == 0 && !items.is_empty()`; recortar la máscara al
  mismo shape; unificar la política de firma (fallo de firma = sistémico →
  abortar).

### [H5] [CORREGIDO] Contratos de entrada no forzados en `tide` (doc-only) → corrupción silenciosa o panic
- **Archivos**: `strandline-core/src/tide/mod.rs:38-57`;
  `strandline-core/src/tide/csv.rs:82-101, 165-176, 189-202`.
- **Dimensión**: D5.
- **Verificado**: (a) `TideCorrection::new` no valida `beach_slope` (el "debe
  ser positivo" es solo doc) → con 0.0 la serie completa sale ±inf/NaN y
  fluye a rates sin error; alcanzable por typo en `--beach-slope` o por un
  slope estimado degenerado (E3 documenta el sesgo a la baja). (b) Cadena
  NaN del CSV: `"nan"` como segundos → MJD NaN → **panic** en
  `sort_by(partial_cmp().unwrap())` (csv.rs:101); elevación `"NaN"`
  (convención estándar de huecos en exports de gauges/pyTMD) → entra a la
  interpolación en silencio. (c) Slicing de bytes con entrada no-ASCII
  (mojibake Latin-1) → panic por char boundary (csv.rs:189, 198-202).
  (d) Componentes fuera de rango (mes 13, hora 25) aceptados
  aritméticamente; minutos ilegibles → 0 silencioso; constituyentes
  duplicados se suman dos veces; filas truncadas desaparecen sin conteo.
- **Fix**: `Result` en `new` con validación de finito y >0; validar rangos y
  finitud en `parse_timestamp`; `total_cmp` en el sort; rechazar elevaciones
  no finitas (o saltar con conteo); operar sobre bytes ASCII en
  `split_timezone`; detectar constituyentes duplicados.

## Hallazgos MEDIUM

### [M1] [CORREGIDO] Panics alcanzables por parámetros públicos (6 ubicaciones, 1 causa raíz: sin guard de dominio)
`slope.rs:110` (serie no creciente → `dts` vacío → index panic — verificado);
`rates.rs:137-142` (`min_valid=0` + 0 obs finitas → underflow usize —
verificado); `transects/mod.rs:53` (`spacing<=0` → `inf as usize + 1`
overflow, comportamiento distinto debug/release); `slope.rs:205`
(`candidates` vacío); `slope.rs:152` (`assert_eq!` de largos en vez de
error); todos alcanzables vía API pública o flags CLI. **Fix**: guards
tempranos que devuelvan `None`/`Err`.

### [M2] [CORREGIDO] `calibrate` no es determinista bit-a-bit — VERIFICADO EMPÍRICAMENTE
`satellite_weights.rs:79`: itera `HashMap::values()` (RandomState) → orden de
acumulación flotante cambia por proceso. Probe ejecutado 3×: los pesos
difieren en el último ulp entre corridas. Rompe la promesa explícita del
proyecto (y del paper §2.7) de resultados bit-idénticos, incluso
single-thread. **Fix**: iterar claves ordenadas (el patrón `BTreeMap` ya
existe en `stl.rs`).

### [M3] [PARCIAL] Divergencias de paridad CoastSat no documentadas (4, verificadas contra `SDS_shoreline.py`)
(a) `shoreline.rs:319,411`: `round()` donde CoastSat usa `np.ceil`
(SDS_shoreline.py:138) — con `min_beach_area/px²` fraccional el filtro puede
**saltarse por completo** (guard `>1` de classify.rs:85); los defaults son
enteros exactos, por eso las validaciones no lo vieron. (b)
`classify.rs:54-60`: píxel con feature NaN → `other`; CoastSat lo clasifica
rellenando 1e-9 (línea 360-366). (c) Falta el fallback de CoastSat cuando hay
<50 px de arena (línea 195): strandline falla o deja que 3 píxeles espurios
dominen el Otsu balanceado. (d) Falta el filtro de puntos a <30 m de nodata
de `process_shoreline` (línea 695) — relevante ahora que `fetch --sensors
etm` entrega L7 SLC-off en volumen (franjas de gap). **Fix**: (a) es un
one-liner (`ceil`); (b-d) replicar o documentar como divergencia deliberada.
**Nota estructural para el paper**: CoastSat umbraliza solo dentro del buffer
de la reference shoreline dilatado; strandline usa toda la escena — debe
decirse en Methods.

### [M4] [CORREGIDO] STL: bins de 30.44 días, no meses calendario
`stl.rs:53-57`: `(frac*12).floor()` sobre año decimal → 2020-01-01 cae en el
bin "diciembre 2019"; 2020-03-01 comparte bin con 2020-02-29 (verificado
numéricamente por el agente). El doc promete "mes calendario" — falso en
bordes (±2 días). No invalida el STL (periodo 12 se mantiene) pero sí el
cruce por mes con series externas. **Repercusión honesta**: el análisis ENSO
de la adenda (3) cruzó residuo STL vs ONI por mes — el corrimiento de borde
mete ruido en los lags; improbable que voltee r≈-0.04, pero re-correr con
meses calendario antes de citarlo en un paper. **Fix**: derivar el mes del
calendario civil real desde MJD.

### [M5] [CORREGIDO] `fit_edge` retorna `Some` aunque LM nunca haya aceptado un paso
`subpixel.rs:180-267`: si las 30 iteraciones se rechazan, devuelve el init
intacto con σ calculada en ese punto — un `ProfileFit` de apariencia
legítima indistinguible del fallback honesto (`sigma_m=NaN`). Contamina
justo los σ que alimentan los pesos WLR (S5a del paper). **Fix**: flag
`improved`; si nunca mejoró el SSE, retornar `None` (con semilla cae al
fallback correcto).

### [M6] [CORREGIDO] `slope`: dos degradaciones silenciosas en el estimador del paper
(a) `slope.rs:205-214`: mínimo del barrido en el borde de la grilla
(0.02/0.30) se reporta como válido — el síntoma clásico de E5 queda
silenciado; (b) `slope.rs:121-137`: marea constante (columna rellenada con
0.0) → empate total → retorna `Some(0.02, ratio=1.0)` plausible en vez de
`None`. **Fix**: campo `at_boundary` + exigir varianza de marea > 0.

### [M7] [CORREGIDO] `read_transects_csv`: el único reader posicional, y descarta los nombres
`main.rs:570-587`: header saltado a ciegas, parseo por índice (contra la
convención por-nombre del resto), `parse()?` sin contexto de archivo/línea, y
la columna `name` se bota — `intersect`/`profile` renumeran `T001…` por
posición: un CSV de transectos editado (borrar T042) **corre todos los
nombres siguientes** y las tasas quedan atribuidas al transecto equivocado,
en silencio. **Fix**: resolver por nombre, preservar `name`, contexto en
errores.

### [M8] [PARCIAL] Formato de shorelines CSV frágil + eslabones faltantes del pipeline
(a) `main.rs:609-626`: fragmentos agrupados por **contigüidad** de
`shoreline_id` — un CSV reordenado (Excel/sort) fusiona fragmentos con
segmentos fantasma que generan intersecciones espurias sin aviso. (b) La
salida de `tide` (keyed por mjd) no la consume `rates`/`slope` (esperan
`shoreline_id`), y el CSV que espera `intersect` no lo produce ningún
subcomando — el glue ad hoc del usuario es donde muerden los descartes de
H1. **Fix**: columna `part_id` opcional + `tide --ids` + `extract
--output-csv`.

## Hallazgos menores (tabla compacta)

| Archivo:línea | Dim | Sev | Resumen |
|---|---|---|---|
| proj.rs:73-101 | D5 | LOW | Sin guardia de dominio en Δλ (singular en ±90°; error silencioso creciente con EPSG equivocado) |
| proj.rs:125 | D5 | LOW | Longitud inversa sin normalizar a [-180,180] en antimeridiano (GeoJSON RFC 7946 inválido) |
| proj.rs:73,104 | D5 | LOW | `zone` sin validar en funciones `pub` (zone=0 → CM −183° silencioso) |
| proj.rs:129-138 | D5 | LOW | Newton sin señal de no-convergencia (converge en la práctica; `debug_assert` documentaría) |
| astro.rs:94-119 | D6 | LOW | Convenciones no documentadas (UT1≈UTC, gregoriano proléptico) + atribución errónea (dice Fliegel–Van Flandern, es Meeus) |
| csv.rs:64,80 | D5 | LOW | Números de línea de error incorrectos con líneas en blanco intercaladas |
| rates.rs:192-203 | D5 | LOW | `r²` sin clamp a [-1,1] (scipy sí clampa) → CI95 NaN con series casi colineales |
| rates.rs:209-240 | D5 | LOW | Pesos ≤0/no finitos no rechazados en WLR; peso 0 infla grados de libertad |
| rates.rs:203,239 | D6 | LOW | 1.96 fijo vs t-Student — convención heredada del paper (validada vs pickle); documentar, no cambiar sin re-validar |
| transects/mod.rs:132 | D5 | LOW | Empate de `seaward_hint` (dot≈0) puede flippear orientación entre transectos vecinos en bahías en C |
| transects/mod.rs:126 | D5 | LOW | `tlen==0 → continue` desalinea numeración T00k vs posición alongshore |
| baseline.rs:88-94 | D5 | LOW | Empate exacto de score lo gana el último según orden de archivos de entrada |
| subpixel.rs:100-116 | D5 | LOW | Geotransform rotado no verificado; transecto origin==end → NaN empaquetado como fit "válido" |
| subpixel.rs:507-524 | D6 | LOW | `refine_line_subpixel` puede devolver línea vacía, contradiciendo su contrato doc |
| main.rs (readers) | D5 | LOW | BOM UTF-8 de Excel rompe detección de columnas con mensaje engañoso ("falta 'X'" cuando está) |
| main.rs (outputs) | D5 | LOW | Escrituras no atómicas + sobrescritura silenciosa; disk-full deja CSV parcial que parsea "bien" |
| fetch.rs:407-411 | D7 | LOW | Test trivial (constante contra su propio literal); el filtro SLC-off real sin test directo |
| fetch.rs:161-198 | D5 | LOW | Cap STAC heurístico aún supera-ble con filtros estrictos; `truncate` retiene siempre las escenas más antiguas sin aviso |
| main.rs:959-967 | D3 | LOW | `baseline` hardcodea parámetros que `extract` expone como flags (no reproducible con settings custom) |
| otsu.rs:30-33 | — | NIT | Binning multiplicativo vs bordes linspace de numpy (bit-parity residual) |
| shoreline.rs:457-460 | — | NIT | Buffer de nubes: vértice redondeado vs distancia exacta (~≤0.7 px solo en la frontera de 300 m) |
| error.rs:13 | D6 | NIT | Mensaje `Io` dice "reading" aunque la variante cubre escritura |

## Arquitectura

Limpia y coherente con el propósito: grafo unidireccional (`error` en la
base; `shoreline` como fachada con 7 imports internos, esperable; sin
ciclos), sin módulos cajón de sastre, features (`parallel`, `cloud`) bien
factorizadas, separación core/CLI correcta (numérica en core, I/O y parsing
en CLI). Dos observaciones: (1) la frontera CSV del CLI es hoy el eslabón
débil del sistema — merece el helper común de H1 como componente de primera
clase; (2) el patrón de determinismo correcto (`BTreeMap`) ya existe en
`stl.rs` — aplicarlo en `satellite_weights` es consistencia interna, no
diseño nuevo.

## Deuda técnica

Mínima: 0 TODOs fósiles, 0 código muerto detectado, 2 `#[allow]`
justificados, deps todas en uso y pinned por workspace. El único warning
propio de clippy (`manual Range::contains`, otsu.rs) es drift de versión de
clippy ya conocido.

## Recomendaciones priorizadas

1. **Helper CSV con contadores + `--strict`** (cierra H1 y parte de H5/M7 de
   una vez) — el fix de mayor apalancamiento del reporte.
2. **Guards de finitud en el núcleo post-V1**: `satellite_weights` (H2),
   `compute_rates`/`evaluate` (H3) — dos filtros `is_finite` y un
   short-circuit; antes de volver a usar `--satellite-weights` en datos
   reales.
3. **fetch atómico y honesto** (H4): tmp+rename, exit≠0 en fallo total,
   recorte de máscara — antes de cualquier fetch masivo nocturno.
4. **Contratos de `tide`** (H5) y **guards anti-panic** (M1) — validación de
   entrada barata, sin tocar numérica validada.
5. **Determinismo de `calibrate`** (M2) — 3 líneas, restaura la promesa del
   paper §2.7.
6. **Paridad**: `ceil` (one-liner) + decidir replicar-o-documentar las otras
   3 divergencias (M3) — importa directamente para el claim de "port fiel"
   del paper C&G; la nota del reference-buffer va a Methods.
7. **STL con meses calendario** (M4) y re-verificación del cruce ENSO antes
   de citarlo.
8. `fit_edge` con flag de convergencia (M5) y `slope` con `at_boundary`/guard
   de varianza (M6) — antes de usar σ y slope en el benchmark del paper.

**Nota de calibración**: los módulos validados contra pickles (extract,
intersect, rates camino feliz, qc camino feliz, tide síntesis, proj) están
sólidos — ninguna de las validaciones exactas reportadas en la bitácora ni
las cifras del paper está comprometida por estos hallazgos. El riesgo vive
en los caminos con NaN, en inputs degenerados/editados, y en los módulos
nuevos sin pickle de referencia.

---

## Apéndice: qué se corrigió (2026-07-22, misma sesión)

### Cambios en `strandline-core`

| Módulo | Cambio | Hallazgo |
|---|---|---|
| `satellite_weights.rs` | Filtro de finitud en `t`/`y` antes del detrend; iteración por claves ordenadas + `BTreeMap` para el pool de residuos | H2, M2 |
| `rates.rs` | `compute_rates` exige `times_years[i].is_finite()`; `min_valid.max(2)` cierra el underflow de `t[n-1]` | H3, M1 |
| `qc.rs` | `evaluate` cortocircuita tasas no finitas (antes las **retenía**); `classify` documenta el caso NaN | H3 |
| `tide/mod.rs` | `TideCorrection::new` → `Result` validando `beach_slope` finito y > 0; **campos privados** + getters, para que no se pueda saltar la validación con un struct literal | H5 |
| `tide/csv.rs` | Rangos de fecha/hora validados; `nan`/`inf` rechazados; `split_timezone` opera sobre bytes ASCII (fin del panic por mojibake); minutos/segundos ilegibles ya no valen 0; elevaciones no finitas y filas truncadas son error; constituyente duplicado rechazado; BOM UTF-8 tolerado; números de línea físicos | H5 |
| `slope.rs` | Serie reordenada internamente + guard de gaps positivos (fin del panic por índice); largos incompatibles y `candidates` vacío → `None`; varianza de marea nula → `None`; campo nuevo `at_boundary` | M1, M6 |
| `transects/mod.rs` | Guard de `spacing <= 0` / no finito (fin del overflow `inf as usize + 1`) | M1 |
| `shoreline.rs` | `min_beach_area` a píxeles con `ceil`, como CoastSat | M3a |
| `error.rs` | Variante `InvalidParameter`; mensaje de `Io` ya no dice "reading" para escrituras | M1, L |

### Cambios en `strandline-cli`

| Cambio | Hallazgo |
|---|---|
| `CsvDiscards`: contabilidad de descartes por causa, con primer valor ofensor, reportada por stderr al cerrar cada lectura | H1 |
| Flag global `--strict`: convierte cualquier descarte en error (exit 1) | H1 |
| `csv_header()`: BOM UTF-8 de Excel tolerado en todos los readers | L |
| Aplicado a los 6 readers y a los 4 loops de consumo (rates, stl, slope, satellite-weights) | H1 |
| `rates --tide-correction`: observación sin corrección se **excluye** y se cuenta, en vez de entrar sin corregir mezclada con las corregidas | H1/H4 |
| `read_transects_csv`: columnas por nombre (era el único posicional), preserva `name` del CSV, errores con archivo+línea; `intersect`/`profile` usan ese nombre en vez de renumerar por posición | M7 |
| `read_shorelines_csv`: avisa si un `shoreline_id` reaparece tras cerrarse (CSV reordenado → fragmentos fusionados con segmentos fantasma) | M8 |
| Pesos ≤ 0 / no finitos rechazados en las tablas de peso | L |
| `tide`: ecoa el timestamp original junto al MJD (un join posicional ya no se corre si una fila falla) y cuenta las descartadas | M6-CLI |
| `slope`: columna `at_boundary`; los óptimos de borde se excluyen de la mediana y se avisan | M6 |
| `fetch`: escritura atómica `.tmp` + rename (mask primero); recorte de la máscara al shape común; contadores separados de escritas/omitidas/fallidas; **error si todo el batch falló**; fallo de firma abortando en ambos sitios | H4 |

### Validación de los fixes

- **99 tests** (era 89): 91 unitarios + 5 de fetch + 2 de integración + 1 de
  pixel-locking. Verdes en modo paralelo y `--no-default-features`
  (single-thread determinista). Clippy limpio en todo el código propio.
- **Tests nuevos que capturan cada defecto**: NaN aislado que no borra
  satélites; calibración bit-idéntica ante dos `HashMap` con distinto orden;
  tasas no finitas rechazadas por QC; fechas NaN y `min_valid=0` sin panic;
  `beach_slope` no positivo rechazado; componentes de fecha fuera de rango y
  `nan`/`inf`; entrada no-ASCII sin panic; `read_csv` ante marcas de hueco;
  constituyente duplicado; entradas degeneradas de `slope`.
- **Determinismo probado empíricamente**: el mismo probe que detectó el
  defecto (5 corridas independientes del binario) ahora da salida
  **bit-idéntica**; antes difería en el último ulp.
- **End-to-end con el binario real** (no solo unit tests): `extract` en ambos
  modos sobre el fixture; cadena `transects → intersect → rates --qc` y
  `satellite-weights` sobre datos sintéticos; y los caminos de fallo —
  `dates.csv` con formato mixto reporta *"se descartaron 1 de 2 filas — fecha
  no parseable"* y *"15 de 22 — shoreline_id sin fecha"* (antes: silencio
  total), `--strict` sale con código 1, y `--beach-slope 0` es rechazado con
  mensaje explícito.
- **Goldens de paridad**: intactos salvo el de clasificación del fixture
  (sand 354 → 352), actualizado a propósito por el fix `ceil`, con la
  justificación completa escrita en el test.

### Pendientes (no corregidos)

- **M3b-d** — las otras tres divergencias con CoastSat (NaN→`other` vs relleno
  1e-9; fallback con <50 px de arena; filtro de 30 m a nodata). Decisión
  pendiente: replicarlas o documentarlas como deliberadas en Methods. La nota
  del *reference buffer* va al paper igual.
- **M8b** — eslabones faltantes del pipeline (`tide --ids`, `extract
  --output-csv`) para que los subcomandos se encadenen sin glue externo.
- Los LOW de la tabla compacta (guardas de dominio en `proj`, clamp de `r²`,
  t-Student, empates de `seaward_hint`, escrituras atómicas en el resto de
  outputs).

---

## Segunda tanda (2026-08-03): M4 y M5

Los dos que quedaban requerían re-validar resultados ya citados, así que se
hicieron con su verificación explícita.

### M4 — meses calendario en STL

`abs_month` invierte el año decimal al día que lo originó (la conversión de
`decimal_year_from_mjd` es exactamente invertible sobre días enteros) y toma
el mes civil real, con un calendario gregoriano propio (algoritmo de Hinnant,
round-trip verificado sobre 80.000 días consecutivos). `StlResult` expone
además `year_month: Vec<(i32, u32)>` y el CSV del CLI ahora escribe
`year,month` junto al año decimal: el join contra series mensuales externas
deja de depender de un flotante.

**Re-verificación del cruce ENSO** (el resultado citado en la adenda del
2026-07-17, que se había calculado con el binning defectuoso). Se rehízo
completo: 72 transectos QC-retenidos de Viña, 20.129 observaciones, 491 meses
de residuo promediado, contra el ONI del propio `enso_analysis.pkl`.

| | binning 30.44 d (antes) | meses calendario (ahora) |
|---|---|---|
| lag 0 | r = −0.038 (p = 0.46) | r = −0.071 (p = 0.17) |
| mejor \|r\| | lag +12, r = −0.109 | lag −7, r = −0.131 (p = 0.011) |
| varianza explicada | 1.2 % | 1.7 % |

**La conclusión se sostiene**: ENSO explica como máximo ~1.7 % de la varianza
del residuo y en lag 0 no es significativo — sigue descartado como
explicación de la baja `seasonal_variance_frac` de Viña. Dato que refuerza la
corrección: el mejor lag pasó de +12 meses (residuo *siguiendo* al ONI un año
después, físicamente extraño) a −7 meses, mucho más cerca del −4 que había
encontrado el análisis independiente del propio paper (r = −0.129) — es
decir, arreglar el binning acercó este análisis al del paper en vez de
alejarlo.

### M5 — `fit_edge` sin convergencia

Se distingue "no convergió" de "el init ya era estacionario": se registra si
algún paso fue aceptado y, si ninguno lo fue, se compara la norma infinito
del gradiente `Jᵀr` en el init contra la escala de los residuos. Sin pasos
aceptados y con gradiente no nulo → `None`, que en el camino sembrado activa
el fallback honesto (la semilla, con `sigma_m = NaN`) en vez de reportar una
σ calculada sobre parámetros que nunca se ajustaron.

**Verificación de que no toca nada publicado**:
- `pixel_locking` (el test que respalda la cifra de accuracy del paper) da
  métricas **idénticas**: RMSE 1.61 → 0.53 m, sesgo −0.78 → +0.02 m, n = 250.
  Los 250 ajustes de ese experimento convergían de verdad.
- Comparación end-to-end del subcomando `profile` sobre el fixture, con y sin
  el fix compilado: **54/54 transectos con chainage bit-idéntico** (diferencia
  máxima 0.0 m) y ningún fallback nuevo. Es esperable por construcción —
  `init[2]` es la semilla, así que el fallback devuelve el mismo chainage que
  antes devolvía el "ajuste" no convergente; lo único que cambia es que la σ
  espuria pasa a ser `NaN`.

103 tests (era 99), verdes en paralelo y single-thread, clippy limpio.
