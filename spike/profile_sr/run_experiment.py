#!/usr/bin/env python3
"""
Experimento: ¿el ajuste subpixel S1 (`strandline profile`, sigmoide sobre
MNDWI por transecto) mejora la precisión sobre SR respecto al camino
Otsu+intersección de líneas ya usado (9.35 m documentado en la adenda 4),
sin tocar el problema del clasificador (E4, ya cerrado — no se usa
--classify en ningún paso de este experimento)?

Ground truth: cross_distance_uncorrected de los pickles del paper
(*_tidal_corrected.pkl / *_analysis.pkl), que coincide en fecha exacta con
varias de las escenas SR ya fetched (mismo satélite, mismo path/row, mismo
día de adquisición) — comparación real contra el procesamiento TOA
original del paper, no autoconsistencia.

Para cada sitio (VINA, ARICA) y cada fecha con match:
  1. `strandline extract` (Otsu global, sin --classify) -> shoreline
  2. `strandline intersect` -> chainage por transecto (comparable al
     camino "Otsu refinado" de la adenda 4)
  3. `strandline profile` (sin --classify) -> chainage subpixel S1
  4. compara ambos contra cross_distance_uncorrected[transect][fecha]
"""
import csv
import json
import os
import pickle
import subprocess
import sys
from pathlib import Path

import numpy as np
from pyproj import Transformer

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
SCRATCH = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/88ae1da2-b2ac-4fce-a9e1-41751377bb3c/scratchpad/classifier_sr")
EXP = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/88ae1da2-b2ac-4fce-a9e1-41751377bb3c/scratchpad/profile_exp")
EXP.mkdir(exist_ok=True)
PAPER_OUT = Path("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output")


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("FAIL:", " ".join(cmd))
        print(r.stdout, r.stderr)
        return False
    return True


def geojson_to_shorelines_csv(geojson_path, epsg, out_csv):
    d = json.load(open(geojson_path))
    with open(out_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["shoreline_id", "easting", "northing"])
        for feat in d["features"]:
            for x, y in feat["geometry"]["coordinates"]:
                w.writerow(["s0", f"{x:.3f}", f"{y:.3f}"])


def load_ground_truth(pkl_path, dist_key):
    with open(pkl_path, "rb") as f:
        d = pickle.load(f)
    dates_str = [str(x)[:10] for x in d["dates"]]
    cd = d[dist_key]
    return dates_str, cd


def experiment_site(site, scene_epsg, transects_csv, pkl_path, dist_key, dates_scenes):
    """dates_scenes: dict date -> (ms_path, mask_path)"""
    dates_str, cross_distance = load_ground_truth(pkl_path, dist_key)

    errors_intersect = []
    errors_profile = []
    n_dates_used = 0

    for date, (ms, mask) in dates_scenes.items():
        if date not in dates_str:
            print(f"{site} {date}: sin match en el pickle, se omite")
            continue
        idx = dates_str.index(date)
        stem = f"{site}_{date}"

        # 1) extract (Otsu global, sin clasificador)
        line_geojson = EXP / f"{stem}_line.geojson"
        ok = run([STRANDLINE, "extract", "--ms", str(ms), "--mask", str(mask),
                  "--output", str(line_geojson)])
        if not ok or not json.load(open(line_geojson))["features"]:
            print(f"{site} {date}: extract sin líneas, se omite")
            continue

        # 2) intersect
        shorelines_csv = EXP / f"{stem}_shorelines.csv"
        geojson_to_shorelines_csv(line_geojson, scene_epsg, shorelines_csv)
        inter_csv = EXP / f"{stem}_intersect.csv"
        ok = run([STRANDLINE, "intersect", "--transects", str(transects_csv),
                  "--shorelines", str(shorelines_csv), "--output", str(inter_csv)])
        if not ok:
            continue
        inter_chainage = {}
        with open(inter_csv) as f:
            for row in csv.DictReader(f):
                if row["chainage_m"] not in ("", "nan", "NaN"):
                    inter_chainage[row["transect"]] = float(row["chainage_m"])

        # 3) profile (subpixel S1, sin clasificador)
        profile_csv = EXP / f"{stem}_profile.csv"
        ok = run([STRANDLINE, "profile", "--ms", str(ms), "--mask", str(mask),
                  "--transects", str(transects_csv), "--output", str(profile_csv)])
        if not ok:
            continue
        profile_chainage = {}
        with open(profile_csv) as f:
            for row in csv.DictReader(f):
                if row["chainage_m"] not in ("", "nan", "NaN"):
                    profile_chainage[row["transect"]] = float(row["chainage_m"])

        n_this_date = 0
        for tname in cross_distance:
            gt_val = cross_distance[tname][idx]
            if not np.isfinite(gt_val):
                continue
            if tname in inter_chainage:
                errors_intersect.append(abs(inter_chainage[tname] - gt_val))
            if tname in profile_chainage:
                errors_profile.append(abs(profile_chainage[tname] - gt_val))
            n_this_date += 1
        n_dates_used += 1
        print(f"{site} {date}: {n_this_date} transectos con ground truth válido")

    def stats(name, errs):
        errs = np.array(errs)
        if len(errs) == 0:
            print(f"  {name}: sin datos")
            return
        print(f"  {name}: n={len(errs)} mediana={np.median(errs):.2f} m "
              f"media={np.mean(errs):.2f} m p90={np.percentile(errs,90):.2f} m")

    print(f"\n=== {site}: {n_dates_used} fechas usadas ===")
    stats("Otsu + intersect (chainage discreta)", errors_intersect)
    stats("profile (subpixel S1)", errors_profile)
    return errors_intersect, errors_profile


def main():
    # --- VINA ---
    vina_transects = "/home/franciscoparrao/proyectos/strandline/spike/profile/transects_32619.csv"
    vina_dates = {}
    for ms in sorted((SCRATCH / "VINA").glob("*_ms.tif")):
        date = ms.name[:10]
        mask = ms.with_name(ms.name.replace("_ms.tif", "_mask.tif"))
        if mask.exists():
            vina_dates[date] = (ms, mask)
    ei_vina, ep_vina = experiment_site(
        "VINA", 32619, vina_transects,
        PAPER_OUT / "VINA_DEL_MAR_tidal_corrected.pkl", "cross_distance_uncorrected",
        vina_dates,
    )

    # --- ARICA ---
    arica_transects = str(EXP / "arica_transects_32619.csv")
    arica_dates = {}
    for ms in sorted((SCRATCH / "ARICA").glob("*_ms.tif")):
        date = ms.name[:10]
        mask = ms.with_name(ms.name.replace("_ms.tif", "_mask.tif"))
        if mask.exists():
            arica_dates[date] = (ms, mask)
    ei_arica, ep_arica = experiment_site(
        "ARICA", 32619, arica_transects,
        PAPER_OUT / "ARICA_analysis.pkl", "cross_distance_clean",
        arica_dates,
    )

    print("\n=== RESUMEN GLOBAL (VINA + ARICA) ===")
    all_i = np.array(ei_vina + ei_arica)
    all_p = np.array(ep_vina + ep_arica)
    print(f"Otsu + intersect:  mediana={np.median(all_i):.2f} m  media={np.mean(all_i):.2f} m")
    print(f"profile subpixel:  mediana={np.median(all_p):.2f} m  media={np.mean(all_p):.2f} m")


if __name__ == "__main__":
    main()
