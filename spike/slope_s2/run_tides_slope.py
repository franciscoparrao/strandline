#!/usr/bin/env python3
"""Paso final del experimento E3+S6: marea GOT5.5 a las horas S2 + slope.
Correr con el python del venv del paper (tiene pyTMD)."""
import csv
import subprocess
from pathlib import Path

import numpy as np
from pyTMD.compute import tide_elevations

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
OUT = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/a601a99b-0806-4f58-a4a4-7a6043941e3a/scratchpad/slope_s2")
LON, LAT = -71.53, -33.00  # mismo punto GOT del paper

rows = list(csv.DictReader(open(OUT / "dates.csv")))
dts = np.array([np.datetime64(r["date"]) for r in rows])
tides = tide_elevations(np.array([LON]), np.array([LAT]), dts,
                        model="GOT5.5_extrapolated", type="time series",
                        standard="datetime", extrapolate=True, cutoff=20.0,
                        compressed=False).values.flatten()
print(f"marea GOT5.5: {len(tides)} fechas, rango [{tides.min():.3f}, {tides.max():.3f}] m, "
      f"std {tides.std():.3f} m")
with open(OUT / "tides.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["shoreline_id", "tide_m"])
    w.writerows((r["shoreline_id"], f"{t:.6f}") for r, t in zip(rows, tides))

subprocess.run([STRANDLINE, "slope",
                "--intersections", str(OUT / "intersections.csv"),
                "--dates", str(OUT / "dates.csv"),
                "--tides", str(OUT / "tides.csv"),
                "--min-valid", "50",
                "--output", str(OUT / "slopes_s2.csv")], check=True)

res = list(csv.DictReader(open(OUT / "slopes_s2.csv")))
slopes = np.array([float(r["slope"]) for r in res])
er = np.array([float(r["energy_ratio"]) for r in res])
print(f"\n{len(res)} transectos con slope estimado")
print(f"slope: mediana {np.median(slopes):.4f}, p25 {np.percentile(slopes,25):.4f}, "
      f"p75 {np.percentile(slopes,75):.4f}")
print(f"energy_ratio: mediana {np.median(er):.3f}")
# ¿cuántos caen en el borde del rango de barrido? (síntoma E3 de no-identificabilidad)
lo, hi = slopes.min(), slopes.max()
print(f"en bordes: {np.sum(slopes == lo)} en min ({lo}), {np.sum(slopes == hi)} en max ({hi})")
