#!/usr/bin/env python3
"""Experimento E3+S6: slope satelital en Viña (micro-tidal) con serie S2 densa.

Flujo: baseline desde escena clara -> transects suavizados -> profile por
escena (subpixel + sigma) -> intersections/dates/tides CSVs -> strandline slope.
"""
import csv
import json
import re
import subprocess
import sys
from pathlib import Path

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
SCRATCH = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/a601a99b-0806-4f58-a4a4-7a6043941e3a/scratchpad")
SERIES = SCRATCH / "s2_series"
OUT = SCRATCH / "slope_s2"
BASELINE_SCENE = sys.argv[1] if len(sys.argv) > 1 else None

OUT.mkdir(exist_ok=True)
scenes = sorted(SERIES.glob("*_ms.tif"))
print(f"{len(scenes)} escenas en la serie")

# ── 1. baseline: extract sobre la escena de referencia ──────────────
base_ms = Path(BASELINE_SCENE) if BASELINE_SCENE else scenes[len(scenes) // 2]
base_mask = Path(str(base_ms).replace("_ms.tif", "_mask.tif"))
base_gj = OUT / "baseline_shoreline.geojson"
subprocess.run([STRANDLINE, "extract", "--ms", str(base_ms), "--mask", str(base_mask),
                "--mask-kind", "scl", "--output", str(base_gj)], check=True)

gj = json.load(open(base_gj))
lines = [f["geometry"]["coordinates"] for f in gj["features"]]
longest = max(lines, key=len)
print(f"baseline: {base_ms.name}, línea más larga {len(longest)} vértices")
with open(OUT / "baseline.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["easting", "northing"])
    w.writerows((f"{x:.3f}", f"{y:.3f}") for x, y in longest)

# ── 2. transectos shore-normal suavizados ───────────────────────────
subprocess.run([STRANDLINE, "transects", "--baseline", str(OUT / "baseline.csv"),
                "--spacing", "100", "--length", "600", "--smooth-window", "500",
                "--seaward-e=-1", "--seaward-n=0",
                "--output", str(OUT / "transects.csv")], check=True)
n_tr = sum(1 for _ in open(OUT / "transects.csv")) - 1
print(f"{n_tr} transectos generados")

# ── 3. profile por escena ───────────────────────────────────────────
inter_rows, date_rows = [], []
ok, fail = 0, 0
for ms in scenes:
    mask = Path(str(ms).replace("_ms.tif", "_mask.tif"))
    sid = f"S{ok + fail:03d}"
    # timestamp UTC desde el id de la escena (p.ej. 20240108T143739)
    m = re.search(r"_(\d{8}T\d{6})_", ms.name)
    ts = m.group(1)
    iso = f"{ts[0:4]}-{ts[4:6]}-{ts[6:8]}T{ts[9:11]}:{ts[11:13]}:{ts[13:15]}"
    prof = OUT / f"profile_{sid}.csv"
    r = subprocess.run([STRANDLINE, "profile", "--ms", str(ms), "--mask", str(mask),
                        "--mask-kind", "scl", "--transects", str(OUT / "transects.csv"),
                        "--georef-rmse", "3", "--output", str(prof)],
                       capture_output=True, text=True)
    if r.returncode != 0:
        print(f"  {ms.name}: profile falló ({r.stderr.strip().splitlines()[-1] if r.stderr else '?'})")
        fail += 1
        continue
    with open(prof) as f:
        rows = list(csv.DictReader(f))
    for row in rows:
        inter_rows.append((row["transect"], sid, row["chainage_m"]))
    date_rows.append((sid, iso))
    ok += 1
print(f"profile: {ok} escenas OK, {fail} fallidas, {len(inter_rows)} observaciones")

with open(OUT / "intersections.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["transect", "shoreline_id", "chainage_m"])
    w.writerows(inter_rows)
with open(OUT / "dates.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["shoreline_id", "date"])
    w.writerows(date_rows)
print("intersections.csv y dates.csv escritos; falta tides.csv (pyTMD) y slope")
