#!/usr/bin/env python3
"""Slope satelital en Algarrobo, contra los perfiles de terreno de la tesis Navarro.

Es la primera validacion del estimador S4 contra un dato independiente: hasta
ahora solo estaba validado en sintetico (+-0.01) y contra un valor de literatura
(~0.10). Algarrobo se elige sobre El Quisco porque tiene coordenadas UTM por
perfil y pendientes en el rango donde la comparacion discrimina.

Flujo: baseline automatico -> transects -> profile por escena -> intersections/
dates -> marea GOT5.5 (pyTMD, venv del paper) -> strandline slope.

Uso:  python3 spike/slope_field/run_algarrobo.py <dir_fetch> <dir_salida>
"""
import csv
import json
import re
import subprocess
import sys
from pathlib import Path

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
PAPER_VENV = ("/home/franciscoparrao/proyectos/zenodo-gaps/"
              "propuesta-4-erosion-costera/.venv/bin/python3")
# Punto para la marea GOT: centro de los cuatro perfiles de la tesis.
LON, LAT = -71.6542, -33.3475

SERIES = Path(sys.argv[1])
OUT = Path(sys.argv[2])
OUT.mkdir(parents=True, exist_ok=True)

scenes = sorted(SERIES.glob("*_ms.tif"))
print(f"{len(scenes)} escenas en la serie", flush=True)

# ── 1. baseline automatica ──────────────────────────────────────────
base_csv = OUT / "baseline.csv"
subprocess.run([STRANDLINE, "baseline", "--dir", str(SERIES),
                "--mask-kind", "scl", "--min-length", "200",
                "--output", str(base_csv)], check=True)

# ── 2. transectos shore-normal suavizados ───────────────────────────
# Algarrobo mira al oeste (Pacifico), igual que Viña: seaward = -E.
# La playa es chica (~2.5 km cubiertos por los perfiles), asi que se usa
# espaciado 50 m para densificar y largo 400 m para no cruzar la punta.
tr_csv = OUT / "transects.csv"
subprocess.run([STRANDLINE, "transects", "--baseline", str(base_csv),
                "--spacing", "50", "--length", "400", "--smooth-window", "500",
                "--seaward-e=-1", "--seaward-n=0",
                "--output", str(tr_csv)], check=True)
n_tr = sum(1 for _ in open(tr_csv)) - 1
print(f"{n_tr} transectos generados", flush=True)

# ── 3. profile por escena ───────────────────────────────────────────
inter_rows, date_rows = [], []
ok = fail = 0
for ms in scenes:
    mask = Path(str(ms).replace("_ms.tif", "_mask.tif"))
    if not mask.exists():
        fail += 1
        continue
    sid = f"S{ok:03d}"
    m = re.search(r"_(\d{8}T\d{6})_", ms.name)
    if not m:
        fail += 1
        continue
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
print(f"profile: {ok} escenas OK, {fail} fallidas, {len(inter_rows)} observaciones",
      flush=True)

with open(OUT / "intersections.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["transect", "shoreline_id", "chainage_m"])
    w.writerows(inter_rows)
with open(OUT / "dates.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["shoreline_id", "date"])
    w.writerows(date_rows)

# ── 4. marea GOT5.5 en el venv del paper ────────────────────────────
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

# ── 5. slope ────────────────────────────────────────────────────────
for tag, extra in (("", []), ("_despiked", ["--despike-max-change", "30"])):
    subprocess.run([STRANDLINE, "slope",
                    "--intersections", str(OUT / "intersections.csv"),
                    "--dates", str(OUT / "dates.csv"),
                    "--tides", str(OUT / "tides.csv"),
                    "--min-valid", "50", *extra,
                    "--output", str(OUT / f"slopes{tag}.csv")], check=True)
    rows = list(csv.DictReader(open(OUT / f"slopes{tag}.csv")))
    v = sorted(float(r["slope"]) for r in rows)
    if v:
        n = len(v)
        med = v[n // 2] if n % 2 else (v[n // 2 - 1] + v[n // 2]) / 2
        print(f"slope{tag or ' (crudo)'}: n={n}  mediana={med:.4f}  "
              f"p25={v[n // 4]:.4f}  p75={v[3 * n // 4]:.4f}", flush=True)

print("\nreferencia de terreno (tesis Navarro, pendiente efectiva sobre la banda "
      "de marea):\n  invierno (may 2023) 0.105   verano (oct 2023) 0.167")
