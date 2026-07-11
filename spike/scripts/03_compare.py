#!/usr/bin/env python3
"""
Spike strandline — Paso 3: comparación strandline vs CoastSat.

Métrica: para cada punto de la shoreline CoastSat (ground truth) del día D,
distancia al segmento más cercano de la shoreline strandline del mismo día,
y viceversa (simétrica). Reporta media/mediana/p90 por escena y global.

Criterio GO: mediana global < 5 m.
"""

import glob
import json
import os

import numpy as np
from pyproj import Transformer
from shapely.geometry import LineString, MultiPoint, Point
from shapely.ops import unary_union

RESULTS = os.path.expanduser("~/proyectos/strandline/spike/results")
TARGET_EPSG = 32719

with open(os.path.join(RESULTS, "coastsat_reference.geojson")) as f:
    ref = json.load(f)

# shorelines de referencia por fecha (EPSG:32719, puntos)
ref_by_date = {}
for feat in ref["features"]:
    date = feat["properties"]["date"]
    pts = np.array(feat["geometry"]["coordinates"])
    ref_by_date.setdefault(date, []).append(pts)

rows = []
all_d_rust2ref = []
all_d_ref2rust = []

for path in sorted(glob.glob(os.path.join(RESULTS, "rust_*.geojson"))):
    date = os.path.basename(path)[5:15]
    if date not in ref_by_date:
        print(f"{date}: sin referencia CoastSat (descartada por QC), se omite")
        continue

    with open(path) as f:
        rust = json.load(f)
    if not rust["features"]:
        print(f"{date}: strandline no extrajo líneas")
        continue

    epsg = rust["features"][0]["properties"]["epsg"]
    tf = Transformer.from_crs(f"EPSG:{epsg}", f"EPSG:{TARGET_EPSG}", always_xy=True)

    rust_lines = []
    for feat in rust["features"]:
        coords = np.array(feat["geometry"]["coordinates"])
        x, y = tf.transform(coords[:, 0], coords[:, 1])
        rust_lines.append(LineString(np.column_stack([x, y])))
    # Corredor costero: ambos engines extraen TODOS los contornos agua/tierra
    # (lagunas, esteros, blobs urbanos). En producción CoastSat los descarta
    # con un buffer alrededor de una reference shoreline (max_dist_ref, 100 m
    # default). El spike replica ese QC para ambos: solo se compara dentro de
    # un corredor de 150 m alrededor de la costa principal (la línea más larga
    # de strandline, verificada visualmente sobre la costa). La fracción
    # excluida se reporta — la métrica no la esconde.
    # la costa de rust viene fragmentada (píxeles de swash marcados nube por
    # CFMASK cortan los contornos): la "costa principal" son todas las líneas
    # largas, no solo la mayor
    coast_lines = [l for l in rust_lines if l.length >= 800.0]
    if not coast_lines:
        coast_lines = [max(rust_lines, key=lambda l: l.length)]
    main_coast = unary_union(coast_lines)
    corridor = main_coast.buffer(150.0)

    ref_pts_all = np.vstack(ref_by_date[date])
    in_corridor = np.array([corridor.contains(Point(p)) for p in ref_pts_all])
    ref_pts = ref_pts_all[in_corridor]
    frac_ref_excluded = 1.0 - in_corridor.mean()

    ref_line_parts = [LineString(p) for p in ref_by_date[date] if len(p) >= 2]
    ref_geom = unary_union(ref_line_parts)

    # CoastSat -> strandline (solo la costa principal de rust)
    d_ref2rust = np.array([main_coast.distance(Point(p)) for p in ref_pts])
    # strandline -> CoastSat (vértices de la costa principal de rust)
    rust_pts = np.vstack([np.asarray(l.coords) for l in coast_lines])
    d_rust2ref = np.array([ref_geom.distance(Point(p)) for p in rust_pts])

    rows.append({
        "date": date,
        "n_ref_pts": int(len(ref_pts)),
        "frac_ref_excluded": float(frac_ref_excluded),
        "median_ref2rust": float(np.median(d_ref2rust)),
        "mean_ref2rust": float(np.mean(d_ref2rust)),
        "p90_ref2rust": float(np.percentile(d_ref2rust, 90)),
        "median_rust2ref": float(np.median(d_rust2ref)),
        "mean_rust2ref": float(np.mean(d_rust2ref)),
        "p90_rust2ref": float(np.percentile(d_rust2ref, 90)),
    })
    all_d_ref2rust.append(d_ref2rust)
    all_d_rust2ref.append(d_rust2ref)

    print(f"{date}: ref->rust mediana={np.median(d_ref2rust):.2f} m "
          f"media={np.mean(d_ref2rust):.2f} m p90={np.percentile(d_ref2rust, 90):.2f} m "
          f"| rust->ref mediana={np.median(d_rust2ref):.2f} m "
          f"(ref fuera de corredor: {frac_ref_excluded*100:.0f}%)")

if all_d_ref2rust:
    g1 = np.concatenate(all_d_ref2rust)
    g2 = np.concatenate(all_d_rust2ref)
    med1, med2 = np.median(g1), np.median(g2)
    print("\n================= RESULTADO GLOBAL =================")
    print(f"CoastSat->strandline: mediana={med1:.2f} m, media={g1.mean():.2f} m, "
          f"p90={np.percentile(g1, 90):.2f} m  (n={len(g1)})")
    print(f"strandline->CoastSat: mediana={med2:.2f} m, media={g2.mean():.2f} m, "
          f"p90={np.percentile(g2, 90):.2f} m  (n={len(g2)})")
    verdict = "GO" if max(med1, med2) < 5.0 else "NO-GO"
    print(f"\nCriterio (mediana < 5 m): {verdict}")

    with open(os.path.join(RESULTS, "comparison.json"), "w") as f:
        json.dump({
            "per_scene": rows,
            "global": {
                "median_ref2rust": float(med1),
                "median_rust2ref": float(med2),
                "mean_ref2rust": float(g1.mean()),
                "mean_rust2ref": float(g2.mean()),
                "p90_ref2rust": float(np.percentile(g1, 90)),
                "p90_rust2ref": float(np.percentile(g2, 90)),
            },
            "verdict": verdict,
        }, f, indent=2)
