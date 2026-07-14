# TASK — `fetch`: soportar el archivo Landsat completo (TM/ETM+ además de OLI)

**Origen**: proyecto `zenodo-gaps/propuesta-4-erosion-costera` (reanálisis de
erosión costera de Chile, 1984–2025). 2026-07-13.
**Prioridad**: alta — bloquea el reanálisis multidecadal con `fetch`.
**Relacionado**: extiende el ítem S2 del audit (procesamiento cloud-native COG/STAC).

## Problema

`fetch` sobre `landsat-c2-l2` devuelve **solo escenas Landsat-8/9** (OLI). El
filtro está en `strandline-cli/src/fetch.rs:152-160`:

```rust
if collection == "landsat-c2-l2" {
    items.retain(|it| {
        it.properties.platform.as_deref()
            .map(|p| p.contains("landsat-8") || p.contains("landsat-9"))
            .unwrap_or(false)
    });
}
```

Consecuencia: una consulta histórica (p. ej. Viña del Mar, 2005) devuelve
**0 escenas**, porque excluye Landsat 4/5 (TM, 1984–2013) y Landsat 7 (ETM+,
1999+). Eso impide construir series multidecadales con `fetch` — hay que caer a
descargas GEE (que se cuelgan) o a datos locales recortados al AOI original.

**Caso de uso concreto que lo motiva**: cerrar el gap de cobertura de Caleta
Abarca (Viña del Mar) requiere fetch del AOI extendido con historia completa
1984–2025. Hoy es imposible por este filtro.

## Fix propuesto (probablemente chico)

El filtro es lo único que bloquea. La maquinaria de bandas y reflectancia YA
sirve para TM/ETM+:

1. **Assets por nombre común**: `collection_assets` (fetch.rs:33) usa
   `["blue","green","red","nir08","swir16"]` — nombres comunes que en el STAC de
   Planetary Computer resuelven al band correcto **por sensor** (TM/ETM+/OLI).
   *Verificar* que existen para TM/ETM+ (deberían); si algún sensor no expone
   un common-name, mapear por `SR_B*` según plataforma.
2. **Reflectancia**: `to_reflectance` (fetch.rs:48) usa el escalado C2 L2
   `dn*0.0000275 - 0.2`, **idéntico** para TM/ETM+/OLI. Sin cambios.
3. **Relajar el filtro de plataforma**: incluir landsat-4/5/7. Sugerido: en vez
   de hardcodear, un flag `--sensors oli|tm|etm|all` (default `all` para
   `landsat-c2-l2`), o al menos quitar el `retain` a L8/L9.
4. **L7 SLC-off**: las escenas ETM+ post-2003-05-31 tienen franjas de relleno
   (DN 0 → NaN vía `to_reflectance`, ya manejado aguas abajo). Agregar flag
   opcional `--skip-l7-slc` (espeja el `skip_L7_SLC` de CoastSat) para quien
   quiera excluirlas; por default incluirlas (la extracción tolera los gaps).

## Por qué es seguro

strandline **ya extrae shorelines de escenas TM/ETM+**: la validación multisitio
del spike (`spike/multisite/`, `spike/constitucion/`) corre `rates` a paridad
exacta contra los pickles del paper, que incluyen L5/L7; y la extracción se
validó a 0.3 m vs CoastSat. El único eslabón que falta es **adquirir** esas
escenas por STAC; el resto del pipeline las procesa hoy.

## Criterios de aceptación

1. `fetch --collection landsat-c2-l2 --bbox="-71.60,-33.04,-71.52,-32.98"
   --datetime 2005-01-01/2005-04-01 --max-cloud 60` devuelve **> 0 escenas**
   (L5 y/o L7).
2. `extract --coastsat-compat` sobre una escena L5 (TM) traída por `fetch`
   produce una shoreline válida, comparable (< algunos m) con la extracción de
   la misma fecha desde los ms.tif GEE en `/mnt/kingston/VINA_DEL_MAR/L5/ms/`.
3. `--skip-l7-slc` excluye correctamente ETM+ post-2003.
4. Sin regresión en L8/L9 ni en Sentinel-2.

## Validación sugerida

Cruzar contra los ms.tif GEE ya en disco:
`/mnt/kingston/{ARICA,COQUIMBO,CONSTITUCION,VALDIVIA,VINA_DEL_MAR}/L5|L7/ms/*.tif`
— misma fecha, fetch-vs-GEE, distancia punto-a-línea. Debería quedar en el mismo
orden que la paridad ya lograda con OLI.

## Notas

- Ojo con el CRS: PC entrega los COG en su UTM nativo (p. ej. EPSG:32619 para
  Viña, con northing negativo). `extract`/`profile` ya lo reportan por escena;
  el consumidor reproyecta. No cambia con TM/ETM+.
- Este cambio convierte a `fetch` en un adquiridor de archivo completo sin GEE
  — un diferenciador fuerte para el paper de strandline (ningún challenger
  compilado hace streaming del archivo Landsat 40-años por COG range requests).
