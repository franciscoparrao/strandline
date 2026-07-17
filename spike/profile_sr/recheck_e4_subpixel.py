#!/usr/bin/env python3
"""
Cierra el loop con el hallazgo E4 (adenda 4): el pipeline SR sin
clasificador (Otsu refinado, refine_radius por default) dio 9.35 m de
mediana vs la referencia CoastSat real (comparación inter-producto SR-30m
vs TOA-15m). Pregunta: ¿`extract --subpixel-refine` (adenda 10, refina el
contorno mismo con ajuste sigmoide sembrado) baja ese número?

Corre `extract` con y sin --subpixel-refine sobre las 6 escenas SR de Viña
que coinciden en fecha exacta con `coastsat_reference.geojson` (ground
truth real, TOA), y compara ambas con la misma metodología del spike
original (`spike/scripts/03_compare.py`): distancia punto-a-línea
simétrica, corredor de 150 m alrededor de la costa principal.
"""
import json
import subprocess
from pathlib import Path

import numpy as np
from pyproj import Transformer
from shapely.geometry import LineString, Point
from shapely.ops import unary_union

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
SCRATCH = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/88ae1da2-b2ac-4fce-a9e1-41751377bb3c/scratchpad/e4_recheck")
REF_PATH = Path("/home/franciscoparrao/proyectos/strandline/spike/results/coastsat_reference.geojson")
OUT = SCRATCH / "out"
OUT.mkdir(exist_ok=True)
TARGET_EPSG = 32719
CORRIDOR_M = 150.0


def load_reference():
    d = json.load(open(REF_PATH))
    ref_by_date = {}
    for feat in d["features"]:
        date = feat["properties"]["date"]
        pts = np.array(feat["geometry"]["coordinates"])
        ref_by_date.setdefault(date, []).append(pts)
    return ref_by_date


def run_extract(ms, mask, subpixel, out_geojson):
    cmd = [STRANDLINE, "extract", "--ms", str(ms), "--mask", str(mask), "--output", str(out_geojson)]
    if subpixel:
        cmd.append("--subpixel-refine")
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("FAIL:", r.stdout, r.stderr)
        return False
    return True


def compare(out_geojson, ref_lines_native):
    d = json.load(open(out_geojson))
    if not d["features"]:
        return None
    epsg = d["features"][0]["properties"]["epsg"]
    tf = Transformer.from_crs(f"EPSG:{epsg}", f"EPSG:{TARGET_EPSG}", always_xy=True)

    rust_lines = []
    for feat in d["features"]:
        coords = np.array(feat["geometry"]["coordinates"])
        x, y = tf.transform(coords[:, 0], coords[:, 1])
        rust_lines.append(LineString(np.column_stack([x, y])))

    coast_lines = [line_ for line_ in rust_lines if line_.length >= 800.0]
    if not coast_lines:
        coast_lines = [max(rust_lines, key=lambda line_: line_.length)]
    main_coast = unary_union(coast_lines)
    corridor = main_coast.buffer(CORRIDOR_M)

    ref_pts_all = np.vstack(ref_lines_native)
    in_corridor = np.array([corridor.contains(Point(p)) for p in ref_pts_all])
    ref_pts = ref_pts_all[in_corridor]
    if len(ref_pts) == 0:
        return None

    ref_geom = unary_union([LineString(p) for p in ref_lines_native if len(p) >= 2])
    d_ref2rust = np.array([main_coast.distance(Point(p)) for p in ref_pts])
    rust_pts = np.vstack([np.asarray(line_.coords) for line_ in coast_lines])
    d_rust2ref = np.array([ref_geom.distance(Point(p)) for p in rust_pts])
    return d_ref2rust, d_rust2ref, 1.0 - in_corridor.mean()


def main():
    ref_by_date = load_reference()
    ms_paths = sorted(SCRATCH.glob("*_ms.tif"))

    results = {False: {"ref2rust": [], "rust2ref": []}, True: {"ref2rust": [], "rust2ref": []}}

    for ms in ms_paths:
        date = ms.name[:10]
        if date not in ref_by_date:
            continue
        mask = ms.with_name(ms.name.replace("_ms.tif", "_mask.tif"))
        print(f"\n=== {date} ===")
        for subpixel in (False, True):
            tag = "subpixel" if subpixel else "crudo"
            out_geojson = OUT / f"{date}_{tag}.geojson"
            if not run_extract(ms, mask, subpixel, out_geojson):
                continue
            res = compare(out_geojson, ref_by_date[date])
            if res is None:
                print(f"  {tag}: sin comparación válida")
                continue
            d_ref2rust, d_rust2ref, frac_excl = res
            results[subpixel]["ref2rust"].append(d_ref2rust)
            results[subpixel]["rust2ref"].append(d_rust2ref)
            print(f"  {tag}: ref->rust mediana={np.median(d_ref2rust):.2f} m "
                  f"| rust->ref mediana={np.median(d_rust2ref):.2f} m "
                  f"(ref fuera de corredor: {frac_excl*100:.0f}%)")

    print("\n================= RESULTADO GLOBAL =================")
    for subpixel in (False, True):
        g1 = np.concatenate(results[subpixel]["ref2rust"])
        g2 = np.concatenate(results[subpixel]["rust2ref"])
        tag = "CON --subpixel-refine" if subpixel else "SIN --subpixel-refine (baseline actual, ~9.35 m)"
        print(f"\n{tag}:")
        print(f"  ref->rust:  mediana={np.median(g1):.2f} m  media={g1.mean():.2f} m  p90={np.percentile(g1,90):.2f} m  (n={len(g1)})")
        print(f"  rust->ref:  mediana={np.median(g2):.2f} m  media={g2.mean():.2f} m  p90={np.percentile(g2,90):.2f} m  (n={len(g2)})")


if __name__ == "__main__":
    main()
