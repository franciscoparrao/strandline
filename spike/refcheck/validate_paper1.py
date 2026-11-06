#!/usr/bin/env python3
"""Valida `strandline refcheck` contra el diagnóstico del paper 1 de propuesta-4.

Mismos insumos que propuesta-4/scripts/27_elevation_diagnostic.py: transectos
retenidos del flujo original (anclas = punto medio), costa OSM del sitio y
Copernicus GLO-30. Compara el % de anclas a <=200 m de la costa y el % que pasa
ambos criterios contra manuscript_v2/figures/elevation_stats.json.

Uso: validate_paper1.py   → validation.txt en este directorio
"""
import csv, json, os, pickle, subprocess, sys
from pathlib import Path
import numpy as np

ST = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
P4 = Path("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera")
HERE = Path(__file__).resolve().parent
WORK = HERE / "work"; WORK.mkdir(exist_ok=True)
EPSG = json.load(open(P4 / "data_reprocessed/site_epsg.json"))
REF = json.load(open(P4 / "manuscript_v2/figures/elevation_stats.json"))
SITES = {  # sitio: (change_rates del flujo original, pickle con la geometría)
    "ARICA": ("ARICA_change_rates.csv", "ARICA_analysis.pkl"),
    "COQUIMBO": ("COQUIMBO_change_rates.csv", "COQUIMBO_analysis.pkl"),
    "VINA_DEL_MAR": ("VINA_DEL_MAR_change_rates_filtered.csv", "VINA_DEL_MAR_tidal_corrected.pkl"),
    "CONSTITUCION": ("CONSTITUCION_change_rates.csv", "CONSTITUCION_analysis.pkl"),
    "VALDIVIA": ("VALDIVIA_change_rates.csv", "VALDIVIA_analysis.pkl"),
    "CHILOE": ("CHILOE_change_rates.csv", "CHILOE_analysis.pkl"),
}
AWS = "https://copernicus-dem-30m.s3.amazonaws.com/{t}/{t}.tif"


def overpass_to_geojson(src, dst):
    j = json.load(open(src))
    lines = [[[p["lon"], p["lat"]] for p in e["geometry"]]
             for e in j["elements"] if e.get("geometry") and len(e["geometry"]) > 1]
    json.dump({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString", "coordinates": l}}
        for l in lines]}, open(dst, "w"))
    pts = np.array([p for l in lines for p in l])
    return pts.min(axis=0), pts.max(axis=0)


def dem_for(site, lo, hi):
    out = WORK / f"{site}_glo30.tif"
    if out.exists():
        return out
    tiles = []
    for la in range(int(np.floor(lo[1])), int(np.floor(hi[1])) + 1):
        for lo_ in range(int(np.floor(lo[0])), int(np.floor(hi[0])) + 1):
            t = (f"Copernicus_DSM_COG_10_{'S' if la < 0 else 'N'}{abs(la):02d}_00_"
                 f"{'W' if lo_ < 0 else 'E'}{abs(lo_):03d}_00_DEM")
            tiles.append("/vsicurl/" + AWS.format(t=t))
    vrt = WORK / f"{site}.vrt"
    subprocess.run(["gdalbuildvrt", "-q", str(vrt)] + tiles, check=True)
    subprocess.run(["gdal_translate", "-q", "-projwin", str(lo[0] - 0.01), str(hi[1] + 0.01),
                    str(hi[0] + 0.01), str(lo[1] - 0.01), "-a_srs", "EPSG:4326", str(vrt), str(out)], check=True)
    return out


def main():
    lines = [f"{'sitio':14s} {'n':>4s} | {'sobre costa':>11s} (paper 1) | {'ambos':>6s} (paper 1)"]
    for site, (frates, fpkl) in SITES.items():
        rates = [r for r in csv.DictReader(open(P4 / "output" / frates))
                 if str(r.get("excluded", "False")).lower() in ("false", "0")]
        an = pickle.load(open(P4 / "output" / fpkl, "rb"))
        tcsv = WORK / f"{site}_transects.csv"
        with open(tcsv, "w", newline="") as f:
            w = csv.writer(f); w.writerow(["name", "origin_e", "origin_n", "end_e", "end_n"])
            for r in rates:
                t = np.asarray(an["transects"][r["transect"]], float)
                w.writerow([r["transect"], t[0, 0], t[0, 1], t[-1, 0], t[-1, 1]])
        coast = WORK / f"{site}_coast.geojson"
        lo, hi = overpass_to_geojson(P4 / "data_reprocessed" / site / "coastline_osm.json", coast)
        dem = dem_for(site, lo, hi)
        out = WORK / f"{site}_refcheck.csv"
        r = subprocess.run([ST, "refcheck", "--transects", str(tcsv), "--transects-epsg", str(EPSG[site]),
                            "--coastline", str(coast), "--dem", str(dem), "--output", str(out)],
                           capture_output=True, text=True)
        if r.returncode != 0:
            sys.exit(r.stderr)
        rows = list(csv.DictReader(open(out)))
        n = len(rows)
        on = 100 * sum(x["pass_dist"] == "true" for x in rows) / n
        both = 100 * sum(x["pass"] == "true" and x["pass_elev"] == "true" for x in rows) / n
        ref = REF[f"{site}|original"]
        lines.append(f"{site:14s} {n:4d} | {on:10.1f}% ({ref['on_coast_pct']:5.1f}%) | "
                     f"{both:5.1f}% ({ref['both_pass_pct']:5.1f}%)   n paper 1 = {ref['n']}")
        print(lines[-1], flush=True)
    (HERE / "validation.txt").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
