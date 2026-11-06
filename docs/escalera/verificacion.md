# Verificación — escalera-strandline (2026-10-17)

Manuscrito: `paper/paper.tex` (borrador C&G, PDF de 39 páginas; las referencias «p. N» son páginas del PDF).
Material: modo completo, 46 conceptos, 22 preguntas, 17 079 palabras, PDF de 61 páginas.
Validador mecánico (`verificacion-mecanica.md`): 0 errores, 0 avisos.

## Prueba del lector novato

| Nivel | Ciclo 1: SALTO / FALLA / CARGA | Ciclo 2: SALTO / FALLA / CARGA |
|---|---|---|
| 1 | 10 / 1 / 8 | 10 / 2 (una leve) / 14 |
| 2 | 15 / 2 / 9 | 14 / 0 / 9 |
| 3 | 18 / 1 / 9 | 9 / 1 / 6 |
| 4 | 22 / 1 / 13 | 12 / 0 / 7 |
| Total | 65 / 5 / 39 | 45 / 3 / 36 |

En el ciclo 2 los lectores marcaron sobre todo saltos nuevos y más finos: los del ciclo 1 se corrigieron casi todos.

### Corregido después del ciclo 2 (no se relanzó un tercer ciclo, como fija el skill)

- N1: el ejemplo del IQR contradecía la definición (daba 3 m; con la definición del material es 2 m).
- N1: faltaba decir que la arena devuelve NIR (la falla de la pregunta 5).
- N1: desviación típica y varianza, con su cálculo y la razón de las unidades al cuadrado.
- N2: «si llega el doble de luz, a y b se duplican» contradecía la definición de reflectancia.
- N2: por qué una reflectancia corregida puede ser negativa; valor absoluto; periodo; de dónde sale Δx.
- N2: el coseno es el lado que toca θ; por qué cos(−θ) = cos θ.
- N3: la pregunta intercalada del ángulo de 4° mezclaba ruido del estimador con inflación de la tasa (falla).
- N3: origen de la referencia en Python vectorizada; por qué el orden de las sumas cambia el redondeo; 12 núcleos y 16 hilos; qué significan 15, 10 y 7 en la tabla de pesos.
- N3/N1: contradicción sobre si los transectos son perpendiculares a la línea de referencia.
- N4: «razón» con dos sentidos; «ventana de ajuste» frente a «ventana de muestreo»; ventana del estimador usada antes de definirla; media mayor que el percentil 90; el rango 21-39 % no sale de la mediana.

### Sin resolver (decisión editorial)

- CARGA: la mayoría de los párrafos marcados juntan dos ideas muy ligadas (mediana/media/percentil, transecto/línea base, TOA/SR, regresión/R²). Partirlas más alarga el material sin ganancia clara.
- Saltos menores de vocabulario declarados como suficientes: «coordenadas», «geoespacial», «código», «lenguaje de programación», «función», la banda pancromática de 15 m, qué pares de bandas forman las otras cuatro diferencias normalizadas, qué es la «pendiente verdadera» en el intervalo de confianza, la pendiente de una recta antes del ejemplo.
- Por qué 17 y 23 transectos son «muy pocos» para las pruebas del ángulo: el manuscrito no da el criterio salvo el mínimo de 30 transectos con 10 fechas (Tabla 4).
- La máscara de nubes de Sentinel-2 no se menciona (el material solo habla de la de Landsat).

## Prueba de fidelidad

- 6 bloques `.manuscrito` y 6 afirmaciones de `grafo.yaml`: 12 FIEL; 0 EXAGERA, 0 DEBILITA, 0 NO_ENCONTRADO, 0 REF_ERRONEA.
- Corregido: páginas imprecisas (bit a bit en p. 21; Fig. 3 en p. 30; «tens of percent» y «neither settles» en p. 27; transectos a 0,0 m en Sec. 2.5, p. 10); «oleaje» en vez de «transporte de arena transversal»; surtgis-core no es «de los mismos autores» sino «motor hermano»; «sin paquetes externos» en vez de «sin dependencias en tiempo de ejecución»; «costa de Chile central» generalizaba desde un solo sitio.
- Limitaciones que faltaban y se agregaron: líneas sintéticas en el benchmark; el 0,30-0,35 m exige el modo compatible; error del estimador comparable al efecto; Arica y Chiloé fuera del ángulo y de la Tabla 5; instalación y memoria no cuantificadas; el autor no encontró trabajos previos y pide confirmación independiente.
- No agregadas (menores): la marea no reduce el SCE agregado de Viña; el motor no compite en descubrimiento ni interactividad.
- Lectura propia del material, marcada como tal en el texto: la frase «incluso un desalineamiento simétrico al azar produce un sesgo positivo» (p. 24) se interpreta como ángulos persistentes repartidos al azar entre transectos.

## Referencias

`verify_refs` sobre `fuente/referencias.bib`: 4/4 verificadas tras una corrección.
La entrada `pardopascual2018shorex` (copiada de `paper/refs.bib`) tenía autores, año, volumen, páginas y DOI de otro artículo. El trabajo citado es Sánchez-García et al. (2020), *Coastal Engineering* 160:103732, doi:10.1016/j.coastaleng.2020.103732. **El mismo error sigue en `paper/refs.bib` del manuscrito.**

## Salidas revisadas

- PDF: hojas de contacto de las 61 páginas y páginas sueltas a 70-90 ppp; sin cajas cortadas ni `Missing character`. La página 1 queda casi vacía porque el mapa es un flotante.
- HTML: captura completa; el botón de diagnóstico responde (aciertos en niveles 0 y 1 → «Empieza en el Nivel 2») y las respuestas desplegables abren; 0 errores de JavaScript.

## Actualización 2026-10-22 (paper reescrito en §3, §5.4-5.6, commit 927e801)

Material: 47 conceptos (nuevo: «Referencia desviada», nivel 4), 22 preguntas, 17 565 palabras, PDF de 62 páginas. Validador mecánico: 0 errores, 0 avisos. Referencias: 4/4 verificadas. HTML: diagnóstico y respuestas desplegables funcionan, 0 errores de JavaScript.

Cambios: afirmaciones a3, a4 y a5 reescritas con las cifras oceánicas; nivel 4 nuevo desde «Dos versiones de los mismos transectos»; Fig. 3 nueva; todas las páginas citadas remapeadas al PDF de 41 páginas.

| Prueba | Ciclo 1 | Ciclo 2 |
|---|---|---|
| Lector nivel 4 | 9 saltos / 0 fallas / 10 cargas | 5 saltos / 1 falla / 12 cargas |
| Fidelidad | 6 bloques y 6 afirmaciones FIEL; 1 error de hecho; 6 restos de la versión vieja; 6 limitaciones omitidas | (no se relanzó) |

Corregido: el nivel 2 decía que la referencia TOA usa pansharpening, y el paper dice que está desactivado (error presente desde la primera versión de la escalera, no detectado entonces); ejemplos de los niveles 2 y 3 con cifras viejas (±0,14; 35°); limitaciones omitidas (afirmación acotada que el paper defiende, pesos aplicados a ciegas, pendiente en sitios macromareales, corte de 0,71 px corregible); la falla del ciclo 2 (la respuesta presentaba como causa probada la explicación de los autores para Constitución).

Sin resolver: de dónde sale 0,71 px exige Pitágoras, que el material no enseña; cómo se ponen escalones de píxel a la nula; las cargas restantes juntan ideas muy ligadas. El ciclo 2 no se repitió, como fija el skill.
