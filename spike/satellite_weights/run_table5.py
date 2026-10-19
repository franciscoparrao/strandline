#!/usr/bin/env python3
"""§5.6 / Tabla 5 — desviación típica del residuo por satélite, por sitio y conjunto.

Antes de este script la Tabla 5 no tenía fuente versionada (la calibración de
julio fue ad hoc). Aquí se calcula con el subcomando del motor,
``strandline satellite-weights`` (detrend OLS por transecto, residuos agrupados
por satélite), sobre dos conjuntos:

- ``original``: series del estudio de referencia (``output/``) y etiquetas de
  satélite de ``/mnt/kingston/{SITE}/{SITE}_output.pkl``, alineadas por fecha.
  Viña usa ``cross_distance_uncorrected`` de ``*_tidal_corrected.pkl``.
- ``ocean``: series del reanálisis oceánico (``output_reprocessed/``) y
  etiquetas de las shorelines re-extraídas con máscara, alineadas por fecha.

La serie de cada transecto se usa tal cual viene en el pickle (ya limpia de
picos donde corresponde); ``satellite-weights`` ignora los transectos con menos
de 10 observaciones válidas.

Uso: run_table5.py   → table5.json y table5.txt en este directorio
"""
import csv
import json
import pickle
import subprocess
from pathlib import Path

import numpy as np

ST = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
P4 = Path("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera")
HERE = Path(__file__).resolve().parent
WORK = HERE / "work"
SITES = ["VINA_DEL_MAR", "COQUIMBO", "CONSTITUCION", "VALDIVIA", "CHILOE"]
SATS = ["L5", "L7", "L8", "L9"]


def sources(site, dataset):
    if dataset == "original":
        if site == "CHILOE":
            return None  # sin etiquetas de satélite en el conjunto original
        an = P4 / "output" / ("VINA_DEL_MAR_tidal_corrected.pkl" if site == "VINA_DEL_MAR" else f"{site}_analysis.pkl")
        out = Path("/mnt/kingston") / site / f"{site}_output.pkl"
    else:
        an = P4 / "output_reprocessed" / f"{site}_analysis_ocean.pkl"
        out = P4 / "data_reprocessed" / site / (f"{site}_output.pkl" if site == "CHILOE" else f"{site}_output_masked.pkl")
    return an, out


def calibrate(site, dataset):
    src = sources(site, dataset)
    if src is None:
        return None
    a = pickle.load(open(src[0], "rb"))
    o = pickle.load(open(src[1], "rb"))
    series = a["cross_distance_clean"] if "cross_distance_clean" in a else a["cross_distance_uncorrected"]
    sat_by_date = {}
    for d, s in zip(o["dates"], o["satname"]):
        sat_by_date.setdefault(str(d)[:19], str(s))
    dates = [str(d)[:19] for d in a["dates"]]
    unmatched = sum(d not in sat_by_date for d in dates)
    WORK.mkdir(exist_ok=True)
    inter, dcsv, out = (WORK / f"{site}_{dataset}_{x}" for x in ("inter.csv", "dates.csv", "weights.csv"))
    with open(inter, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["transect", "shoreline_id", "chainage_m"])
        for t, v in series.items():
            for i, x in enumerate(np.asarray(v, float)):
                if np.isfinite(x):
                    w.writerow([t, i, f"{x:.4f}"])
    with open(dcsv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["shoreline_id", "date", "satname"])
        for i, d in enumerate(dates):
            if d in sat_by_date:
                w.writerow([i, d[:10], sat_by_date[d]])
    r = subprocess.run([ST, "satellite-weights", "--intersections", str(inter), "--dates", str(dcsv),
                        "--output", str(out)], capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(r.stderr)
    std = {row["satname"]: (float(row["std_m"]), int(row["n_obs"])) for row in csv.DictReader(open(out))}
    return {"std_m": {s: std[s][0] for s in SATS if s in std},
            "n_obs": {s: std[s][1] for s in SATS if s in std},
            "n_transects": len(series), "dates_without_satname": unmatched}


def main():
    res = {ds: {s: calibrate(s, ds) for s in SITES} for ds in ("original", "ocean")}
    json.dump(res, open(HERE / "table5.json", "w"), indent=1)
    lines = []
    for ds in ("original", "ocean"):
        lines.append(f"== {ds}: desviación típica del residuo (m) por satélite")
        lines.append(f"{'sitio':14s} " + " ".join(f"{s:>7s}" for s in SATS) + "   más preciso   razón var L5/L8   fechas sin satname")
        for site, r in res[ds].items():
            if r is None:
                lines.append(f"{site:14s} (sin etiquetas de satélite)"); continue
            sd = r["std_m"]
            best = min(sd, key=sd.get)
            ratio = (sd["L5"] / sd["L8"]) ** 2 if "L5" in sd and "L8" in sd else float("nan")
            lines.append(f"{site:14s} " + " ".join(f"{sd.get(s, float('nan')):7.1f}" for s in SATS)
                         + f"   {best:>10s}   {ratio:13.2f}   {r['dates_without_satname']:>5d}")
        lines.append("")
    txt = "\n".join(lines)
    (HERE / "table5.txt").write_text(txt + "\n")
    print(txt)


if __name__ == "__main__":
    main()
