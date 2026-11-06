#!/usr/bin/env python3
"""Paridad de `extract --reference` con `reference_shoreline` de CoastSat.

Mismas escenas L8 TOA de 15 m del spike (Viña, 2020-01/04, pan_off=True), misma
referencia oceánica (propuesta-4, data_reprocessed/VINA_DEL_MAR/ocean_reference.npy,
EPSG:32719) y la misma franja de 300 m en los dos motores. strandline corre en
modo compatible (umbral del quirk de CoastSat). Métrica del spike: distancia
punto-a-línea simétrica por escena, mediana/media/p90.

Uso: parity_reference.py   → parity.txt y parity.json en este directorio
"""
import glob, json, os, pickle, subprocess, sys, warnings
from pathlib import Path
import numpy as np
warnings.filterwarnings("ignore")
sys.path.insert(0, os.path.expanduser("~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat"))
import matplotlib; matplotlib.use("Agg")
from coastsat import SDS_shoreline, SDS_tools
from shapely.geometry import LineString, Point

HERE = Path(__file__).resolve().parent
WORK = HERE / "work"; WORK.mkdir(exist_ok=True)
ST = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
DATA = Path("/home/franciscoparrao/proyectos/strandline/spike/data")
REF = np.load("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/data_reprocessed/VINA_DEL_MAR/ocean_reference.npy")[:, :2]
MAX_DIST = 300

# 1) CoastSat con referencia
metadata = pickle.load(open(DATA / "VINA_SPIKE" / "VINA_SPIKE_metadata.pkl", "rb"))
inputs = {"polygon": SDS_tools.smallest_rectangle([[[-71.563, -32.965], [-71.490, -32.965],
          [-71.490, -33.025], [-71.563, -33.025], [-71.563, -32.965]]]),
          "dates": ["2020-01-01", "2020-05-01"], "sat_list": ["L8"], "sitename": "VINA_SPIKE",
          "filepath": str(DATA), "landsat_collection": "C02"}
settings = {"cloud_thresh": 0.05, "dist_clouds": 300, "output_epsg": 32719, "check_detection": False,
            "adjust_detection": False, "save_figure": False, "min_beach_area": 500, "min_length_sl": 200,
            "cloud_mask_issue": False, "sand_color": "default", "pan_off": True, "s2cloudless_prob": 40,
            "inputs": inputs, "reference_shoreline": REF, "max_dist_ref": MAX_DIST}
out = SDS_tools.remove_duplicates(SDS_shoreline.extract_shorelines(metadata, settings))
cs = {d.strftime("%Y-%m-%d"): np.asarray(sl) for d, sl in zip(out["dates"], out["shorelines"]) if len(sl)}

# 2) strandline con la misma referencia
ref_csv = WORK / "vina_ocean_reference.csv"
np.savetxt(ref_csv, REF, delimiter=",", header="easting,northing", comments="", fmt="%.3f")
rows, all_d = [], []
for ms in sorted(glob.glob(str(DATA / "VINA_SPIKE" / "L8" / "ms" / "*_ms.tif"))):
    date = os.path.basename(ms)[:10]
    if date not in cs:
        continue
    mask = ms.replace("/ms/", "/mask/").replace("_ms.tif", "_mask.tif")
    gj = WORK / f"st_{date}.geojson"
    subprocess.run([ST, "extract", "--ms", ms, "--mask", mask, "--coastsat-compat",
                    "--reference", str(ref_csv), "--reference-epsg", "32719",
                    "--max-dist-ref", str(MAX_DIST), "--output-epsg", "32719", "--output", str(gj)],
                   check=True, capture_output=True)
    feats = json.load(open(gj))["features"]
    if not feats:
        rows.append((date, None)); continue
    st_lines = [LineString(f["geometry"]["coordinates"]) for f in feats if len(f["geometry"]["coordinates"]) > 1]
    cs_pts = cs[date]
    d1 = [min(l.distance(Point(p)) for l in st_lines) for p in cs_pts]
    st_pts = [p for l in st_lines for p in l.coords]
    cs_line = LineString(cs_pts)
    d2 = [cs_line.distance(Point(p)) for p in st_pts]
    d = np.array(d1 + d2); all_d.append(d)
    rows.append((date, (float(np.median(d)), float(d.mean()), float(np.percentile(d, 90)), len(d))))

lines = ["fecha        mediana   media     p90     n"]
for date, r in rows:
    lines.append(f"{date}  " + ("sin líneas" if r is None else f"{r[0]:7.2f} {r[1]:7.2f} {r[2]:7.2f} {r[3]:5d}"))
g = np.concatenate(all_d)
lines.append(f"GLOBAL      {np.median(g):7.2f} {g.mean():7.2f} {np.percentile(g, 90):7.2f} {len(g):5d}")
(HERE / "parity.txt").write_text("\n".join(lines) + "\n")
json.dump({"rows": rows, "global": [float(np.median(g)), float(g.mean()), float(np.percentile(g, 90))]},
          open(HERE / "parity.json", "w"), indent=1)
print("\n".join(lines))
