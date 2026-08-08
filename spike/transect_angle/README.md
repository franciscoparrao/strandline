# Desalineación shore-normal de los transectos fijos — medición directa

Mide el ángulo θ entre cada transecto **fijo** del estudio de referencia y la
normal local de la shoreline instantánea, usando los transectos y shorelines
**publicados** (sin re-extraer nada, sin confound SR-vs-TOA).

## Por qué importa

La distancia medida a lo largo de un transecto desalineado un ángulo θ respecto
de la normal es la perpendicular verdadera dividida por `cos(θ)`. Si θ es
**persistente** para ese transecto, la tasa de cambio queda inflada por el mismo
factor: un retroceso real de Δ perpendicular se mide como Δ/cos(θ).

Como `1/cos(θ) ≥ 1` siempre, el error es **de un solo signo**: sólo puede
inflar, nunca reducir.

## Resultado (`results.txt`)

Control/nula: 0.5–4.2° · Señal: 30–35° · estable en ventanas de ±60 a ±200 m
(Mann-Whitney p≈0).

Descomposición persistente vs fluctuante (ventana ±100 m, transectos con ≥10
fechas):

| sitio | n transectos | θ persistente | fluctuación (MAD) | inflación de tasa |
|---|---|---|---|---|
| Viña del Mar | 16 | 41.8° | 6.3° | 34.3% |
| Coquimbo | 327 | 38.6° | 13.1° | 28.0% |
| Constitución | 22 | 40.9° | 13.1° | 32.4% |
| Valdivia | 161 | 46.5° | 10.6° | 45.4% |
| **global** | **526** | **40.3°** | **12.9°** | **31.1%** |

85% de los transectos tienen θ persistente > 20°; 76% > 30°.

## Por qué el control es el punto clave

θ = arcsin|u·t| es una cantidad **rectificada**: el ruido del estimador infla la
mediana medida aunque el θ verdadero sea 0. Y estimar la normal de un contorno
de marching squares es incierto en varios grados porque el contorno está
escalonado a escala de píxel. Por eso no basta con medir: hay que comparar
contra una **nula** construida con el mismo estimador sobre una línea cuyo θ
verdadero es 0 por construcción — la línea de referencia desde la que se
generaron los transectos — re-muestreada a la misma densidad de vértices (15 m)
y cuantizada a la grilla de píxel para imitar el escalonado.

Tres versiones previas fallaron su propio control y sus números se descartaron:

| versión | fallo |
|---|---|
| v1 | tangente por PCA de bola de radio fijo: mezclaba contornos disjuntos; 2% de cobertura |
| v2 | gap fijo de 100 m despedazaba la línea de referencia (vértices cada 184 m); cruce exigido dentro de `[0,L]` cuando la semántica real es recta infinita |
| v3 | cruce exacto y gap adaptativo (cobertura ~100%), pero sin nula justa el piso de error era del orden de la señal |

## ¿Es geometría real o artefacto del estimador? (`verify_results.txt`)

Aplicar la corrección `cos(θ)` a las tasas es **aritméticamente forzado** y no
prueba nada. La pregunta que sí se puede responder es si el ángulo es real. Dos
discriminantes independientes, ambos positivos:

**1. Coherencia espacial.** Los transectos están ordenados a lo largo de la
costa (~100 m). Si θ_persist es orientación real, vecinos correlacionan; si es
ruido del estimador, no.

| sitio | n | r lag1 | r lag2 | r lag5 | r lag20 | nula permutada |
|---|---|---|---|---|---|---|
| Coquimbo | 319 | **0.652** | 0.556 | 0.443 | −0.046 | [−0.106, +0.103] |
| Valdivia | 162 | **0.503** | 0.267 | 0.356 | 0.070 | [−0.170, +0.124] |

Muy fuera de la nula, y **decae con la distancia** hasta ~0 a lag 20 (≈2 km) —
exactamente la firma de orientación costera real. Ruido puro daría r≈0 en todos
los lags.

**2. Estabilidad temporal.** θ_persist calculado en dos mitades temporales
independientes: r = **0.698** (Coquimbo), **0.921** (Valdivia). El ruido del
estimador no se reproduce entre subconjuntos de fechas disjuntos.

**3. Consecuencia aritmética** (no evidencia independiente, pero ya significativa
dado 1 y 2): proyectar el LRR publicado a la normal local lo reduce 21.7% a
38.7% según el sitio.

## Limitaciones a declarar si esto va al paper

1. **n de transectos desigual**: el global está dominado por Coquimbo (327) y
   Valdivia (161); Viña (16) y Constitución (22) son delgados por cobertura.
2. **La nula es una línea suave.** La rugosidad morfológica real de la costa a
   escala 50–200 m (cúspides, barras, ensenadas de rip) se cuenta como
   desalineación. Es defendible —esa rugosidad *sí* desalinea el transecto—
   pero conviene decirlo.
3. La traducción θ → inflación de tasa supone que la costa retrocede
   aproximadamente paralela a sí misma y que θ se mantiene; es justamente lo
   que mide la componente persistente, pero es un supuesto explícito.
4. **Solo 2 de 4 sitios sostienen los tests discriminantes**: Viña (17) y
   Constitución (23) tienen muy pocos transectos con ≥10 fechas. El resultado
   descansa en Coquimbo y Valdivia.
5. **La caveat interpretativa de fondo — cuál es la dirección "correcta"**. Que
   el transecto esté oblicuo a la orientación local a escala de ~100 m está
   medido y es sólido. Que eso *infle la tasa* supone además que el
   desplazamiento verdadero es perpendicular a esa orientación local. Si la
   costa retrocede paralela a sí misma a escala **regional**, medir a lo largo
   de un transecto normal a la tendencia regional sería correcto y la
   "corrección" estaría de más. A favor de la interpretación local: el ángulo
   se mantiene al ensanchar la ventana del estimador de ±60 a ±200 m (30.4° →
   35.5°), o sea no es un efecto de cúspides de pequeña escala; y el transporte
   transversal que gobierna el retroceso actúa perpendicular a la costa local.
   Aun así es un supuesto explícito, no una medición.
6. **Implicación incómoda**: si la interpretación es correcta, las tasas
   publicadas con transectos fijos —incluidas las del estudio de referencia—
   estarían infladas en ese orden.

## Uso

    python3 measure_transect_angle.py

Lee los transectos de `{SITE}_analysis.pkl` y las shorelines de
`/mnt/kingston/{SITE}/{SITE}_output.pkl`. Si el control no pasa, no imprime la
medición.
