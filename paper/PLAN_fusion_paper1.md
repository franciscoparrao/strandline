# Fusión del paper 1 de propuesta-4 con el paper de strandline (C&G)

Evaluación del 2026-10-30, respuesta al mensaje de propuesta-4 del mismo día.

## Veredicto

**Sí sirve, y conviene, pero como función de la librería y con el paper
reorganizado; no como una sección más.** El paper de strandline ya depende del
reanálisis oceánico (§3, §5.4-5.6) sin poder citarlo, y ya muestra el caso más
llamativo de la falla (el ángulo espurio de 40°). Integrar el diagnóstico y la
corrección resuelve las dos críticas que tumbaron al paper 1 («la contribución
computacional es un buffer y un umbral», «el código no es inspeccionable») y la
cita huérfana del paper de strandline.

El riesgo es el inverso: el paper ya tiene 8 811 palabras y tres hilos; sumar el
paper 1 entero produce la crítica de «papers engrapados» que los revisores ya
hicieron. La salida es cambiar el eje del paper.

## Eje propuesto

**strandline: motor compilado compatible con CoastSat, con chequeos de
consistencia incorporados que detectan errores sistemáticos invisibles al QC por
serie.** Tres chequeos, cada uno como subcomando con tests y un caso real:

| Chequeo | Qué detecta | Caso real | Estado en el motor |
|---|---|---|---|
| Geometría de la referencia | referencia desviada tierra adentro | 6 sitios Chile + DEA NSW | **no existe**: implementar |
| Desfase entre sensores pareados | umbral dependiente del sensor (L8−L7) | 60 playas, compat sobre SR | test de regresión; falta subcomando |
| Ángulo transecto-normal con nula | transectos desalineados | §5.5 actual | script de spike; falta subcomando |

Los tres comparten la misma lección, que hoy es el párrafo más fuerte del paper:
una geometría equivocada pero consistente pasa los controles habituales.

## Qué entra del paper 1 y qué no

| Material | Decisión | Condición |
|---|---|---|
| Diagnóstico (distancia del ancla + elevación) | **Entra**, como subcomando | Regla de sitio corregida: 4/6 contaminados (Coquimbo pasa 43 %, verificado en `elevation_stats.json`); declarar el DSM: GLO-30 marca 2-13 % de anclas ya corregidas (Constitución 12,6 %) o probar FABDEM |
| Test sobre la distribución de los cruces medidos | **Entra** (lo pidió el revisor espacial) | Nuevo; usa la geometría que ya calcula `intersect` |
| Corrección (referencia OSM + banda 300 m) | **Entra**, como opción de `extract` | Implementar `--reference` y `--max-dist-ref` (hoy no existen); convertir los pasos manuales (recorte de Valdivia, spline de Caleta Abarca) en regla o declararlos |
| Cambios de signo de las tasas (Viña, Constitución) | **Entra**, resumido | Como demostración de impacto, no como resultado central |
| Monte Carlo (1 440 corridas) | **Entra corto** | Describirlo como generador 1-D con probabilidades impuestas; sirve para elegir el ancho de banda |
| Portabilidad DEA NSW (7 681 estaciones) | **Entra corto** | Responde la crítica de generalidad |
| «Ceguera» del QC estadístico | **Se reduce** | Solo «los umbrales en uso las retuvieron»; corregir el 47 % (es la logística; la duración sola quita 90,9 % perdiendo 26,5 %) |
| Validación de OSM | **Pendiente obligatorio** | Medir el error de OSM contra SHOA o línea digitalizada; etiquetas independientes por interpretación VHR (cuenta Planet E&R disponible, 3 m) |

## Qué sale o se reduce del paper actual

- **§5.6 pesos por sensor** → apéndice o fuera: es la sección más débil y el
  desfase entre sensores la reemplaza con mejor evidencia.
- **Estimación de pendiente (§5.3, segunda mitad)** → fuera o una oración; tiene
  una cifra sin fuente (0,097).
- **§5.4 geometría de refinado** → se mantiene corta.

## Trabajo nuevo necesario (orden)

1. Implementar en strandline `refcheck` (distancia del ancla a una costa en
   GeoJSON, elevación desde un DEM GeoTIFF, distribución de cruces) y
   `extract --reference --max-dist-ref`. Tests y un ejemplo de punta a punta.
2. Validar contra terreno en sitios de Vos et al. 2023 (Narrabeen, Duck…): sirve
   al benchmark del motor y a la corrección.
3. Error de OSM y etiquetas VHR independientes.
4. Baselines: CoastSeg, SAET u «OSM desde el principio».
5. Reescribir con el eje nuevo; review simulada en C&G.

## Decisiones que son del usuario

1. **Alcance:** recomendación arriba (diagnóstico + corrección + Monte Carlo corto + DEA).
2. **Autoría:** el material del paper 1 trae a sus cuatro coautores; hoy el paper es Parra-O. y Marín. Pasar `/firma-postdoc` después.
3. **Zenodo:** recomendación, dos registros. Software (strandline, versionado) y datos/benchmark (scripts y salidas de los 6 sitios + 60 playas), citado por el paper. El borrador 22845598 puede convertirse en el de datos.
