#!/usr/bin/env python3
"""
Spike strandline — Paso 2: shorelines CoastSat de referencia (ground truth).

pan_off=True para que CoastSat trabaje sobre los mismos píxeles de 15 m
del ms tif que lee strandline (la comparación es engine-vs-engine sobre
input idéntico, sin pansharpening de por medio).

Mide el tiempo de extract_shorelines para el benchmark de velocidad.
"""

import os
import sys
import time
import json
import pickle
import warnings

warnings.filterwarnings("ignore")

COASTSAT_DIR = os.path.expanduser(
    "~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat"
)
sys.path.insert(0, COASTSAT_DIR)

import matplotlib
matplotlib.use("Agg")

from coastsat import SDS_download, SDS_shoreline, SDS_tools

GEE_PROJECT = "1052232428380"
SDS_download.authenticate_and_initialize(GEE_PROJECT)

polygon = [[[-71.563, -32.965],
            [-71.490, -32.965],
            [-71.490, -33.025],
            [-71.563, -33.025],
            [-71.563, -32.965]]]
polygon = SDS_tools.smallest_rectangle(polygon)

sitename = "VINA_SPIKE"
filepath_data = os.path.expanduser("~/proyectos/strandline/spike/data")
results_dir = os.path.expanduser("~/proyectos/strandline/spike/results")

inputs = {
    "polygon": polygon,
    "dates": ["2020-01-01", "2020-05-01"],
    "sat_list": ["L8"],
    "sitename": sitename,
    "filepath": filepath_data,
    "landsat_collection": "C02",
}

with open(os.path.join(filepath_data, sitename, f"{sitename}_metadata.pkl"), "rb") as f:
    metadata = pickle.load(f)

settings = {
    "cloud_thresh": 0.05,        # solo escenas despejadas para el spike
    "dist_clouds": 300,
    "output_epsg": 32719,
    "check_detection": False,
    "adjust_detection": False,
    "save_figure": False,
    "min_beach_area": 500,
    "min_length_sl": 200,
    "cloud_mask_issue": False,
    "sand_color": "default",
    "pan_off": True,             # mismo input de 15 m que strandline
    "s2cloudless_prob": 40,
    "inputs": inputs,
}

print("Extrayendo shorelines de referencia con CoastSat...")
t0 = time.perf_counter()
output = SDS_shoreline.extract_shorelines(metadata, settings)
elapsed = time.perf_counter() - t0

output = SDS_tools.remove_duplicates(output)

n = len(output["shorelines"])
print(f"\nShorelines extraídas: {n} en {elapsed:.1f} s "
      f"({elapsed / max(n, 1):.1f} s/escena procesada)")

# Guardar como GeoJSON por fecha (EPSG:32719)
features = []
for i, (date, sl) in enumerate(zip(output["dates"], output["shorelines"])):
    if len(sl) == 0:
        continue
    features.append({
        "type": "Feature",
        "properties": {
            "date": date.strftime("%Y-%m-%d"),
            "datetime": date.isoformat(),
            "satname": output["satname"][i],
            "cloud_cover": float(output["cloud_cover"][i]),
        },
        # MultiPoint: CoastSat entrega la shoreline como nube de puntos ordenados
        "geometry": {
            "type": "MultiPoint",
            "coordinates": [[float(x), float(y)] for x, y in sl],
        },
    })

gj = {"type": "FeatureCollection", "features": features, "crs_epsg": 32719}
out_path = os.path.join(results_dir, "coastsat_reference.geojson")
with open(out_path, "w") as f:
    json.dump(gj, f)
print(f"Referencia guardada: {out_path}")

with open(os.path.join(results_dir, "coastsat_output.pkl"), "wb") as f:
    pickle.dump(output, f)

with open(os.path.join(results_dir, "coastsat_timing.json"), "w") as f:
    json.dump({"elapsed_s": elapsed, "n_shorelines": n,
               "dates": [d.strftime("%Y-%m-%d") for d in output["dates"]]}, f, indent=2)
