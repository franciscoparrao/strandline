# Validación del slope satelital (S4) contra perfiles de terreno — RESULTADO NEGATIVO

Primer intento de validar `strandline slope` contra un dato independiente. Hasta
ahora S4 estaba validado solo en sintético (±0.01) y contra un valor de
literatura (~0.10) — el eslabón más débil de §5.3 del paper.

**Resultado: el estimador NO convergió en Algarrobo. La validación no se logró,
pero el fracaso es diagnosticable y da un criterio de identificabilidad
cuantitativo que el paper hoy no tiene.**

## Los datos de terreno

Perfiles de playa medidos por tesistas de geología, recuperados de
`~/proyectos/playas-machine-learning/tesis/`. **Los CSV de perfiles no se
distribuyen en este repositorio** (datos de terceros); los scripts los esperan
en esta carpeta:

| archivo | sitio | perfiles | campañas |
|---|---|---|---|
| `algarrobo_profiles_2023.csv` | Algarrobo (33.36°S) | 4 | mayo y octubre 2023 |
| `el_quisco_profiles_2023.csv` | El Quisco (33.38°S) | 5 | marzo y noviembre 2023 |

Ambos extraídos con la orientación verificada contra las figuras de las tesis
(mar a la izquierda, S0 = cara de playa) y con las trampas de etiquetas
documentadas en el encabezado de cada CSV. Ver ahí antes de usarlos: la tesis de
El Quisco **se contradice a sí misma** sobre qué código corresponde a cada
temporada, y se resolvió por física.

`clip_to_tidal_range.py` reconstruye la geometría (x, z) encadenando segmentos y
recorta a la banda de marea que la serie satelital efectivamente muestrea
(1.43 m, de `spike/slope_s2/tides.csv`). El recorte resultó ser casi irrelevante:
en la mayoría de los perfiles el primer segmento ya asciende más que la banda,
así que la pendiente efectiva coincide con la del segmento.

Pendiente efectiva de cara de playa sobre la banda de marea:

| sitio | invierno / post-invierno | verano |
|---|---|---|
| Algarrobo | 0.105 | 0.167 |
| El Quisco | 0.194 | 0.325 |

Se eligió **Algarrobo** para el experimento: es el único con coordenadas UTM por
perfil, y sus pendientes caen en el rango donde la comparación discrimina.

## El experimento

`run_algarrobo.py`: 214 escenas Sentinel-2 (2019–2025) vía `fetch` COG streaming
→ `baseline` automática → 130 transectos (50 m de espaciado, 400 m de largo) →
`profile` por escena (212/214 OK, 20.849 observaciones) → marea GOT5.5 (pyTMD,
venv del paper) → `slope`.

Artefactos en `algarrobo_run/`.

## Qué pasó

```
                        satelital        terreno
mediana (crudo)            0.021       0.105-0.167
mediana (despiked)         0.026
```

Un factor de 5 a 8 por debajo. Pero el número satelital **no es una estimación
sesgada: es un no-resultado**:

- **65 de 130 transectos quedaron exactamente en 0.0200**, que es el piso de la
  grilla del barrido, y 69 vienen marcados `at_boundary=true`.
- El `energy_ratio` mediano es **0.96–0.99**: la corrección mareal no remueve
  prácticamente nada de energía. No hay señal mareal que minimizar.
- Los 61 transectos que sí convergieron dan mediana 0.043, todavía muy por
  debajo del terreno.
- `tide_sign` se invierte entre la mitad norte (+1) y la sur (−1) de la línea
  base, señal de que la orientación seaward única (`--seaward-e=-1`) no sirve
  para los 8.4 km de costa que eligió `baseline`, que incluye tramos rocosos y
  no solo la playa arenosa.

## Segunda corrida: línea base recortada a costa abierta

`run_algarrobo_clipped.py` prueba la hipótesis (a), que el fracaso fuera
geométrico. La línea base completa incluía el contorno del Humedal El Membrillo,
donde **se dobla sobre sí misma** (el azimut pasa de 189° a 26° en el arco
3.6–4.7 km): ahí los transectos apuntan tierra adentro. Se detectan los tramos
de costa abierta por la normal local (se conservan los que apuntan al oeste) y
se usan los dos que cubren los perfiles sin solaparse — 3653 m y 1560 m — en vez
de los 8436 m originales.

**No cambió nada material:**

| | línea base completa | recortada |
|---|---|---|
| transectos | 130 | 86 |
| en el borde de la grilla | 69 (53%) | 43 (50%) |
| mediana | 0.0212 | 0.0225 |
| mediana de los convergidos | 0.0425 | 0.0350 |
| `energy_ratio` | 0.960 | 0.968 |

Cerca de los perfiles, la mediana queda en 0.0200 —el piso de la grilla— en tres
de los cuatro (Humedal da 0.0325). El `tide_sign` sigue mezclado (51 contra 35)
incluso sobre tramos de costa abierta bien orientados, lo que indica que el signo
no era un artefacto de orientación sino que el estimador lo elige al azar cuando
no hay señal.

**Conclusión: la causa es la física, no la geometría.** Arreglar los transectos
no rescató la estimación.

## El diagnóstico, que es el resultado aprovechable

Con rango mareal de 1.43 m, la **excursión horizontal de la línea de agua** es:

| pendiente | excursión | en píxeles S2 (10 m) |
|---|---|---|
| 0.105 (terreno, invierno) | 13.6 m | 1.4 px |
| 0.167 (terreno, verano) | 8.6 m | 0.9 px |
| 0.020 (lo que "estimó") | 71.5 m | 7.2 px |

En una playa tan empinada como Algarrobo, **la marea entera mueve la costa menos
de un píxel y medio**. La señal que S4 necesita minimizar está por debajo de la
resolución del sensor, y el barrido escapa al piso de la grilla.

La asimetría descarta la explicación alternativa: si el terreno estuviera
equivocado y la pendiente real fuera 0.02, la zona intermareal mediría 71 m de
ancho, más que el ancho total de la playa. El dato de terreno es mucho más
creíble que el satelital acá.

## Por qué esto importa para el paper

1. Es una versión **cuantitativa** del hallazgo E3, que hoy está enunciado de
   forma cualitativa ("en micro-mareal no es identificable con solo-Landsat").
   El criterio real no es micro-mareal vs macro-mareal sino
   **excursión mareal ÷ tamaño de píxel**, y hace falta bastante más de 1 px.
   Viña funcionó (0.097–0.105) con la misma marea porque su playa es más tendida
   y sobre todo porque son 75 transectos sobre una playa arenosa continua y bien
   orientada, no 130 sobre 8.4 km de costa mixta.
2. Da un dato de terreno **independiente** para la sección de limitaciones, y de
   paso cuantifica algo que el motor supone sin discutir: la cara de playa varía
   un factor **~1.6× entre estaciones** en los dos sitios medidos por dos
   tesistas distintos (Algarrobo 0.105→0.167, El Quisco 0.194→0.325). La Ec. 2
   usa un `tanβ` único por sitio.

## Qué faltaría para cerrar la validación

- ~~Restringir la línea base al tramo arenoso~~ — **probado, no ayuda** (ver
  arriba). Descarta la explicación geométrica.
- Elegir un sitio donde la excursión mareal sea de varios píxeles: costa
  macro-mareal (Chiloé, rango ~2 m con playas tendidas) o una playa disipativa
  de pendiente ~0.02–0.03.
- La comparación correcta contra un terreno estacional exige además partir la
  serie satelital por estación, lo que reduce a la mitad las fechas disponibles.

## Nota operativa

Las mareas de `algarrobo_run_clipped/tides.csv` NO se recalcularon con pyTMD: el
caché de `~/.cache/pytmd/GOT5.5/` quedó vacío entre una corrida y otra. Se
reconstruyeron por join de fecha contra `algarrobo_run/tides.csv`, que es exacto
porque la marea depende solo de fecha y ubicación, ambas idénticas entre las dos
corridas (212/212 fechas encontradas). Para repetir el experimento desde cero hay
que volver a bajar el modelo GOT5.5.
