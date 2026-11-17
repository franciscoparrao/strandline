"""Benchmark justo de la intersección transecto×shoreline (respuesta a la review
simulada C&G del 2026-11-12, issues de velocidad).

Qué agrega respecto de benchmark_intersect.py (Tabla 2 original):

1. Baselines más fuertes en Python, todos sobre el mismo workload y validados
   contra la misma salida:
   - ``coastsat``: ``compute_intersection_QC`` de CoastSat copiado literal
     (``coastsat/SDS_transects.py``, commit 98103cb, GPL-3.0). Es el código que
     corre un usuario de CoastSat.
   - ``python_loop``: la implementación del estudio previo (loop sobre
     transectos y shorelines, vectorizada sobre vértices). Era la referencia de
     la Tabla 2.
   - ``numpy_vec``: vectorizada sobre TODAS las shorelines a la vez por
     transecto (estadísticos por grupo con ordenamiento y ``reduceat``).
   - ``numba_1t`` / ``numba_par``: el mismo algoritmo compilado con numba, en
     un hilo y en paralelo sobre transectos (``prange``). Es el competidor
     compilado que tendría un usuario de Python.
2. Escalamiento: hilos (Rust y numba) y tamaño del problema (shorelines).
3. Memoria pico (RSS máximo) de cada implementación, medida en un proceso
   propio con ``/usr/bin/time``.

Protocolo (igual que la Tabla 2): rondas intercaladas, warm-up descartado,
razones formadas ronda a ronda, veredicto por la dispersión de las razones.
El tiempo de Rust es el del operador (el CLI lo imprime con resolución de
microsegundos); el wall-clock del CLI se reporta aparte.

Uso (con el entorno fijo):
    spike/transects/.venv-bench/bin/python spike/transects/benchmark_fair.py \
        [--mode speed|scaling|memory|all] [--rounds N] [--coastsat-rounds M]
Escribe spike/transects/benchmark_fair_<mode>.json
"""
import argparse
import json
import os
import platform
import re
import subprocess
import sys
import time

import numpy as np
import numba
from numba import njit, prange

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import benchmark_intersect as B  # workload, referencia y lector del CLI  # noqa: E402

D, BIN, SET = B.D, B.BIN, B.SETTINGS


# ---------------------------------------------------------------- CoastSat
def compute_intersection_QC(output, transects, settings):
    """Copia literal del cuerpo de CoastSat ``SDS_transects.compute_intersection_QC``
    (commit 98103cb, 2025-11-01), sin el docstring y sin las ramas de
    ``multiple_inter`` distintas de 'auto' que este workload no usa."""
    cross_dist = dict([])
    shorelines = output['shorelines']
    along_dist = settings['along_dist']
    for key in transects.keys():
        std_intersect = np.zeros(len(shorelines))
        med_intersect = np.zeros(len(shorelines))
        max_intersect = np.zeros(len(shorelines))
        min_intersect = np.zeros(len(shorelines))
        n_intersect = np.zeros(len(shorelines))
        for i in range(len(shorelines)):
            sl = shorelines[i]
            X0 = transects[key][0, 0]
            Y0 = transects[key][0, 1]
            temp = np.array(transects[key][-1, :]) - np.array(transects[key][0, :])
            phi = np.arctan2(temp[1], temp[0])
            Mrot = np.array([[np.cos(phi), np.sin(phi)], [-np.sin(phi), np.cos(phi)]])
            p1 = np.array([X0, Y0])
            p2 = transects[key][-1, :]
            d_line = np.abs(np.cross(p2 - p1, sl - p1) / np.linalg.norm(p2 - p1))
            d_origin = np.array([np.linalg.norm(sl[k, :] - p1) for k in range(len(sl))])
            idx_dist = np.logical_and(d_line <= along_dist, d_origin <= 1000)
            idx_close = np.where(idx_dist)[0]
            if len(idx_close) == 0:
                std_intersect[i] = np.nan
                med_intersect[i] = np.nan
                max_intersect[i] = np.nan
                min_intersect[i] = np.nan
                n_intersect[i] = np.nan
            else:
                xy_close = np.array([sl[idx_close, 0], sl[idx_close, 1]]) - np.tile(np.array([[X0],
                                   [Y0]]), (1, len(sl[idx_close])))
                xy_rot = np.matmul(Mrot, xy_close)
                xy_rot[0, xy_rot[0, :] < settings['min_chainage']] = np.nan
                std_intersect[i] = np.nanstd(xy_rot[0, :])
                med_intersect[i] = np.nanmedian(xy_rot[0, :])
                max_intersect[i] = np.nanmax(xy_rot[0, :])
                min_intersect[i] = np.nanmin(xy_rot[0, :])
                n_intersect[i] = np.sum(~np.isnan(xy_rot[0, :]))
        condition1 = std_intersect <= settings['max_std']
        condition2 = (max_intersect - min_intersect) <= settings['max_range']
        condition3 = n_intersect >= settings['min_points']
        idx_good = np.logical_and(np.logical_and(condition1, condition2), condition3)
        if settings['multiple_inter'] == 'auto':
            prc_over = np.sum(std_intersect > settings['max_std']) / len(std_intersect)
            if prc_over > settings['auto_prc']:
                med_intersect[~idx_good] = max_intersect[~idx_good]
                med_intersect[~condition3] = np.nan
            else:
                med_intersect[~idx_good] = np.nan
        cross_dist[key] = med_intersect
    return cross_dist


def run_coastsat(shorelines, keys, ts):
    return compute_intersection_QC({'shorelines': shorelines}, {k: ts[k] for k in keys}, SET)


# ------------------------------------------------------ NumPy vectorizado
def qc_auto(std_a, med_a, max_a, min_a, n_a, s, n):
    c1 = std_a <= s["max_std"]
    c2 = (max_a - min_a) <= s["max_range"]
    c3 = n_a >= s["min_points"]
    good = c1 & c2 & c3
    med = med_a.copy()
    if np.nansum(std_a > s["max_std"]) / n > s["auto_prc"]:
        med[~good] = max_a[~good]
        med[~c3] = np.nan
    else:
        med[~good] = np.nan
    return med


def prepare_concat(shorelines):
    xy = np.concatenate(shorelines)
    gid = np.concatenate([np.full(len(s), i) for i, s in enumerate(shorelines)])
    return xy, gid


def run_numpy_vec(xy, gid, n, keys, ts, s=SET):
    """Por transecto, todas las shorelines a la vez: máscara sobre el arreglo
    concatenado y estadísticos por grupo con ordenamiento + reduceat."""
    out = {}
    for key in keys:
        p1, p2 = ts[key][0], ts[key][-1]
        t = p2 - p1
        L = np.hypot(t[0], t[1])
        cph, sph = t[0] / L, t[1] / L
        dp = xy - p1
        d_line = np.abs(dp[:, 0] * t[1] - dp[:, 1] * t[0]) / L
        d_o = np.hypot(dp[:, 0], dp[:, 1])
        m = (d_line <= s["along_dist"]) & (d_o <= 1000)
        x = cph * dp[m, 0] + sph * dp[m, 1]
        g = gid[m]
        keep = x >= s["min_chainage"]  # las < min_chainage son NaN y no cuentan
        x, g = x[keep], g[keep]
        std_a = np.full(n, np.nan); med_a = np.full(n, np.nan)
        max_a = np.full(n, np.nan); min_a = np.full(n, np.nan)
        n_a = np.full(n, np.nan)
        # grupos con puntos cercanos pero todos bajo min_chainage: n = 0
        has_close = np.zeros(n, bool); has_close[gid[m]] = True
        n_a[has_close] = 0.0
        if len(x):
            o = np.lexsort((x, g))
            x, g = x[o], g[o]
            starts = np.flatnonzero(np.r_[True, g[1:] != g[:-1]])
            cnt = np.diff(np.r_[starts, len(x)])
            ug = g[starts]
            sm = np.add.reduceat(x, starts)
            mean = sm / cnt
            sq = np.add.reduceat((x - np.repeat(mean, cnt)) ** 2, starts)
            std_a[ug] = np.sqrt(sq / cnt)
            min_a[ug] = x[starts]
            max_a[ug] = x[starts + cnt - 1]
            mid = starts + (cnt - 1) // 2
            med_a[ug] = np.where(cnt % 2 == 1, x[mid], (x[mid] + x[np.minimum(mid + 1, len(x) - 1)]) / 2)
            n_a[ug] = cnt
        out[key] = qc_auto(std_a, med_a, max_a, min_a, n_a, s, n)
    return out


# ------------------------------------------------------------------ numba
@njit(cache=True)
def _one_transect(p1x, p1y, tx, ty, xy, starts, ends, along, minch, out5):
    L = np.hypot(tx, ty)
    cph = tx / L
    sph = ty / L
    n = len(starts)
    buf = np.empty(xy.shape[0])
    for i in range(n):
        k = 0
        close = 0
        for j in range(starts[i], ends[i]):
            dx = xy[j, 0] - p1x
            dy = xy[j, 1] - p1y
            if abs(dx * ty - dy * tx) / L <= along and np.hypot(dx, dy) <= 1000.0:
                close += 1
                v = cph * dx + sph * dy
                if v >= minch:
                    buf[k] = v
                    k += 1
        if close == 0:
            for c in range(5):
                out5[c, i] = np.nan
        elif k == 0:
            out5[0, i] = np.nan; out5[1, i] = np.nan; out5[2, i] = np.nan; out5[3, i] = np.nan
            out5[4, i] = 0.0
        else:
            v = buf[:k]
            out5[0, i] = np.std(v)
            out5[1, i] = np.median(v)
            out5[2, i] = v.max()
            out5[3, i] = v.min()
            out5[4, i] = k


@njit(cache=True)
def _all_serial(P, T, xy, starts, ends, along, minch, res):
    for t in range(P.shape[0]):
        _one_transect(P[t, 0], P[t, 1], T[t, 0], T[t, 1], xy, starts, ends, along, minch, res[t])


@njit(cache=True, parallel=True)
def _all_parallel(P, T, xy, starts, ends, along, minch, res):
    for t in prange(P.shape[0]):
        _one_transect(P[t, 0], P[t, 1], T[t, 0], T[t, 1], xy, starts, ends, along, minch, res[t])


def run_numba(xy, starts, ends, keys, ts, parallel, s=SET):
    P = np.array([ts[k][0] for k in keys]); T = np.array([ts[k][-1] - ts[k][0] for k in keys])
    n = len(starts)
    res = np.empty((len(keys), 5, n))
    f = _all_parallel if parallel else _all_serial
    f(P, T, xy, starts, ends, float(s["along_dist"]), float(s["min_chainage"]), res)
    return {k: qc_auto(res[i, 0], res[i, 1], res[i, 2], res[i, 3], res[i, 4], s, n)
            for i, k in enumerate(keys)}


# ------------------------------------------------------------- correctitud
def compare(ref, other, keys):
    nanmis, diffs = 0, []
    for k in keys:
        a, b = ref[k], other[k]
        na, nb = np.isnan(a), np.isnan(b)
        nanmis += int((na != nb).sum())
        ok = ~na & ~nb
        if ok.any():
            diffs.append(np.abs(a[ok] - b[ok]).max())
    return {"nan_mismatches": nanmis, "max_abs_diff_m": float(max(diffs) if diffs else 0.0)}


def stats(v):
    a = np.asarray(v, float)
    q1, q3 = np.percentile(a, [25, 75])
    return {"median": float(np.median(a)), "iqr": float(q3 - q1),
            "rel_iqr": float((q3 - q1) / np.median(a)), "min": float(a.min()),
            "max": float(a.max()), "n": len(a)}


def env_info():
    return {
        "cpu": next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo")
                     if l.startswith("model name")), "?"),
        "logical_cpus": os.cpu_count(),
        "ram_gb": round(os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES") / 1e9, 1),
        "kernel": platform.release(), "python": platform.python_version(),
        "numpy": np.__version__, "numba": numba.__version__,
        "rustc": subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip(),
        "coastsat_commit": "98103cb",
    }


def wait_quiet(max_load, settle_s, force):
    load = os.getloadavg()[0]; waited = 0
    while load > max_load and not force and waited < settle_s:
        print(f"  carga {load:.1f} > {max_load:.1f}; esperando ({waited}/{settle_s} s)", flush=True)
        time.sleep(15); waited += 15; load = os.getloadavg()[0]
    if load > max_load and not force:
        raise SystemExit(f"carga {load:.1f} > {max_load:.1f}: medición no publicable (usa --force para medir igual)")
    return load


def write_synth(shorelines):
    path = os.path.join(D, "shorelines_synth.csv")
    with open(path, "w") as f:
        f.write("shoreline_id,easting,northing\n")
        for i, sl in enumerate(shorelines):
            for e, n in sl:
                f.write(f"S{i:03d},{e:.4f},{n:.4f}\n")


def scaled_shorelines(base, n):
    """n shorelines: se repiten las 200 del workload (los tiempos crecen con n
    sin cambiar la distribución de cruces)."""
    return [base[i % len(base)] for i in range(n)]


# ------------------------------------------------------------------- modos
def mode_speed(args, ts, keys, shorelines):
    xy, gid = prepare_concat(shorelines)
    lens = np.array([len(s) for s in shorelines]); ends = np.cumsum(lens); starts = ends - lens
    # compilar numba antes de medir (la compilación no es parte del cómputo)
    run_numba(xy, starts, ends, keys, ts, False); run_numba(xy, starts, ends, keys, ts, True)
    numba.set_num_threads(os.cpu_count())

    ref = B.compute_intersection_paper(shorelines, keys, ts, SET)
    correctness = {
        "coastsat": compare(ref, run_coastsat(shorelines, keys, ts), keys),
        "numpy_vec": compare(ref, run_numpy_vec(xy, gid, len(shorelines), keys, ts), keys),
        "numba_1t": compare(ref, run_numba(xy, starts, ends, keys, ts, False), keys),
        "numba_par": compare(ref, run_numba(xy, starts, ends, keys, ts, True), keys),
    }
    B.run_rust(None)
    correctness["rust"] = B.check(ref, keys)
    print("correctitud:", json.dumps(correctness), flush=True)

    impls = {
        "python_loop": lambda: B.compute_intersection_paper(shorelines, keys, ts, SET),
        "numpy_vec": lambda: run_numpy_vec(xy, gid, len(shorelines), keys, ts),
        "numba_1t": lambda: run_numba(xy, starts, ends, keys, ts, False),
        "numba_par": lambda: run_numba(xy, starts, ends, keys, ts, True),
    }
    names = ["coastsat", *impls, "rust_1t", "rust_par", "rust_cli_wall"]
    T = {k: [None] * args.rounds for k in names}
    w = args.warmup
    for r in range(args.rounds):
        # CoastSat tarda minutos por ronda: se mide solo en las primeras
        # rondas después del warm-up
        if w <= r < w + args.coastsat_rounds:
            t0 = time.perf_counter(); run_coastsat(shorelines, keys, ts); T["coastsat"][r] = time.perf_counter() - t0
        for k, f in impls.items():
            t0 = time.perf_counter(); f(); T[k][r] = time.perf_counter() - t0
        T["rust_1t"][r] = B.run_rust(1)[1]
        wall, c = B.run_rust(None); T["rust_par"][r] = c; T["rust_cli_wall"][r] = wall
        print(f"  ronda {r + 1}/{args.rounds}: " + " | ".join(
            f"{k} {T[k][r]:.4f}" for k in names if T[k][r] is not None),
            "(warm-up)" if r < w else "", flush=True)

    rounds = range(w, args.rounds)
    timings = {k: stats([T[k][r] for r in rounds if T[k][r] is not None])
               for k in names if any(T[k][r] is not None for r in rounds)}
    # razones ronda a ronda contra cada baseline de Python
    ratios = {}
    for base in ("coastsat", "python_loop", "numpy_vec", "numba_1t", "numba_par"):
        for rust in ("rust_1t", "rust_par"):
            q = [T[base][r] / T[rust][r] for r in rounds if T[base][r] is not None]
            if q:
                ratios[f"{base}/{rust}"] = stats(q)
    worst = max(v["rel_iqr"] for k, v in ratios.items() if not k.startswith("coastsat"))
    return {"correctness": correctness, "timings": timings, "speedups": ratios,
            "worst_rel_iqr_excluding_coastsat": worst, "publishable": worst <= args.max_rel_iqr, "raw": T}


def mode_scaling(args, ts, keys, base):
    res = {"threads": {}, "size": {}}
    # hilos, problema base
    write_synth(base)
    xy, gid = prepare_concat(base)
    lens = np.array([len(s) for s in base]); ends = np.cumsum(lens); starts = ends - lens
    run_numba(xy, starts, ends, keys, ts, True)
    for th in (1, 2, 4, 8, 12, 16):
        rt, nt = [], []
        numba.set_num_threads(th)
        for r in range(args.rounds):
            rt.append(B.run_rust(th)[1])
            t0 = time.perf_counter(); run_numba(xy, starts, ends, keys, ts, True); nt.append(time.perf_counter() - t0)
        res["threads"][th] = {"rust": stats(rt[args.warmup:]), "numba_par": stats(nt[args.warmup:])}
        print(f"  {th:2d} hilos: rust {np.median(rt[args.warmup:]):.4f} s, numba {np.median(nt[args.warmup:]):.4f} s", flush=True)
    numba.set_num_threads(os.cpu_count())
    # tamaño (número de shorelines)
    for n in (50, 100, 200, 400, 800, 1600):
        sl = scaled_shorelines(base, n)
        write_synth(sl)
        xy, gid = prepare_concat(sl)
        lens = np.array([len(s) for s in sl]); ends = np.cumsum(lens); starts = ends - lens
        row = {k: [] for k in ("python_loop", "numpy_vec", "numba_par", "rust_1t", "rust_par")}
        for r in range(args.rounds):
            if n <= 400:
                t0 = time.perf_counter(); B.compute_intersection_paper(sl, keys, ts, SET); row["python_loop"].append(time.perf_counter() - t0)
            t0 = time.perf_counter(); run_numpy_vec(xy, gid, n, keys, ts); row["numpy_vec"].append(time.perf_counter() - t0)
            t0 = time.perf_counter(); run_numba(xy, starts, ends, keys, ts, True); row["numba_par"].append(time.perf_counter() - t0)
            row["rust_1t"].append(B.run_rust(1)[1]); row["rust_par"].append(B.run_rust(None)[1])
        res["size"][n] = {k: stats(v[args.warmup:]) for k, v in row.items() if len(v) > args.warmup}
        print(f"  {n:5d} shorelines: " + " | ".join(f"{k} {res['size'][n][k]['median']:.4f}" for k in res["size"][n]), flush=True)
    write_synth(base)
    return res


MEM_SNIPPET = r"""
import sys, numpy as np
sys.path.insert(0, {d!r})
import benchmark_intersect as B, benchmark_fair as F
ts, keys, sl = B.build_workload()
xy, gid = F.prepare_concat(sl)
lens = np.array([len(s) for s in sl]); ends = np.cumsum(lens); starts = ends - lens
which = {which!r}
if which == 'baseline': pass
elif which == 'coastsat': F.run_coastsat(sl, keys, ts)
elif which == 'python_loop': B.compute_intersection_paper(sl, keys, ts, B.SETTINGS)
elif which == 'numpy_vec': F.run_numpy_vec(xy, gid, len(sl), keys, ts)
elif which == 'numba_par': F.run_numba(xy, starts, ends, keys, ts, True)
"""


def peak_rss_kb(cmd, env=None):
    r = subprocess.run(["/usr/bin/time", "-f", "%M", *cmd], capture_output=True, text=True, env=env, check=True)
    return int(r.stderr.strip().splitlines()[-1])


def mode_memory(args, ts, keys, base):
    write_synth(base)
    py = sys.executable
    res = {}
    for which in ("baseline", "coastsat", "python_loop", "numpy_vec", "numba_par"):
        res[which] = peak_rss_kb([py, "-c", MEM_SNIPPET.format(d=D, which=which)])
        print(f"  {which:12s} RSS pico {res[which] / 1024:.1f} MB", flush=True)
    cmd = [BIN, "intersect", "--transects", os.path.join(D, "transects_rust.csv"),
           "--shorelines", os.path.join(D, "shorelines_synth.csv"), "--along-dist", str(SET["along_dist"]),
           "-o", os.path.join(D, "cross_rust.csv")]
    for th in (1, 16):
        env = dict(os.environ, RAYON_NUM_THREADS=str(th))
        res[f"rust_cli_{th}t"] = peak_rss_kb(cmd, env)
        print(f"  rust CLI {th:2d} hilos RSS pico {res[f'rust_cli_{th}t'] / 1024:.1f} MB", flush=True)
    res["note"] = ("RSS pico del proceso completo; 'baseline' es el proceso de Python que solo arma "
                   "el workload (intérprete + numpy + numba importados), la diferencia es lo atribuible "
                   "a cada implementación. El de Rust incluye lectura y escritura de CSV.")
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", default="speed", choices=["speed", "scaling", "memory", "all"])
    ap.add_argument("--rounds", type=int, default=21)
    ap.add_argument("--warmup", type=int, default=3)
    ap.add_argument("--coastsat-rounds", type=int, default=6)
    ap.add_argument("--max-load", type=float, default=0.25 * os.cpu_count())
    ap.add_argument("--settle-s", type=int, default=600)
    ap.add_argument("--max-rel-iqr", type=float, default=0.20)
    ap.add_argument("--force", action="store_true")
    args = ap.parse_args()
    if not os.path.exists(BIN):
        raise SystemExit(f"falta el binario release: {BIN}")

    ts, keys, shorelines = B.build_workload()
    modes = ["speed", "scaling", "memory"] if args.mode == "all" else [args.mode]
    for m in modes:
        load0 = wait_quiet(args.max_load, args.settle_s, args.force)
        print(f"== {m} (carga inicial {load0:.1f}) ==", flush=True)
        t0 = time.time()
        if m == "speed":
            res = mode_speed(args, ts, keys, shorelines)
        elif m == "scaling":
            sargs = argparse.Namespace(**{**vars(args), "rounds": min(args.rounds, 7), "warmup": 2})
            res = mode_scaling(sargs, ts, keys, shorelines)
        else:
            res = mode_memory(args, ts, keys, shorelines)
        out = {"mode": m, "workload": {"transects": len(keys), "shorelines": len(shorelines), "settings": SET},
               "env": env_info(), "loadavg_before": load0, "loadavg_after": os.getloadavg()[0],
               "forced": args.force, "elapsed_s": time.time() - t0, "result": res}
        with open(os.path.join(D, f"benchmark_fair_{m}.json"), "w") as f:
            json.dump(out, f, indent=1, default=float)
        print(f"   -> benchmark_fair_{m}.json", flush=True)


if __name__ == "__main__":
    main()
