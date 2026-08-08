# Medición directa de la desalineación shore-normal — INCONCLUSA

Intento de medir el ángulo θ entre cada transecto fijo y la normal local de la
shoreline instantánea, usando los transectos y las shorelines publicadas del
estudio de referencia (sin re-extraer nada, sin confound SR-vs-TOA).

**Motivación**: §5.4 del paper sostiene la desalineación de forma *indirecta*
(por su efecto sobre una ventana de ajuste). Medir θ directamente daría el
claim fuerte: la distancia medida a lo largo de un transecto desalineado θ es
la perpendicular verdadera dividida por cos(θ), y como 1/cos(θ) ≥ 1 siempre,
**cualquier** desalineación infla — incluso una simétrica y aleatoria produce
sesgo positivo sistemático en el chainage y en la tasa.

## Estado: no validado. NO usar sus números.

El script incluye un **control**: contra la propia línea de referencia desde la
que se generaron los transectos, θ debe dar ~0° por construcción. No pasa.

| versión | método | control | veredicto |
|---|---|---|---|
| v1 | tangente por PCA de bola de radio fijo | 2% de cobertura | artefacto de selección: la bola mezcla contornos disjuntos y con vértices cada 184 m casi no junta puntos |
| v2 | vértices consecutivos, gap fijo 100 m | mediana 5-40° | el gap despedaza la línea de referencia (espaciado 184 m); cruce exigido dentro de `[0,L]` cuando la semántica real es recta infinita |
| v3 (este) | cruce exacto segmento-segmento, gap adaptativo | mediana 4.5-12.2°, p90 40-55° | cobertura ~100%, pero el estimador tiene piso de error del mismo orden que la señal buscada |

## Qué haría falta

El problema de fondo: estimar la normal local de un contorno de marching
squares es incierto en varios grados (el contorno es escalonado a escala de
píxel), comparable al efecto que se busca. Haría falta un estimador de tangente
que **pase el control** — p. ej. replicar exactamente el mismo estimador
(diferencias centrales sobre la línea re-muestreada) con el que se generaron los
transectos, y recién entonces aplicarlo a las shorelines instantáneas.

Mientras el control no dé ~0°, cualquier θ que salga de aquí es indistinguible
del error del propio método.

## Uso

    python3 measure_transect_angle.py

Lee los transectos de `{SITE}_analysis.pkl` y las shorelines de
`/mnt/kingston/{SITE}/{SITE}_output.pkl`. Si el control falla, no imprime la
medición.
