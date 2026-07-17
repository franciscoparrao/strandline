#!/usr/bin/env python3
"""
Extiende el experimento profile_sr (Viña, Arica) a los otros 4 sitios del
benchmark del paper: Coquimbo, Valdivia, Chiloé, Constitución. Compara 3
variantes contra el mismo ground truth real (`cross_distance_clean` de los
pickles del paper, en fechas exactas coincidentes con escenas SR fetched):

  1. Otsu + intersect            (baseline actual)
  2. Otsu --subpixel-refine + intersect  (adenda 10, refina el contorno)
  3. profile sembrado desde Otsu  (adenda 9, ajuste subpixel por transecto)

Cada escena puede tener su propio EPSG (Constitución cruza zona UTM), así
que los transectos se reproyectan por escena, no por sitio.
"""
import csv
import json
import pickle
import subprocess
from pathlib import Path

import numpy as np
import rasterio
from pyproj import Transformer

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
SCRATCH = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/88ae1da2-b2ac-4fce-a9e1-41751377bb3c/scratchpad/multisite4")
EXP = SCRATCH / "out"
EXP.mkdir(exist_ok=True, parents=True)
PAPER_OUT = Path("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output")

SITES = {
    "COQUIMBO": {"pkl": "COQUIMBO_analysis.pkl", "native_epsg": 32719},
    "VALDIVIA": {"pkl": "VALDIVIA_analysis.pkl", "native_epsg": 32718},
    "CHILOE": {"pkl": "CHILOE_analysis.pkl", "native_epsg": 32718},
    "CONSTITUCION": {"pkl": "CONSTITUCION_analysis.pkl", "native_epsg": 32719},
}
SPACING, LENGTH = 100.0, 500.0


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("FAIL:", " ".join(cmd), "\n", r.stdout, r.stderr)
        return False
    return True


def build_baseline_and_transects(site, ref_shoreline, native_epsg, outdir):
    baseline_csv = outdir / f"{site}_baseline_{native_epsg}.csv"
    with open(baseline_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["easting", "northing"])
        w.writerows((f"{x:.6f}", f"{y:.6f}") for x, y in ref_shoreline)

    transects_csv = outdir / f"{site}_transects_{native_epsg}.csv"
    ok = run([STRANDLINE, "transects", "--baseline", str(baseline_csv),
              "--spacing", str(SPACING), "--length", str(LENGTH),
              "--seaward-e=-1", "--seaward-n=0", "--output", str(transects_csv)])
    if not ok:
        return None
    return transects_csv


def reproject_transects(transects_csv, from_epsg, to_epsg, out_csv):
    if from_epsg == to_epsg:
        return transects_csv
    t = Transformer.from_crs(f"EPSG:{from_epsg}", f"EPSG:{to_epsg}", always_xy=True)
    rows = list(csv.DictReader(open(transects_csv)))
    with open(out_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["name", "origin_e", "origin_n", "end_e", "end_n"])
        for r in rows:
            oe, on = t.transform(float(r["origin_e"]), float(r["origin_n"]))
            ee, en = t.transform(float(r["end_e"]), float(r["end_n"]))
            w.writerow([r["name"], f"{oe:.6f}", f"{on:.6f}", f"{ee:.6f}", f"{en:.6f}"])
    return out_csv


def geojson_to_shorelines_csv(geojson_path, out_csv):
    d = json.load(open(geojson_path))
    with open(out_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["shoreline_id", "easting", "northing"])
        for feat in d["features"]:
            for x, y in feat["geometry"]["coordinates"]:
                w.writerow(["s0", f"{x:.3f}", f"{y:.3f}"])


def read_chainage_csv(path, col="chainage_m"):
    out = {}
    if not path.exists():
        return out
    with open(path) as f:
        for row in csv.DictReader(f):
            v = row.get(col, "")
            if v not in ("", "nan", "NaN"):
                out[row["transect"]] = float(v)
    return out


def process_scene(site, ms, mask, native_epsg, native_transects_csv):
    with rasterio.open(ms) as src:
        scene_epsg = src.crs.to_epsg()
    stem = ms.stem.replace("_ms", "")
    transects_csv = reproject_transects(
        native_transects_csv, native_epsg, scene_epsg, EXP / f"{site}_{stem}_transects_{scene_epsg}.csv"
    )

    results = {}

    # 1) Otsu crudo + intersect
    line_raw = EXP / f"{site}_{stem}_raw.geojson"
    if run([STRANDLINE, "extract", "--ms", str(ms), "--mask", str(mask), "--output", str(line_raw)]):
        if json.load(open(line_raw))["features"]:
            sh_csv = EXP / f"{site}_{stem}_raw_sh.csv"
            geojson_to_shorelines_csv(line_raw, sh_csv)
            inter_csv = EXP / f"{site}_{stem}_raw_inter.csv"
            if run([STRANDLINE, "intersect", "--transects", str(transects_csv),
                    "--shorelines", str(sh_csv), "--output", str(inter_csv)]):
                results["otsu_intersect"] = read_chainage_csv(inter_csv)

    # 2) Otsu --subpixel-refine + intersect
    line_ref = EXP / f"{site}_{stem}_refined.geojson"
    if run([STRANDLINE, "extract", "--ms", str(ms), "--mask", str(mask),
            "--subpixel-refine", "--output", str(line_ref)]):
        if json.load(open(line_ref))["features"]:
            sh_csv = EXP / f"{site}_{stem}_refined_sh.csv"
            geojson_to_shorelines_csv(line_ref, sh_csv)
            inter_csv = EXP / f"{site}_{stem}_refined_inter.csv"
            if run([STRANDLINE, "intersect", "--transects", str(transects_csv),
                    "--shorelines", str(sh_csv), "--output", str(inter_csv)]):
                results["subpixel_extract_intersect"] = read_chainage_csv(inter_csv)

    # 3) profile sembrado
    profile_csv = EXP / f"{site}_{stem}_profile.csv"
    if run([STRANDLINE, "profile", "--ms", str(ms), "--mask", str(mask),
            "--transects", str(transects_csv), "--output", str(profile_csv)]):
        results["profile_seeded"] = read_chainage_csv(profile_csv)

    return results


def main():
    variants = ["otsu_intersect", "subpixel_extract_intersect", "profile_seeded"]
    global_errors = {v: [] for v in variants}
    per_site_summary = {}

    for site, cfg in SITES.items():
        pkl = pickle.load(open(PAPER_OUT / cfg["pkl"], "rb"))
        dates_str = [str(x)[:10] for x in pkl["dates"]]
        cross_distance = pkl["cross_distance_clean"]
        ref_shoreline = pkl["reference_shoreline"]
        native_epsg = cfg["native_epsg"]

        transects_csv = build_baseline_and_transects(site, ref_shoreline, native_epsg, EXP)
        if transects_csv is None:
            print(f"{site}: fallo generando transectos, se omite")
            continue

        site_errors = {v: [] for v in variants}
        n_dates = 0
        for ms in sorted((SCRATCH / site).glob("*_ms.tif")):
            date = ms.name[:10]
            if date not in dates_str:
                continue
            idx = dates_str.index(date)
            mask = ms.with_name(ms.name.replace("_ms.tif", "_mask.tif"))
            if not mask.exists():
                continue
            res = process_scene(site, ms, mask, native_epsg, transects_csv)
            if not any(res.values()):
                print(f"{site} {date}: sin resultados utilizables")
                continue
            n_dates += 1
            counts = []
            for v in variants:
                chainages = res.get(v, {})
                errs = []
                for tname, val in chainages.items():
                    if tname not in cross_distance:
                        continue
                    gt = cross_distance[tname][idx]
                    if np.isfinite(gt):
                        errs.append(abs(val - gt))
                site_errors[v].extend(errs)
                global_errors[v].extend(errs)
                counts.append(f"{v}={len(errs)}")
            print(f"{site} {date}: " + ", ".join(counts))

        print(f"\n=== {site}: {n_dates} fechas usadas ===")
        summary = {}
        for v in variants:
            e = np.array(site_errors[v])
            if len(e) == 0:
                print(f"  {v}: sin datos")
                continue
            summary[v] = (np.median(e), np.mean(e), len(e))
            print(f"  {v}: n={len(e)} mediana={np.median(e):.2f} m media={np.mean(e):.2f} m")
        per_site_summary[site] = summary

    print("\n================= RESUMEN GLOBAL (4 sitios nuevos) =================")
    for v in variants:
        e = np.array(global_errors[v])
        if len(e) == 0:
            continue
        print(f"{v}: n={len(e)} mediana={np.median(e):.2f} m media={np.mean(e):.2f} m p90={np.percentile(e,90):.2f} m")


if __name__ == "__main__":
    main()
