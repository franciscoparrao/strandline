#!/usr/bin/env python3
"""§5.4 v2 — dónde se muestrea el ajuste subpíxel, sobre dos conjuntos de datos.

Repite la comparación de la Tabla 3 del paper (contorno crudo, perfil sobre los
transectos fijos, contorno refinado sobre normales locales) con dos verdades:

- ``original``: transectos y ``cross_distance_clean`` del estudio de referencia
  (``output/{SITE}_analysis.pkl``). Es la base de la Tabla 3 de agosto, que el
  reanálisis de propuesta-4 mostró contaminada por agua interior.
- ``ocean``: transectos y distancias del reanálisis anclado a la costa oceánica
  (``output_reprocessed/{SITE}_analysis_ocean.pkl``).

Diferencias con ``run_experiment_multisite.py`` (v1), declaradas:
1. Las escenas SR de la v1 vivían en un scratchpad que ya no existe y su
   selección de fechas no quedó registrada. Aquí se vuelven a bajar con
   ``strandline fetch`` y se eligen con una regla fija: Landsat 8/9 (OLI),
   2018-2021, nubosidad ≤ 15 %, fecha presente en los DOS pickles del sitio,
   hasta 15 fechas repartidas en el tiempo. Las mismas escenas sirven a los dos
   conjuntos, así que la diferencia entre ellos es solo la verdad.
2. Los transectos se leen del propio pickle (en la v1 se regeneraban desde la
   referencia; para el conjunto original es lo mismo, validado a micrómetros),
   porque los transectos oceánicos tienen otra geometría y orientación.

Uso: run_geometry_v2.py fetch | run
"""
import csv
import json
import pickle
import subprocess
import sys
from pathlib import Path

import numpy as np
import rasterio
from pyproj import Transformer

ST = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
P4 = Path("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera")
HERE = Path(__file__).resolve().parent
SCENES = HERE / "scenes_v2"          # no versionado (.gitignore): regenerable con `fetch`
WORK = HERE / "work_v2"
EPSG = json.load(open(P4 / "data_reprocessed/site_epsg.json"))

COMPLEX = ["COQUIMBO", "CONSTITUCION", "VALDIVIA", "CHILOE"]
STRAIGHT = ["VINA_DEL_MAR", "ARICA"]
VARIANTS = ["otsu_intersect", "profile_seeded", "subpixel_extract_intersect"]
N_DATES, MAX_CLOUD, PERIOD = 15, 15, "2018-01-01/2022-01-01"


def pkl_path(site, dataset):
    if dataset == "original" or site == "ARICA":
        # Arica no se re-extrajo en el reanálisis (96 % de transectos sobre la costa)
        if site == "VINA_DEL_MAR":
            return P4 / "output" / "VINA_DEL_MAR_tidal_corrected.pkl"
        return P4 / "output" / f"{site}_analysis.pkl"
    return P4 / "output_reprocessed" / f"{site}_analysis_ocean.pkl"


def load(site, dataset):
    a = pickle.load(open(pkl_path(site, dataset), "rb"))
    # el pickle original de Viña no tiene cross_distance_clean; la v1 usaba la serie sin corregir
    key = "cross_distance_clean" if "cross_distance_clean" in a else "cross_distance_uncorrected"
    return {
        "transects": {k: np.asarray(v, float) for k, v in a["transects"].items()},
        "cdc": a[key],
        "dates": [str(d)[:10] for d in a["dates"]],
    }


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.returncode == 0


def bbox_lonlat(site):
    pts = np.vstack([t for ds in ("original", "ocean") for t in load(site, ds)["transects"].values()])
    tr = Transformer.from_crs(f"EPSG:{EPSG[site]}", "EPSG:4326", always_xy=True)
    m = 500.0
    lon, lat = tr.transform([pts[:, 0].min() - m, pts[:, 0].max() + m],
                            [pts[:, 1].min() - m, pts[:, 1].max() + m])
    return f"{min(lon):.5f},{min(lat):.5f},{max(lon):.5f},{max(lat):.5f}"


def fetch():
    for site in COMPLEX + STRAIGHT:
        out = SCENES / site
        out.mkdir(parents=True, exist_ok=True)
        ok = run([ST, "fetch", "--collection", "landsat-c2-l2", "--sensors", "oli",
                  "--bbox", bbox_lonlat(site), "--datetime", PERIOD,
                  "--max-cloud", str(MAX_CLOUD), "--max-items", "120", "--out-dir", str(out)])
        print(site, "fetch", "ok" if ok else "FALLÓ", len(list(out.glob("*_ms.tif"))), "escenas", flush=True)


def select_scenes(site):
    common = set(load(site, "original")["dates"]) & set(load(site, "ocean")["dates"])
    cands = sorted(p for p in (SCENES / site).glob("*_ms.tif") if p.name[:10] in common)
    if len(cands) > N_DATES:
        idx = np.linspace(0, len(cands) - 1, N_DATES).round().astype(int)
        cands = [cands[i] for i in sorted(set(idx))]
    return cands


def transects_csv(site, dataset, scene_epsg):
    path = WORK / f"{site}_{dataset}_{scene_epsg}_transects.csv"
    if not path.exists():
        tr = Transformer.from_crs(f"EPSG:{EPSG[site]}", f"EPSG:{scene_epsg}", always_xy=True)
        with open(path, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["name", "origin_e", "origin_n", "end_e", "end_n"])
            for k, t in load(site, dataset)["transects"].items():
                (oe, ee), (on, en) = tr.transform([t[0, 0], t[-1, 0]], [t[0, 1], t[-1, 1]])
                w.writerow([k, f"{oe:.3f}", f"{on:.3f}", f"{ee:.3f}", f"{en:.3f}"])
    return path


def chainages(path, col="chainage_m"):
    out = {}
    if path.exists():
        for row in csv.DictReader(open(path)):
            v = row.get(col, "")
            if v not in ("", "nan", "NaN"):
                out[row["transect"]] = float(v)
    return out


def lines_csv(geojson, out):
    feats = json.load(open(geojson))["features"]
    if not feats:
        return None
    with open(out, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["shoreline_id", "easting", "northing"])
        for ft in feats:
            for x, y in ft["geometry"]["coordinates"]:
                w.writerow(["s0", f"{x:.3f}", f"{y:.3f}"])
    return out


def scene_results(site, dataset, ms):
    mask = ms.with_name(ms.name.replace("_ms.tif", "_mask.tif"))
    with rasterio.open(ms) as src:
        epsg = src.crs.to_epsg()
    tcsv = transects_csv(site, dataset, epsg)
    stem = f"{site}_{dataset}_{ms.name[:-7]}"
    res = {}
    for v, flags in (("otsu_intersect", []), ("subpixel_extract_intersect", ["--subpixel-refine"])):
        gj = WORK / f"{site}_{ms.name[:-7]}_{v}.geojson"     # la línea no depende del conjunto
        if not gj.exists():
            run([ST, "extract", "--ms", str(ms), "--mask", str(mask), "--output", str(gj)] + flags)
        if gj.exists() and lines_csv(gj, WORK / f"{stem}_{v}_sl.csv"):
            inter = WORK / f"{stem}_{v}_inter.csv"
            if run([ST, "intersect", "--transects", str(tcsv), "--shorelines",
                    str(WORK / f"{stem}_{v}_sl.csv"), "--output", str(inter)]):
                res[v] = chainages(inter)
    prof = WORK / f"{stem}_profile.csv"
    if run([ST, "profile", "--ms", str(ms), "--mask", str(mask), "--transects", str(tcsv),
            "--output", str(prof)]):
        res["profile_seeded"] = chainages(prof)
    return res


def summarize(errs):
    e = np.asarray(errs)
    if len(e) == 0:
        return None
    return {"median": float(np.median(e)), "mean": float(np.mean(e)),
            "p90": float(np.percentile(e, 90)), "n": int(len(e))}


def run_all():
    WORK.mkdir(parents=True, exist_ok=True)
    report = {"rule": {"sensors": "oli", "period": PERIOD, "max_cloud": MAX_CLOUD, "n_dates": N_DATES},
              "sites": {}, "groups": {}}
    pooled = {(g, ds, v): [] for g in ("complex", "straight") for ds in ("original", "ocean") for v in VARIANTS}
    for site in COMPLEX + STRAIGHT:
        group = "complex" if site in COMPLEX else "straight"
        scenes = select_scenes(site)
        report["sites"][site] = {"group": group, "dates": [s.name[:10] for s in scenes]}
        for ds in ("original", "ocean"):
            d = load(site, ds)
            errs = {v: [] for v in VARIANTS}
            for ms in scenes:
                i = d["dates"].index(ms.name[:10])
                for v, ch in scene_results(site, ds, ms).items():
                    for t, val in ch.items():
                        if t in d["cdc"] and np.isfinite(d["cdc"][t][i]):
                            errs[v].append(abs(val - d["cdc"][t][i]))
            report["sites"][site][ds] = {v: summarize(errs[v]) for v in VARIANTS}
            for v in VARIANTS:
                pooled[(group, ds, v)] += errs[v]
            print(site, ds, {v: (round(s["median"], 2), s["n"]) if s else None
                             for v, s in report["sites"][site][ds].items()}, flush=True)
    for (g, ds, v), e in pooled.items():
        report["groups"].setdefault(g, {}).setdefault(ds, {})[v] = summarize(e)
    json.dump(report, open(HERE / "geometry_v2.json", "w"), indent=1)
    for g in ("complex", "straight"):
        for ds in ("original", "ocean"):
            print(f"\n== {g} · {ds}")
            for v in VARIANTS:
                s = report["groups"][g][ds][v]
                if s:
                    print(f"  {v:28s} mediana {s['median']:6.2f}  media {s['mean']:6.2f}  p90 {s['p90']:7.2f}  n {s['n']}")


if __name__ == "__main__":
    {"fetch": fetch, "run": run_all}[sys.argv[1]]()
