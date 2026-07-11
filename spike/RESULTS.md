# Spike de viabilidad — Resultados (2026-07-10 → 2026-07-11)

## Veredicto: **GO** ✅

Con el clasificador de píxeles portado end-to-end (MLP de CoastSat + Otsu
balanceado sand/water, modo `--coastsat-compat`), la diferencia mediana
strandline↔CoastSat es **0.30–0.35 m** — 15× bajo el criterio GO (< 5 m),
sin ningún valor calculado a mano en Python. La maquinaria geométrica
(MNDWI + marching squares subpixel + georeferenciación convención CoastSat)
reproduce el contorno de referencia a nivel subpíxel.

**Actualización 2026-07-11**: se completó el port del clasificador MLP de
CoastSat a Rust (`strandline-core::classify`, `mlp`, `features`) + Otsu
balanceado (`otsu::balanced_otsu`) + flag `--coastsat-compat`. Los
resultados de exactitud de esta sección ya no dependen de umbrales
calculados manualmente en Python (sección "Umbral igualado" más abajo,
conservada como referencia histórica) — el binario Rust solo, sin ayuda
externa, reproduce CoastSat a 0.3 m de mediana. Detalle del port más abajo.

## Setup

- **Sitio**: Viña del Mar / Reñaca (mismo polígono del paper de Geomorphology).
- **Datos**: 14 escenas L8 ene–abr 2020 re-descargadas vía GEE (pipeline
  CoastSat, project 1052232428380); 6 despejadas (<1% nube) usadas.
- **Referencia**: CoastSat `extract_shorelines` con `pan_off=True` (ambos
  engines ven el mismo ms tif de 15 m — comparación engine-vs-engine sin
  pansharpening de por medio), EPSG:32719.
- **Métrica**: distancia punto-a-línea simétrica dentro de un corredor
  costero de 150 m (replica el QC `max_dist_ref` que CoastSat usa en
  producción; ambos engines extraen también lagunas/blobs no costeros que
  ese buffer descarta).

## Resultados de exactitud

| Modo | Mediana | Media | p90 |
|------|---------|-------|-----|
| Otsu global propio (standalone, sin clasificador) | 5.7–6.0 m | 6.6–15 m | 11–34 m |
| Umbral igualado a mano en Python (histórico, ver abajo) | 0.38–0.47 m | 0.7–9.2 m | 1.5–17 m |
| **`--coastsat-compat` (MLP + Otsu balanceado, Rust puro)** | **0.30–0.35 m** | 0.6–7.7 m | 1.2–2.7 m |

Por escena (`--coastsat-compat`, ref→rust): 0.02–1.03 m de mediana
(`spike/results/comparison_classifier_compat.json`).

### Port del clasificador de píxeles (2026-07-11)

Se portó el pipeline completo de clasificación de CoastSat a Rust puro,
sin dependencias de Python en runtime:

- **`strandline-core::mlp`**: forward pass del MLP `NN_4classes_Landsat_new.pkl`
  (20 features → Dense(100, ReLU) → Dense(4, softmax); se omite el softmax
  final porque es monótono y el argmax de los logits basta). Pesos
  exportados a JSON (`spike/scripts/04_export_classifier.py`) y embebidos
  en el binario vía `include_str!` (52.9 KB) — sin dependencia de sklearn.
- **`strandline-core::features`**: réplica de `calculate_features` (20
  features: 5 bandas + 5 índices NIR−G/SWIR−G/NIR−R/SWIR−NIR/B−R + 10
  desviaciones estándar locales 3×3 NaN-aware con padding por reflexión,
  equivalente funcional del `astropy.convolve` con `nan_treatment=
  'interpolate'` que usa CoastSat).
- **`strandline-core::classify`**: clasificación pixel a pixel + filtro
  morfológico `remove_small_objects` (8-conectividad, min_size en píxeles)
  sobre las máscaras sand/water — soluciona además la fragmentación de
  costa por swash marcado como nube (hallazgo de la sección anterior).
- **`strandline-core::otsu::balanced_otsu`**: Otsu con las dos clases
  pesadas a igual peso total (1.0 cada una), réplica funcional
  determinística del submuestreo aleatorio de CoastSat — sin RNG, converge
  al mismo umbral para N grande.
- **Validación del port**: conteos de píxeles clasificados en la escena
  2020-01-10 — Rust: sand=2915, whitewater=732, water=36215; Python
  (CoastSat real): sand=2886, whitewater=751, water=36270 (Δ < 1.5%).
  Los umbrales efectivos del modo `--coastsat-compat` (calculados 100% en
  Rust) coinciden con los valores hallados manualmente en Python a
  ±0.002 (ver tabla de umbrales más abajo).

## Hallazgo clave: el umbral de CoastSat no es el Otsu del MNDWI

Al perseguir la brecha de ~5 m del modo standalone se encontró que en
`find_wl_contours2` (SDS_shoreline.py, **presente también en el master
actual de kvos/CoastSat**, verificado 2026-07-10):

```python
im_ind = np.stack((im_wi, im_mwi), axis=-1)   # col 0 = NDWI, col 1 = MNDWI
...
t_mwi = filters.threshold_otsu(int_all[:,0])  # ← umbral del NDWI (col 0)
t_wi  = filters.threshold_otsu(int_all[:,1])  # ← umbral del MNDWI (col 1)
...
contours_mwi = measure.find_contours(im_mwi_buffer, t_mwi)  # MNDWI con umbral NDWI
```

Es decir, **CoastSat contornea la imagen MNDWI con el umbral Otsu calculado
sobre el NDWI (NIR−G)** — las columnas están intercambiadas respecto a los
nombres. Verificación numérica (Viña, textbook convention G−SWIR):

| Escena | Otsu global (strandline) | Otsu MNDWI sand+water balanceado | Umbral efectivo CoastSat (quirk) |
|--------|--------------------------|----------------------------------|----------------------------------|
| 2020-01-26 | 0.235 | 0.271 | 0.135 |
| 2020-04-15 | 0.265 | 0.246 | 0.138 |

El Otsu global de strandline coincide con el Otsu "correcto" del MNDWI
(±0.03); la línea de CoastSat está desplazada ~5 m hacia tierra por el
umbral NDWI. Un barrido de umbral fijo confirmó que la distancia mínima
(0.65–1.4 m) se alcanza exactamente en el umbral del quirk (~0.14–0.15).

**Implicancias**:
- El "error" de 5 m del modo standalone no es un defecto de strandline:
  es la diferencia entre el MNDWI-Otsu fiel y el comportamiento real
  (accidental o no) de CoastSat.
- Para compatibilidad bit-a-bit, V1 necesitaba el clasificador de píxeles +
  Otsu balanceado sand/water + un flag `--coastsat-compat` que replique el
  quirk. **Completado 2026-07-11** — ver sección "Port del clasificador"
  arriba.
- Material para el paper de C&G: una reimplementación independiente
  detecta divergencias del reference implementation (cf. sección de
  validación). Redactar con cuidado y verificar contra issues/changelog
  de CoastSat antes de llamarlo bug.

## Modos del CLI (`strandline extract`)

| Flag | Umbral | Uso |
|------|--------|-----|
| (ninguno) | Otsu global sobre MNDWI, refinado por corredor (`--refine-radius`, default 10 px) | rápido, sin clasificador; ~5.7-6 m de mediana vs CoastSat |
| `--classify` | Otsu balanceado sand/water (MNDWI fiel, sin quirk) | "verdad" MNDWI-Otsu; no comparado 1:1 contra CoastSat (CoastSat siempre usa el quirk) |
| `--coastsat-compat` (implica `--classify`) | Otsu balanceado sobre NDWI aplicado al contorno MNDWI (réplica del quirk) | compatibilidad con shorelines CoastSat existentes; **0.3 m de mediana** |
| `--level <f32>` | fijo, ignora clasificador/Otsu | debug/benchmarking |

`--min-beach-area` (m², default 500) controla el filtro `remove_small_objects`
sobre sand/water tras clasificar.

## Velocidad

| Engine | Por escena (extracción) | 6 escenas |
|--------|-------------------------|-----------|
| CoastSat (Python, incl. clasificador MLP) | ~1.17 s | 7.0 s |
| strandline sin clasificador (Rust, single-thread) | ~0.018 s | 0.11 s |
| strandline `--coastsat-compat`, clasificador **single-thread** | ~0.55-0.62 s | — |
| strandline `--coastsat-compat`, clasificador **16 threads** | ~0.17-0.19 s | ~1.6 s* |

*incluye el spawn de 6 procesos separados; la extracción pura es ~0.175 s/escena.

Sin clasificador: **~65× más rápido** que CoastSat. Con el clasificador MLP
portado y **paralelizado con rayon** (2026-07-11), la etapa de clasificación
baja de ~0.9 s (versión secuencial inicial) a ~0.15 s por escena en 16 cores —
**~6× más rápido que CoastSat en total** (0.17 s vs 1.17 s por escena), en vez
de apenas comparable. Desglose:

- **Etapa `classify`**: 1 thread ~0.51-0.62 s → 16 threads ~0.14-0.18 s
  (≈3.5× en la etapa; el resto del pipeline no cambia).
- **Sin clasificador**: read ~10-25 ms, MNDWI ~1.4-3 ms, Otsu (2 pasadas)
  ~4-12 ms, contorno ~2-4 ms.

### Optimizaciones de la paralelización (2026-07-11)

El escalado ingenuo (paralelizar solo los dos loops calientes) daba apenas
~2.5×. Tres cambios lo subieron a ~3.5-6× y mejoraron también el
single-thread:

1. **Fusión feature-cómputo + clasificación** (`FeatureBank`): se eliminó la
   matriz intermedia `(n_píxeles × 20)` de ~17 MB que se construía en serie
   y se releía. Ahora las 20 features de cada píxel se reúnen bajo demanda
   (`FeatureBank::gather`) y se predicen en la **misma** pasada paralela.
   Fue el mayor salto (de ~2.5× a ~4×): elimina el ensamblado secuencial y
   la presión de memoria.
2. **MLP con pesos en `Vec<f32>` plano** en vez de `ndarray::Array2`: los dos
   productos matriz-vector se recorren en orden axpy (acceso secuencial a los
   pesos, sin bounds-checking 2D de ndarray en el loop caliente), con buffers
   en stack para la capa oculta. Acelera single-thread y paralelo por igual.
3. **`local_std` con `par_chunks_mut`** sobre un buffer plano, sin asignar un
   `Vec` por fila.

La feature `parallel` de `strandline-core` es default; `--no-default-features`
da un binario determinista single-thread (mismos resultados, 13 tests pasan en
ambos modos). El resultado paralelo es idéntico bit-a-bit al secuencial: cada
píxel se calcula independientemente, sin acumulación de orden variable.
Cuello de botella restante: las 10 llamadas a `local_std` en
`FeatureBank::build` son secuenciales entre sí (cada una paralela por dentro);
paralelizarlas unas contra otras es la siguiente optimización si hiciera falta,
pero el escalado actual ya cumple.

## Otras notas técnicas para V1

- **Máscara de nubes**: CFMASK marca píxeles de swash/espuma como nube y
  corta los contornos de strandline en modo sin clasificador (costa
  fragmentada en segmentos de 0.8–2 km). `remove_small_objects` ya está
  portado (`classify::remove_small_objects`, 8-conectividad) y se aplica a
  sand/water cuando `--classify`/`--coastsat-compat` están activos; falta
  aplicar un filtro equivalente a la máscara de nubes cruda para el modo
  sin clasificador.
- **CRS**: escenas del mismo sitio llegan en zonas UTM distintas según el
  path (32618/32619, hemisferio norte con y negativo). V1 necesita
  reproyección integrada a un EPSG de salida (surtgis-core la tiene).
- **Convención pix→world**: CoastSat aplica el geotransform GDAL directo a
  (col, row) fraccionales de skimage, sin offset de medio píxel. strandline
  replica esto (`pix2world` en shoreline.rs).
- **MNDWI de CoastSat es (SWIR−G)/(SWIR+G)** — signo invertido del textbook.
  Internamente strandline usa textbook (G−SWIR) como default; el modo
  `--coastsat-compat` calcula por separado los arrays con signo CoastSat
  (`index::nd_index` directo) sólo para ese modo.
- El refinamiento de umbral por corredor (2ª pasada de Otsu cerca del
  contorno inicial, modo sin clasificador) NO acerca al comportamiento
  CoastSat (el océano domina el histograma y empuja el umbral hacia agua).
  El camino correcto es el clasificador (ahora disponible). Se deja
  implementado (`--refine-radius`, default 10) porque sí estabiliza el
  umbral MNDWI fiel entre escenas sin pagar el costo del clasificador.
- **Clasificador paralelizado con rayon** (feature `parallel`, default): ver
  sección Velocidad. `FeatureBank` fusiona features + predicción en una
  pasada; MLP con pesos planos; `local_std` con `par_chunks_mut`. Cuello de
  botella restante: las 10 `local_std` de `FeatureBank::build` corren en
  serie unas contra otras (cada una paralela por dentro) — paralelizarlas
  entre sí es la próxima palanca si se necesitara más.
- **Balanceo de clases sin RNG**: `balanced_otsu` pesa cada clase a peso
  total 1.0 en vez de submuestrear aleatoriamente como CoastSat — mismo
  resultado en el límite, determinista, sin necesidad de fijar semilla.
- El `Otsu global (strandline)` de la tabla comparativa usa la convención
  textbook (G−SWIR); compararlo directo con `t_mwi` de CoastSat (signo
  SWIR−G) requiere negar uno de los dos — ver `coastsat_compat` en
  `shoreline.rs` para el manejo explícito de ambos signos.

## Archivos

- `spike/scripts/01_download_scenes.py` — descarga GEE (14 escenas L8).
- `spike/scripts/02_reference_shorelines.py` — referencia CoastSat + timing.
- `spike/scripts/03_compare.py` — métrica de distancia con corredor costero.
- `spike/scripts/04_export_classifier.py` — exporta el MLP de CoastSat a JSON.
- `spike/results/coastsat_reference.geojson` — ground truth (EPSG:32719).
- `spike/results/rust_*.geojson` — shorelines strandline modo `--coastsat-compat`.
- `spike/results/effective_levels.json` — umbrales calculados a mano en Python (histórico).
- `spike/results/comparison.json` / `comparison_classifier_compat.json` — métricas finales.
- `spike/results/diagnostic_overlay.png` — overlay visual (generado con el modo umbral-igualado histórico).
- `strandline-core/models/coastsat_landsat_mlp.json` — pesos del MLP embebidos en el binario.
