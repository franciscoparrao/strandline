"""Exporta los datos crudos de la medición de θ para la Fig. 3 del paper.

Escribe dos CSV en este mismo directorio:
  theta_null.csv     — site, theta        (nula: línea de referencia, θ real = 0)
  theta_persist.csv  — site, transect_num, theta_pers, theta_h1, theta_h2, n_dates

El estimador es el mismo de `measure_transect_angle.py` / `verify_rate_impact.py`;
se replica aquí para que el export no dispare la medición completa al importar.
"""
import pickle
import numpy as np

import datasets  # fuente seleccionable con SDS_DATASET=original|ocean

OUT = datasets.OUT_DIR
SITES = [(s, None, datasets.LABELS[s]) for s in datasets.SITES]
HALF_WIN = 100.0
MIN_PTS = 3
MIN_DATES = 10
MAX_DATES = 300
PIXEL = 15.0
VERTEX_SPACING = 15.0


def piece_bounds(p, gap):
    d = np.linalg.norm(np.diff(p, axis=0), axis=1)
    return np.concatenate(([0], np.flatnonzero(d > gap) + 1, [len(p)]))


def tangent_at(p, b, k, half_win):
    j = np.searchsorted(b, k, side='right') - 1
    a, c = b[j], b[j + 1]
    if c - a < MIN_PTS:
        return None
    lo = hi = k
    acc = 0.0
    while lo - 1 >= a and acc < half_win:
        acc += np.linalg.norm(p[lo] - p[lo - 1]); lo -= 1
    acc = 0.0
    while hi + 1 < c and acc < half_win:
        acc += np.linalg.norm(p[hi + 1] - p[hi]); hi += 1
    sel = p[lo:hi + 1]
    if len(sel) < MIN_PTS:
        return None
    cc = sel - sel.mean(axis=0)
    w, v = np.linalg.eigh(cc.T @ cc / len(cc))
    if w[1] <= 0:
        return None
    return v[:, 1] / np.linalg.norm(v[:, 1])


def cross2(a, b):
    return a[..., 0] * b[..., 1] - a[..., 1] * b[..., 0]


def measure(transects, lines, half_win):
    """dict transecto -> [(idx_fecha, θ)]."""
    prep = []
    for di, p in enumerate(lines):
        if len(p) < 3:
            continue
        d = np.linalg.norm(np.diff(p, axis=0), axis=1)
        gap = max(100.0, 3.0 * float(np.median(d)))
        prep.append((di, p, piece_bounds(p, gap), p[:-1], np.diff(p, axis=0)))

    res = {}
    for tname, tr in transects.items():
        A = np.asarray(tr, dtype=float)
        o, e = A[0], A[1]
        d1 = e - o
        if np.linalg.norm(d1) == 0:
            continue
        u = d1 / np.linalg.norm(d1)
        acc = []
        for di, p, b, q, d2 in prep:
            den = cross2(np.broadcast_to(d1, d2.shape), d2)
            ok = den != 0
            if not ok.any():
                continue
            w = o - q
            t = np.full(len(d2), np.nan); s = np.full(len(d2), np.nan)
            t[ok] = cross2(-w[ok], d2[ok]) / den[ok]
            s[ok] = cross2(-w[ok], np.broadcast_to(d1, d2.shape)[ok]) / den[ok]
            hit = ok & (t >= 0) & (t <= 1) & (s >= 0) & (s <= 1)
            if not hit.any():
                continue
            k = int(np.flatnonzero(hit)[0])
            tg = tangent_at(p, b, k, half_win)
            if tg is None:
                continue
            acc.append((di, np.degrees(np.arcsin(min(abs(float(u @ tg)), 1.0)))))
        if acc:
            res[tname] = acc
    return res


def resample(line, step):
    d = np.linalg.norm(np.diff(line, axis=0), axis=1)
    s = np.concatenate(([0.0], np.cumsum(d)))
    if s[-1] <= 0:
        return line.copy()
    tgt = np.arange(0.0, s[-1], step)
    return np.column_stack([np.interp(tgt, s, line[:, 0]),
                            np.interp(tgt, s, line[:, 1])])


def quantize(line, pixel):
    q = line.copy()
    m = np.arange(len(q)) % 2 == 0
    q[m, 0] = np.round(q[m, 0] / pixel) * pixel
    q[~m, 1] = np.round(q[~m, 1] / pixel) * pixel
    return q


def tnum(name):
    d = ''.join(ch for ch in name if ch.isdigit())
    return int(d) if d else -1


rows_null, rows_pers = [], []
for site, f, label in SITES:
    an, out = datasets.load(site)
    lines = [np.asarray(s, dtype=float) for s in out['shorelines']
             if s is not None and len(s) > 10]
    if len(lines) > MAX_DATES:
        idx = np.linspace(0, len(lines) - 1, MAX_DATES).astype(int)
        lines = [lines[i] for i in idx]

    ref = (np.asarray(an['reference_shoreline'], dtype=float)
           if 'reference_shoreline' in an else
           np.array([np.asarray(t, dtype=float).mean(axis=0)
                     for t in an['transects'].values()]))
    for _, th in measure(an['transects'],
                         [quantize(resample(ref, VERTEX_SPACING), PIXEL)],
                         HALF_WIN).items():
        for _, v in th:
            rows_null.append((label, v))

    per = measure(an['transects'], lines, HALF_WIN)
    for tname, acc in per.items():
        if len(acc) < MIN_DATES:
            continue
        dis = np.array([d for d, _ in acc]); vs = np.array([v for _, v in acc])
        cut = np.median(dis)
        h1, h2 = vs[dis <= cut], vs[dis > cut]
        med = float(np.median(vs))
        rows_pers.append((
            label, tnum(tname), med,
            float(np.median(np.abs(vs - med))),      # MAD: componente fluctuante
            float(np.median(h1)) if len(h1) >= 5 else float('nan'),
            float(np.median(h2)) if len(h2) >= 5 else float('nan'),
            len(vs)))
    print(f'{label}: nula={sum(1 for r in rows_null if r[0]==label)}  '
          f'transectos={sum(1 for r in rows_pers if r[0]==label)}', flush=True)

with open(f'{OUT}/theta_null.csv', 'w') as fh:
    fh.write('site,theta\n')
    for s, v in rows_null:
        fh.write(f'{s},{v:.4f}\n')
with open(f'{OUT}/theta_persist.csv', 'w') as fh:
    fh.write('site,transect_num,theta_pers,theta_mad,theta_h1,theta_h2,n_dates\n')
    for r in rows_pers:
        fh.write(f'{r[0]},{r[1]},{r[2]:.4f},{r[3]:.4f},{r[4]:.4f},{r[5]:.4f},{r[6]}\n')
print(f'escritos {len(rows_null)} filas de nula y {len(rows_pers)} de persistente')
