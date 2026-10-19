# §5.4-5.6 re-corridos sobre los transectos oceánicos (2026-10-19)

Los tres resultados de §5.4-5.6 del paper C&G se midieron sobre los transectos y
series del estudio de referencia (`propuesta-4/output/`). El reanálisis de
propuesta-4 mostró que esa base está contaminada por agua interior. Aquí se
repiten sobre `output_reprocessed/*_analysis_ocean.pkl` (referencia anclada a la
costa oceánica, shorelines re-extraídas con `reference_shoreline` +
`max_dist_ref`). Chiloé entra como quinto sitio.

## Reproducibilidad del conjunto original

- §5.5: `SDS_DATASET=original` reproduce bit a bit Viña, Constitución y
  Valdivia. Coquimbo no: `/mnt/kingston/COQUIMBO/COQUIMBO_output.pkl` se
  modificó el 2026-08-31, después del export del paper (2026-08-10); hoy da 249
  transectos en vez de 334.
- §5.6: `run_table5.py` reproduce la Tabla 5 publicada (Coquimbo, Constitución y
  Valdivia exactos al decimal; Viña a 0,2 m en L5 y L9).
- §5.4: las escenas SR de la Tabla 3 no se conservaron. La v2 las vuelve a bajar
  con una regla fija (OLI, 2018-2021, nubosidad ≤ 15 %, 15 fechas presentes en
  ambos pickles). Con esas escenas, el conjunto original reproduce el patrón
  publicado: 13,80 / 14,71 / 11,47 m frente a 14,29 / 15,15 / 11,78 m.

## §5.5 — ángulo con la normal local (`transect_angle/ocean/`)

| | Original (paper) | Oceánico |
|---|---|---|
| Transectos | 538 (4 sitios) | 857 (5 sitios) |
| Ángulo persistente, mediana | 39,9° | **5,7°** |
| Inflación de tasa implicada | 30 % | **0,5 %** |
| Transectos > 20° | 86 % | 17 % |
| Reducción de LRR al proyectar | 21-39 % | 0-10 % |

Por sitio (oceánico): Viña 3,7°, Coquimbo 4,6°, Constitución **22,1°**,
Valdivia 5,7°, Chiloé 5,0°. Coherencia espacial (lag 1) 0,36-0,74 y estabilidad
entre mitades 0,77-0,98: las dos pruebas siguen pasando, sobre un ángulo chico.

Del conjunto original, solo 130 de 538 transectos (24 %) tienen el origen a
menos de 200 m de la costa oceánica (`transect_angle/angle_vs_coast.txt`).

## §5.4 — dónde se muestrea el ajuste subpíxel (`profile_sr/geometry_v2.json`)

Mediana del error (m) contra la distancia de cada conjunto, mismas escenas:

| Grupo · conjunto | Sin refinar | Perfil sobre transectos fijos | Contorno en normales locales |
|---|---|---|---|
| Complejos · original | 13,80 | 14,71 | 11,47 |
| Complejos · oceánico | 16,00 | **13,90** | **12,46** |
| Rectos · original | 17,40 | 12,71 | 9,79 |
| Rectos · oceánico | 12,28 | 11,14 | 10,21 |

Con transectos oceánicos, el perfil sobre los transectos fijos deja de ser peor
que no refinar: mejora en Coquimbo, Valdivia y Chiloé, y solo empeora en
Constitución, el único sitio con ángulo grande (22°). Refinar sobre normales
locales sigue siendo lo mejor en los dos conjuntos (≈ 22 % bajo el contorno crudo
en los sitios complejos oceánicos).

## §5.6 — precisión por satélite (`satellite_weights/table5.txt`)

Desviación típica del residuo (m), conjunto oceánico:

| Sitio | L5 | L7 | L8 | L9 |
|---|---|---|---|---|
| Viña del Mar | **40,5** | 51,6 | 42,1 | 43,2 |
| Coquimbo | 39,4 | 37,7 | **34,3** | 36,8 |
| Constitución | 48,3 | 40,8 | **39,1** | 46,4 |
| Valdivia | **28,1** | 42,6 | 31,3 | 33,5 |
| Chiloé | 39,3 | 39,7 | **36,8** | 37,2 |

Los residuos bajan de 44-178 m a 28-52 m. Razón de varianza L5/L8 en Coquimbo:
3,34 → 1,32. El orden sigue cambiando entre sitios, con diferencias chicas.
