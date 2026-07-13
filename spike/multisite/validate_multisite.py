#!/usr/bin/env python3
"""Valida strandline (transects + rates + QC) contra los pickles del paper
para los 4 sitios pendientes del benchmark: ARICA, COQUIMBO, VALDIVIA, CHILOE.

Limitación conocida (igual que Constitución, "hallazgo E1"): estos pickles
NO retienen `satname` por fecha (solo el paper's raw *_output.pkl lo tenía,
y esos no se conservaron en disco) -> WLR no es comparable exacto (pesos
por satélite desconocidos). Se reporta igual mate, marcado como no-exacto.
NSM/SCE/EPR/LRR/R2/CI95 y la decisión QC (excluded) NO dependen de pesos
por satélite y sí deben coincidir exacto.
"""
import csv
import pickle
import subprocess
from pathlib import Path

import numpy as np

STRANDLINE = "/home/franciscoparrao/proyectos/strandline/target/release/strandline"
PAPER_OUT = Path("/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output")
SCRATCH = Path("/tmp/claude-1000/-home-franciscoparrao-proyectos-strandline/a601a99b-0806-4f58-a4a4-7a6043941e3a/scratchpad/multisite")
SCRATCH.mkdir(exist_ok=True)

SITES = ["ARICA", "COQUIMBO", "VALDIVIA", "CHILOE"]
SPACING, LENGTH = 100.0, 500.0


def load(site):
    with open(PAPER_OUT / f"{site}_analysis.pkl", "rb") as f:
        return pickle.load(f)


def validate_transects(site, d, outdir):
    ref = np.asarray(d["reference_shoreline"])
    baseline_csv = outdir / "baseline.csv"
    with open(baseline_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["easting", "northing"])
        w.writerows((f"{x:.6f}", f"{y:.6f}") for x, y in ref)

    out_csv = outdir / "transects_rust.csv"
    subprocess.run(
        [STRANDLINE, "transects", "--baseline", str(baseline_csv),
         "--spacing", str(SPACING), "--length", str(LENGTH),
         "--seaward-e=-1", "--seaward-n=0", "--output", str(out_csv)],
        check=True, capture_output=True, text=True,
    )
    rust = {r["name"]: r for r in csv.DictReader(open(out_csv))}
    paper = d["transects"]
    n_paper, n_rust = len(paper), len(rust)
    max_err = 0.0
    matched = 0
    for key, pts in paper.items():
        r = rust.get(key)
        if r is None:
            continue
        matched += 1
        (po_e, po_n), (pe_e, pe_n) = pts[0], pts[-1]
        err = max(
            abs(po_e - float(r["origin_e"])), abs(po_n - float(r["origin_n"])),
            abs(pe_e - float(r["end_e"])), abs(pe_n - float(r["end_n"])),
        )
        max_err = max(max_err, err)
    return {"n_paper": n_paper, "n_rust": n_rust, "matched": matched, "max_err_m": max_err}


def validate_rates(site, d, outdir, cfg):
    cross = d["cross_distance_clean"]
    dates = d["dates"]
    n_sl = len(dates)

    inter_csv = outdir / "intersections.csv"
    with open(inter_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["transect", "shoreline_id", "chainage_m"])
        for key, arr in cross.items():
            for i in range(n_sl):
                v = arr[i]
                if np.isfinite(v):
                    w.writerow([key, f"S{i:04d}", f"{v:.6f}"])

    dates_csv = outdir / "dates.csv"
    with open(dates_csv, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["shoreline_id", "date"])
        for i, dt in enumerate(dates):
            w.writerow([f"S{i:04d}", dt.strftime("%Y-%m-%dT%H:%M:%S")])

    out_csv = outdir / "rates_rust.csv"
    subprocess.run(
        [STRANDLINE, "rates", "--intersections", str(inter_csv), "--dates", str(dates_csv),
         "--min-valid", "10", "--qc",
         "--max-abs-lrr", str(cfg["max_abs_lrr"]), "--max-sce", str(cfg["max_sce"]),
         "--min-years", str(cfg["min_years"]), "--min-valid-qc", str(cfg["min_valid"]),
         "--min-r2", str(cfg["min_r2"]), "--output", str(out_csv)],
        check=True, capture_output=True, text=True,
    )
    rust = {r["transect"]: r for r in csv.DictReader(open(out_csv))}
    paper = d["rates_all"].set_index("transect")

    cols = ["NSM_m", "SCE_m", "EPR_m_yr", "LRR_m_yr", "LRR_r2", "LRR_ci95"]
    max_err = {c: 0.0 for c in cols}
    n_compared = 0
    qc_mismatch = 0
    n_valid_mismatch = 0
    for tname, prow in paper.iterrows():
        r = rust.get(tname)
        p_valid = int(prow["n_valid"])
        if r is None:
            if p_valid >= 10:
                n_valid_mismatch += 1
            continue
        n_compared += 1
        if int(r["n_valid"]) != p_valid:
            n_valid_mismatch += 1
        for c in cols:
            pv = prow[c]
            rv = float(r[c])
            if np.isfinite(pv) and np.isfinite(rv):
                max_err[c] = max(max_err[c], abs(pv - rv))
        p_excluded = bool(prow["excluded"])
        r_excluded = r["excluded"] == "True"
        if p_excluded != r_excluded:
            qc_mismatch += 1

    return {
        "n_paper": len(paper), "n_rust": len(rust), "n_compared": n_compared,
        "n_valid_mismatch": n_valid_mismatch, "qc_mismatch": qc_mismatch,
        "max_err": max_err,
    }


def main():
    print(f"{'sitio':10} {'transects (paper/rust/match/err_m)':38} | rates (n/match/nvalid_mis/qc_mis) | max err (NSM/SCE/EPR/LRR/R2/CI95)")
    for site in SITES:
        d = load(site)
        cfg = d["filter_config"]
        outdir = SCRATCH / site
        outdir.mkdir(exist_ok=True)

        tr = validate_transects(site, d, outdir)
        ra = validate_rates(site, d, outdir, cfg)

        print(f"\n=== {site} ===  {d['site_config']['desc']}")
        print(f"  transects: paper={tr['n_paper']} rust={tr['n_rust']} matched={tr['matched']} "
              f"max_err={tr['max_err_m']:.6f} m")
        print(f"  rates: paper={ra['n_paper']} rust={ra['n_rust']} comparados={ra['n_compared']} "
              f"n_valid_mismatch={ra['n_valid_mismatch']} qc_mismatch={ra['qc_mismatch']}")
        for c, e in ra["max_err"].items():
            print(f"    max err {c}: {e:.6f}")


if __name__ == "__main__":
    main()
