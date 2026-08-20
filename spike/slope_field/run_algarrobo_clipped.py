#!/usr/bin/env python3
"""Variante del experimento de Algarrobo con la linea base recortada a costa abierta.

La primera corrida uso la linea base completa que elige `baseline`: 8436 m que
incluyen el contorno del Humedal El Membrillo, donde la linea SE DOBLA SOBRE SI
MISMA (el azimut pasa de 189 a 26 grados en el arco 3.6-4.7 km). Ahi los
transectos apuntan tierra adentro, que es lo que producia el flip de `tide_sign`
entre la mitad norte y la sur.

Esta variante separa las dos causas candidatas del fracaso:
  (a) geometria: transectos mal orientados sobre costa que no es playa abierta;
  (b) fisica: la excursion mareal (~1 px de S2 en una playa de pendiente 0.13)
      esta bajo la resolucion del sensor.
Si al arreglar (a) el estimador sigue sin converger, la causa es (b).

Los tramos de costa abierta se detectan por la normal local: se conservan los
puntos cuya normal apunta al oeste (mar abierto en Algarrobo). Se usan los
tramos 1 y 3; el 2 se solapa en northing con ambos —topologia de spit/laguna—
y se excluye por ambiguo.

Uso: python3 spike/slope_field/run_algarrobo_clipped.py <dir_fetch> <dir_salida>
"""
import csv
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
PAPER_VENV = ("/home/franciscoparrao/proyectos/zenodo-gaps/"
              "propuesta-4-erosion-costera/.venv/bin/python3")
LON, LAT = -71.6542, -33.3475
PROFILES = {"Algarrobo 1": (252658, 6306085), "Algarrobo 2": (252707, 6306171),
            "Humedal": (253158, 6307284), "Algarrobo Norte": (253271, 6308596)}
NEAR_M = 300.0          # radio para considerar un transecto "en" un perfil
SEAWARD_MIN = -0.20     # componente este de la normal para contar como costa abierta
MIN_RUN_M = 400.0

SERIES = Path(sys.argv[1])
OUT = Path(sys.argv[2])
OUT.mkdir(parents=True, exist_ok=True)
BASE_FULL = Path("spike/slope_field/algarrobo_run/baseline.csv")


def open_coast_runs(p):
    d = np.linalg.norm(np.diff(p, axis=0), axis=1)
    s = np.concatenate([[0], np.cumsum(d)])
    keep = np.zeros(len(p), bool)
    for i in range(len(p)):
        a = np.searchsorted(s, s[i] - 100.0)
        b = min(len(p) - 1, np.searchsorted(s, s[i] + 100.0))
        if b <= a:
            continue
        t = p[b] - p[a]
        nx, ny = t[1], -t[0]
        n = np.hypot(nx, ny)
        if n > 0 and nx / n < SEAWARD_MIN:
            keep[i] = True
    runs, i = [], 0
    while i < len(keep):
        if keep[i]:
            j = i
            while j + 1 < len(keep) and keep[j + 1]:
                j += 1
            if s[j] - s[i] > MIN_RUN_M:
                runs.append((i, j, s[j] - s[i]))
            i = j + 1
        else:
            i += 1
    return runs


p = np.array([[float(r["easting"]), float(r["northing"])]
              for r in csv.DictReader(open(BASE_FULL))])
runs = open_coast_runs(p)
print(f"linea base completa: {len(p)} pts; tramos de costa abierta: "
      f"{[f'{L:.0f} m' for _, _, L in runs]}", flush=True)
# tramos 1 y 3 (indices 0 y 2): los que cubren los perfiles sin solaparse
chosen = [runs[0], runs[2]] if len(runs) >= 3 else runs

tr_rows = []
for k, (i, j, L) in enumerate(chosen):
    seg = OUT / f"baseline_{k}.csv"
    with open(seg, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["easting", "northing"])
        w.writerows((f"{x:.3f}", f"{y:.3f}") for x, y in p[i:j + 1])
    t_out = OUT / f"transects_{k}.csv"
    subprocess.run([STRANDLINE, "transects", "--baseline", str(seg),
                    "--spacing", "50", "--length", "400", "--smooth-window", "500",
                    "--seaward-e=-1", "--seaward-n=0",
                    "--output", str(t_out)], check=True)
    for r in csv.DictReader(open(t_out)):
        r["name"] = f"R{k}{r['name']}"
        tr_rows.append(r)

tr_csv = OUT / "transects.csv"
with open(tr_csv, "w", newline="") as f:
    w = csv.DictWriter(f, fieldnames=list(tr_rows[0].keys()))
    w.writeheader()
    w.writerows(tr_rows)
print(f"{len(tr_rows)} transectos sobre costa abierta", flush=True)

# ── profile por escena ──────────────────────────────────────────────
scenes = sorted(SERIES.glob("*_ms.tif"))
inter_rows, date_rows = [], []
ok = fail = 0
for ms in scenes:
    mask = Path(str(ms).replace("_ms.tif", "_mask.tif"))
    m = re.search(r"_(\d{8}T\d{6})_", ms.name)
    if not mask.exists() or not m:
        fail += 1
        continue
    sid = f"S{ok:03d}"
    ts = m.group(1)
    iso = f"{ts[0:4]}-{ts[4:6]}-{ts[6:8]}T{ts[9:11]}:{ts[11:13]}:{ts[13:15]}"
    prof = OUT / f"profile_{sid}.csv"
    r = subprocess.run([STRANDLINE, "profile", "--ms", str(ms), "--mask", str(mask),
                        "--mask-kind", "scl", "--transects", str(tr_csv),
                        "--georef-rmse", "3", "--output", str(prof)],
                       capture_output=True, text=True)
    if r.returncode != 0:
        fail += 1
        continue
    for row in csv.DictReader(open(prof)):
        inter_rows.append((row["transect"], sid, row["chainage_m"]))
    date_rows.append((sid, iso))
    ok += 1
print(f"profile: {ok} OK, {fail} fallidas, {len(inter_rows)} observaciones", flush=True)

with open(OUT / "intersections.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["transect", "shoreline_id", "chainage_m"])
    w.writerows(inter_rows)
with open(OUT / "dates.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["shoreline_id", "date"])
    w.writerows(date_rows)

tide_script = OUT / "_tides.py"
tide_script.write_text(f'''
import csv, numpy as np
from pyTMD.compute import tide_elevations
rows = list(csv.DictReader(open("{OUT}/dates.csv")))
dts = np.array([np.datetime64(r["date"]) for r in rows])
t = tide_elevations(np.array([{LON}]), np.array([{LAT}]), dts,
                    model="GOT5.5_extrapolated", type="time series",
                    standard="datetime", extrapolate=True, cutoff=20.0,
                    compressed=False).values.flatten()
print(f"marea GOT5.5: {{len(t)}} fechas, rango [{{t.min():.3f}}, {{t.max():.3f}}] m")
with open("{OUT}/tides.csv", "w", newline="") as f:
    w = csv.writer(f); w.writerow(["shoreline_id", "tide_m"])
    w.writerows((r["shoreline_id"], f"{{v:.6f}}") for r, v in zip(rows, t))
''')
subprocess.run([PAPER_VENV, str(tide_script)], check=True)

subprocess.run([STRANDLINE, "slope", "--intersections", str(OUT / "intersections.csv"),
                "--dates", str(OUT / "dates.csv"), "--tides", str(OUT / "tides.csv"),
                "--min-valid", "50", "--output", str(OUT / "slopes.csv")], check=True)

# ── comparacion contra los perfiles ─────────────────────────────────
sl = {r["transect"]: r for r in csv.DictReader(open(OUT / "slopes.csv"))}
tr = {r["name"]: (float(r["origin_e"]), float(r["origin_n"])) for r in tr_rows}
v = np.array([float(r["slope"]) for r in sl.values()])
b = np.array([r["at_boundary"] == "true" for r in sl.values()])
print(f"\nTODOS: n={len(v)}  en el borde {b.sum()}  "
      f"mediana global {np.median(v):.4f}  interiores {np.median(v[~b]):.4f}")

print("\ncerca de cada perfil (radio 300 m):")
for name, (e, n) in PROFILES.items():
    sel = [t for t, (te, tn) in tr.items()
           if np.hypot(te - e, tn - n) < NEAR_M and t in sl]
    if not sel:
        print(f"  {name:16s} sin transectos")
        continue
    vv = np.array([float(sl[t]["slope"]) for t in sel])
    bb = np.array([sl[t]["at_boundary"] == "true" for t in sel])
    er = np.array([float(sl[t]["energy_ratio"]) for t in sel])
    print(f"  {name:16s} n={len(vv):2d}  borde={bb.sum():2d}  "
          f"mediana={np.median(vv):.4f}  energy_ratio={np.median(er):.3f}")
print("\nterreno: invierno 0.105  verano 0.167")
