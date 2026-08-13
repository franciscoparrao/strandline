"""Benchmark de la intersección transecto×shoreline con protocolo de repeticiones.

Reemplaza la medición de una sola corrida que quedó en la bitácora (0.059 s vs
11.63 s) y que no reproducía: en dos re-corridas sueltas el reference Python dio
10.4 y 14.4 s. Aquí se mide con repeticiones intercaladas y se reporta mediana e
IQR, junto con el hardware y la carga de la máquina.

Qué se compara
--------------
El comparable justo es **cómputo contra cómputo**: la función vectorizada del
paper opera sobre arrays ya en memoria, y el CLI de strandline cronometra
internamente sólo `compute_intersections` (los CSV se leen antes de arrancar el
reloj y se escriben después). Se reporta además el wall-clock completo del
proceso — spawn + lectura de CSV + cómputo + escritura — que es lo que el
usuario efectivamente espera, y que penaliza a Rust, no al revés.

Las rondas se **intercalan** (python, rust_par, rust_1t) en vez de correr en
bloques, para que una racha de carga de la máquina afecte a los tres por igual
en lugar de sesgar a uno.

Uso:  python3 spike/transects/benchmark_intersect.py [--rounds N] [--warmup K]
Escribe spike/transects/benchmark_intersect.json
"""
import argparse
import csv
import json
import os
import platform
import re
import subprocess
import time

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
D = os.path.join(ROOT, "spike", "transects")
BIN = os.path.join(ROOT, "target", "release", "strandline")
N_SHORE = 200
SETTINGS = {
    "along_dist": 25,
    "min_points": 3,
    "max_std": 15,
    "max_range": 30,
    "min_chainage": -100,
    "multiple_inter": "auto",
    "auto_prc": 0.1,
}


# --------------------------------------------------------------- workload
def build_workload():
    """Transectos reales + shorelines sintéticas deterministas (seed 7)."""
    ts = {}
    with open(os.path.join(D, "transects_rust.csv")) as f:
        for row in csv.DictReader(f):
            ts[row["name"]] = np.array(
                [
                    [float(row["origin_e"]), float(row["origin_n"])],
                    [float(row["end_e"]), float(row["end_n"])],
                ]
            )
    keys = sorted(ts.keys())

    base = []
    with open(os.path.join(D, "reference_shoreline.csv")) as f:
        for row in csv.DictReader(f):
            base.append((float(row["easting"]), float(row["northing"])))
    base = np.array(base)

    dx, dy = np.diff(base[:, 0]), np.diff(base[:, 1])
    seg = np.sqrt(dx**2 + dy**2)
    cum = np.concatenate([[0], np.cumsum(seg)])
    dense_d = np.arange(0, cum[-1], 10.0)
    dense_e = np.interp(dense_d, cum, base[:, 0])
    dense_n = np.interp(dense_d, cum, base[:, 1])
    te, tn = np.gradient(dense_e), np.gradient(dense_n)
    tl = np.sqrt(te**2 + tn**2)
    tex, tny = te / tl, tn / tl
    nx, ny = tny, -tex
    flip = nx > 0
    nx[flip] *= -1
    ny[flip] *= -1

    np.random.seed(7)
    shorelines = []
    for i in range(N_SHORE):
        off = 30 * np.sin(i / 12) + np.random.randn() * 3
        disp = off + np.random.randn(len(dense_e)) * 1.5
        shorelines.append(np.column_stack([dense_e + disp * nx, dense_n + disp * ny]))

    path = os.path.join(D, "shorelines_synth.csv")
    with open(path, "w") as f:
        w = csv.writer(f)
        w.writerow(["shoreline_id", "easting", "northing"])
        for i, sl in enumerate(shorelines):
            for e, n in sl:
                w.writerow([f"S{i:03d}", f"{e:.4f}", f"{n:.4f}"])
    return ts, keys, shorelines


# ------------------------------------------- referencia vectorizada del paper
def compute_intersection_paper(shorelines, keys, ts, s):
    along = s["along_dist"]
    cross = {}
    n = len(shorelines)
    for key in keys:
        std_a = np.zeros(n)
        med_a = np.zeros(n)
        max_a = np.zeros(n)
        min_a = np.zeros(n)
        n_a = np.zeros(n)
        p1, p2 = ts[key][0], ts[key][-1]
        temp = p2 - p1
        seg_len = np.linalg.norm(temp)
        phi = np.arctan2(temp[1], temp[0])
        cph, sph = np.cos(phi), np.sin(phi)
        for i in range(n):
            sl = shorelines[i]
            dp = sl - p1
            crs = dp[:, 0] * temp[1] - dp[:, 1] * temp[0]
            d_line = np.abs(crs) / seg_len
            d_o = np.linalg.norm(dp, axis=1)
            idx = np.where((d_line <= along) & (d_o <= 1000))[0]
            if len(idx) == 0:
                std_a[i] = med_a[i] = max_a[i] = min_a[i] = n_a[i] = np.nan
            else:
                xyc = sl[idx] - p1
                xr = cph * xyc[:, 0] + sph * xyc[:, 1]
                xr[xr < s["min_chainage"]] = np.nan
                std_a[i] = np.nanstd(xr)
                med_a[i] = np.nanmedian(xr)
                max_a[i] = np.nanmax(xr)
                min_a[i] = np.nanmin(xr)
                n_a[i] = np.sum(~np.isnan(xr))
        c1 = std_a <= s["max_std"]
        c2 = (max_a - min_a) <= s["max_range"]
        c3 = n_a >= s["min_points"]
        good = c1 & c2 & c3
        if np.nansum(std_a > s["max_std"]) / n > s["auto_prc"]:
            med_a[~good] = max_a[~good]
            med_a[~c3] = np.nan
        else:
            med_a[~good] = np.nan
        cross[key] = med_a
    return cross


# ------------------------------------------------------------------- rust
RE_ELAPSED = re.compile(r"en ([\d.]+) s ->")


def run_rust(threads):
    """Devuelve (wall_clock_s, compute_s) de una invocación del CLI."""
    env = dict(os.environ)
    if threads is not None:
        env["RAYON_NUM_THREADS"] = str(threads)
    cmd = [
        BIN, "intersect",
        "--transects", os.path.join(D, "transects_rust.csv"),
        "--shorelines", os.path.join(D, "shorelines_synth.csv"),
        "--along-dist", str(SETTINGS["along_dist"]),
        "-o", os.path.join(D, "cross_rust.csv"),
    ]
    t0 = time.perf_counter()
    r = subprocess.run(cmd, capture_output=True, text=True, check=True, env=env)
    wall = time.perf_counter() - t0
    m = RE_ELAPSED.search(r.stderr)
    if not m:
        raise RuntimeError(f"no se pudo leer el timing interno de: {r.stderr!r}")
    return wall, float(m.group(1))


# ------------------------------------------------------------- correctitud
def check(cross_paper, keys):
    rust = {}
    with open(os.path.join(D, "cross_rust.csv")) as f:
        for row in csv.DictReader(f):
            rust.setdefault(row["transect"], {})[row["shoreline_id"]] = float(
                row["chainage_m"]
            )
    numeric, nan_mismatch = [], 0
    for key in keys:
        med = cross_paper[key]
        for si in range(N_SHORE):
            pv = med[si]
            rv = rust.get(key, {}).get(f"S{si:03d}", np.nan)
            if np.isnan(pv) and np.isnan(rv):
                continue
            if np.isnan(pv) != np.isnan(rv):
                nan_mismatch += 1
                continue
            numeric.append(abs(pv - rv))
    return {
        "n_crossings": len(numeric),
        "nan_mismatches": nan_mismatch,
        "max_abs_chainage_diff_m": float(max(numeric)),
        "median_abs_chainage_diff_m": float(np.median(numeric)),
    }


def stats(v):
    a = np.array(v, dtype=float)
    q1, q3 = np.percentile(a, [25, 75])
    return {
        "median_s": float(np.median(a)),
        "iqr_s": float(q3 - q1),
        "min_s": float(a.min()),
        "max_s": float(a.max()),
        "n": len(a),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rounds", type=int, default=15)
    ap.add_argument("--warmup", type=int, default=3)
    ap.add_argument(
        "--max-load", type=float, default=None,
        help="carga máxima tolerada al arrancar (default: 25%% de los hilos)")
    ap.add_argument("--force", action="store_true",
                    help="medir igual con la máquina cargada (marca el JSON como no válido)")
    ap.add_argument("--max-rel-iqr", type=float, default=0.20,
                    help="IQR/mediana máximo tolerado en las RAZONES (default 0.20)")
    ap.add_argument("--settle-s", type=int, default=180,
                    help="segundos a esperar a que la carga baje antes de abortar")
    args = ap.parse_args()

    if not os.path.exists(BIN):
        raise SystemExit(f"falta el binario release: {BIN}")

    # Guarda de contención. Una corrida bajo carga externa produce números que
    # parecen medidos y no lo son: en la primera corrida de este script (carga
    # 25 -> 51 por un job ajeno) el IQR de Python salió MAYOR que su mediana.
    max_load = args.max_load if args.max_load is not None else 0.25 * os.cpu_count()
    load0 = os.getloadavg()[0]
    # El loadavg tiene inercia de un minuto, así que tras una corrida previa
    # todavía arrastra la carga de ESA corrida. Se espera a que decaiga en vez
    # de abortar: si no, dos corridas seguidas son imposibles.
    waited = 0
    while load0 > max_load and not args.force and waited < args.settle_s:
        print(f"  carga {load0:.1f} > {max_load:.1f}; esperando a que baje "
              f"({waited}/{args.settle_s} s)", flush=True)
        time.sleep(15)
        waited += 15
        load0 = os.getloadavg()[0]
    if load0 > max_load and not args.force:
        raise SystemExit(
            f"carga actual {load0:.1f} > {max_load:.1f} tolerada en "
            f"{os.cpu_count()} hilos tras esperar {args.settle_s} s. La medición "
            f"no sería publicable; espera a que la máquina se desocupe o pasa "
            f"--force para medir igual."
        )

    ts, keys, shorelines = build_workload()
    print(f"workload: {len(keys)} transectos × {N_SHORE} shorelines", flush=True)

    load_before = os.getloadavg()
    loads = [load_before[0]]
    series = {"python": [], "rust_par_compute": [], "rust_par_wall": [],
              "rust_1t_compute": [], "rust_1t_wall": []}

    for r in range(args.rounds):
        loads.append(os.getloadavg()[0])
        t0 = time.perf_counter()
        cross_paper = compute_intersection_paper(shorelines, keys, ts, SETTINGS)
        series["python"].append(time.perf_counter() - t0)

        w, c = run_rust(None)
        series["rust_par_wall"].append(w)
        series["rust_par_compute"].append(c)

        w, c = run_rust(1)
        series["rust_1t_wall"].append(w)
        series["rust_1t_compute"].append(c)

        tag = "descartada (warm-up)" if r < args.warmup else ""
        print(
            f"  ronda {r + 1:2d}/{args.rounds}: python {series['python'][-1]:6.3f} s | "
            f"rust {series['rust_par_compute'][-1]:.4f} s | "
            f"rust-1t {series['rust_1t_compute'][-1]:.4f} s  {tag}",
            flush=True,
        )

    load_after = os.getloadavg()
    kept = {k: np.array(v[args.warmup:]) for k, v in series.items()}
    summary = {k: stats(v) for k, v in kept.items()}
    for k, s in summary.items():
        s["rel_iqr"] = s["iqr_s"] / s["median_s"]

    # La razón se calcula RONDA A RONDA, no dividiendo medianas: como las
    # rondas están intercaladas, numerador y denominador ven el mismo estado de
    # la máquina y la contención se cancela. Es notablemente más estable que
    # los tiempos absolutos — dos corridas del 2026-08-13 con medianas de
    # Python que difieren 13% dieron razones que coinciden a 0.1%.
    speedups = {}
    for name, den in (("compute_vs_python_parallel", "rust_par_compute"),
                      ("compute_vs_python_single_thread", "rust_1t_compute"),
                      ("wall_vs_python_parallel", "rust_par_wall")):
        q = kept["python"] / kept[den]
        s = stats(q)
        speedups[name] = {
            "median": s["median_s"], "iqr": s["iqr_s"],
            "rel_iqr": s["iqr_s"] / s["median_s"],
            "min": s["min_s"], "max": s["max_s"],
        }
    # El veredicto se toma sobre la dispersión de las RAZONES, que es lo que el
    # paper afirma, no sobre la de los tiempos absolutos.
    worst_rel_iqr = max(s["rel_iqr"] for s in speedups.values())

    out = {
        "workload": {"transects": len(keys), "shorelines": N_SHORE,
                     "settings": SETTINGS, "seed": 7},
        "protocol": {"rounds": args.rounds, "warmup_discarded": args.warmup,
                     "interleaved": True,
                     "rust_single_thread_via": "RAYON_NUM_THREADS=1",
                     "baseline_load": load_before[0],
                     "max_baseline_load_allowed": max_load,
                     "peak_load_during_run": max(loads),
                     "max_rel_iqr_allowed": args.max_rel_iqr,
                     "worst_rel_iqr": worst_rel_iqr,
                     # El veredicto NO se toma del loadavg pico: el loadavg
                     # incluye la carga del propio benchmark (Python fija un
                     # core ~9 s por ronda y rayon dispara 16 hilos), así que
                     # penalizaría a la medición por existir. Lo que decide es
                     # la dispersión de los propios tiempos: si la contención
                     # externa importó, se ve como IQR alto. En la corrida
                     # contendida del 2026-08-12 el IQR de Python fue 119% de
                     # su mediana; sin contención baja al orden del 10%.
                     "publishable": worst_rel_iqr <= args.max_rel_iqr},
        "hardware": {
            "cpu": next((l.split(":", 1)[1].strip()
                         for l in open("/proc/cpuinfo") if l.startswith("model name")), "?"),
            "logical_cpus": os.cpu_count(),
            "ram_gb": round(os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES") / 1e9, 1),
            "kernel": platform.release(),
            "python": platform.python_version(),
            "numpy": np.__version__,
            "rustc": subprocess.run(["rustc", "--version"], capture_output=True,
                                    text=True).stdout.strip(),
        },
        "loadavg_before": load_before,
        "loadavg_after": load_after,
        "correctness": check(cross_paper, keys),
        "timings": summary,
        "speedups": speedups,
        "raw": series,
    }
    with open(os.path.join(D, "benchmark_intersect.json"), "w") as f:
        json.dump(out, f, indent=2)

    print("\n=== resumen (mediana ± IQR sobre "
          f"{args.rounds - args.warmup} rondas) ===")
    for k in ("python", "rust_par_compute", "rust_1t_compute",
              "rust_par_wall", "rust_1t_wall"):
        s = summary[k]
        print(f"  {k:18s} {s['median_s']:8.4f} s  IQR {s['iqr_s']:.4f} "
              f"({100 * s['rel_iqr']:4.1f}%)  [{s['min_s']:.4f}, {s['max_s']:.4f}]")
    print("\n  razones por ronda (mediana, IQR relativo):")
    for lbl, k in (("cómputo, paralelo     ", "compute_vs_python_parallel"),
                   ("cómputo, single-thread", "compute_vs_python_single_thread"),
                   ("wall-clock del CLI    ", "wall_vs_python_parallel")):
        s = speedups[k]
        print(f"    {lbl} {s['median']:7.1f}×  IQR {100 * s['rel_iqr']:4.1f}%  "
              f"[{s['min']:.0f}, {s['max']:.0f}]")
    print(f"\n  correctitud: {out['correctness']}")
    print(f"  carga: base {load_before[0]:.1f} -> {load_after[0]:.1f} "
          f"(pico {max(loads):.1f}; incluye la del propio benchmark)")
    print(f"  peor IQR relativo: {100 * worst_rel_iqr:.1f}% "
          f"(tolerado {100 * args.max_rel_iqr:.0f}%)")
    if not out["protocol"]["publishable"]:
        print("\n  *** NO PUBLICABLE: dispersión demasiado alta. Los tiempos "
              "describen esta corrida, no el motor. ***")


if __name__ == "__main__":
    main()
