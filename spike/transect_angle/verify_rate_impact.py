"""¿El θ persistente es geometría real o artefacto del estimador?

Aplicar la corrección cos(θ) a las tasas es aritméticamente forzado: si θ es
persistente, la tasa baja por ese factor por construcción. Eso NO es evidencia
de nada. La pregunta que sí se puede responder es si el ángulo medido es
geometría real de la costa o ruido del estimador de tangente.

Tres tests, en orden de fuerza:

1. COHERENCIA ESPACIAL (el discriminador). Los transectos están ordenados a lo
   largo de la costa con ~100 m de separación. Si θ_persist refleja la
   orientación real de la costa, transectos vecinos deben compartirlo (la
   orientación varía suave); si es ruido del estimador, θ_persist sería
   espacialmente independiente. Se compara la autocorrelación a lag 1..k contra
   una nula de permutación.

2. ESTABILIDAD ENTRE MITADES TEMPORALES. Se parte la serie de fechas en dos
   mitades independientes y se calcula θ_persist en cada una. Ruido → sin
   correlación; geometría → correlación alta.

3. CONSECUENCIA ARITMÉTICA sobre las tasas, reportada como tal (no como
   evidencia independiente): factor cos(θ_persist) sobre el LRR publicado.
"""
import pickle
import numpy as np

import importlib.util
spec = importlib.util.spec_from_file_location(
    "mta", __file__.replace("verify_rate_impact.py", "measure_transect_angle.py"))

BASE = '/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output'
KING = '/mnt/kingston'
SITES = [
    ('VINA_DEL_MAR', 'VINA_DEL_MAR_tidal_corrected.pkl'),
    ('COQUIMBO', 'COQUIMBO_analysis.pkl'),
    ('CONSTITUCION', 'CONSTITUCION_analysis.pkl'),
    ('VALDIVIA', 'VALDIVIA_analysis.pkl'),
]
HALF_WIN = 100.0
MIN_PTS = 3
MIN_DATES = 10
MAX_DATES = 160


# ---- estimador (idéntico al de measure_transect_angle.py) ----
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


def theta_by_transect_and_date(transects, lines, half_win):
    """dict transecto -> lista de (indice_de_fecha, θ)."""
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
        n1 = np.linalg.norm(d1)
        if n1 == 0:
            continue
        u = d1 / n1
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
        if len(acc) >= MIN_DATES:
            res[tname] = acc
    return res


def tnum(name):
    digits = ''.join(ch for ch in name if ch.isdigit())
    return int(digits) if digits else -1


rng = np.random.default_rng(0)
print('TEST 1 — COHERENCIA ESPACIAL de θ_persist entre transectos vecinos')
print('(si es geometría real, vecinos correlacionan; si es ruido, no)\n')
print(f"{'sitio':14s} {'n':>4s} {'r lag1':>8s} {'r lag2':>8s} {'r lag5':>8s} "
      f"{'r lag20':>8s} | {'nula permutada':>16s}")

store = {}
for site, f in SITES:
    an = pickle.load(open(f'{BASE}/{f}', 'rb'))
    out = pickle.load(open(f'{KING}/{site}/{site}_output.pkl', 'rb'))
    lines = [np.asarray(s, dtype=float) for s in out['shorelines']
             if s is not None and len(s) > 10]
    if len(lines) > MAX_DATES:
        idx = np.linspace(0, len(lines) - 1, MAX_DATES).astype(int)
        lines = [lines[i] for i in idx]
    per_t = theta_by_transect_and_date(an['transects'], lines, HALF_WIN)
    if len(per_t) < 30:
        print(f'{site:14s} solo {len(per_t)} transectos con ≥{MIN_DATES} fechas — omitido')
        store[site] = (an, per_t)
        continue
    names = sorted(per_t, key=tnum)
    idxs = np.array([tnum(n) for n in names])
    th = np.array([np.median([v for _, v in per_t[n]]) for n in names])

    def lagcorr(vals, positions, lag):
        pos = {p: i for i, p in enumerate(positions)}
        a, b = [], []
        for p, v in zip(positions, vals):
            j = pos.get(p + lag)
            if j is not None:
                a.append(v); b.append(vals[j])
        if len(a) < 10:
            return np.nan
        return float(np.corrcoef(a, b)[0, 1])

    r = [lagcorr(th, idxs, L) for L in (1, 2, 5, 20)]
    null = []
    for _ in range(200):
        sh = rng.permutation(th)
        v = lagcorr(sh, idxs, 1)
        if not np.isnan(v):
            null.append(v)
    nlo, nhi = np.percentile(null, [2.5, 97.5]) if null else (np.nan, np.nan)
    print(f'{site:14s} {len(th):4d} {r[0]:8.3f} {r[1]:8.3f} {r[2]:8.3f} {r[3]:8.3f} | '
          f'[{nlo:+.3f}, {nhi:+.3f}]')
    store[site] = (an, per_t)

print()
print('TEST 2 — ESTABILIDAD entre dos mitades temporales independientes')
print(f"{'sitio':14s} {'n':>4s} {'r(mitad1, mitad2)':>18s}")
for site, _ in SITES:
    an, per_t = store.get(site, (None, {}))
    if len(per_t) < 30:
        continue
    a, b = [], []
    for n, acc in per_t.items():
        dis = np.array([d for d, _ in acc]); vs = np.array([v for _, v in acc])
        cut = np.median(dis)
        h1, h2 = vs[dis <= cut], vs[dis > cut]
        if len(h1) >= 5 and len(h2) >= 5:
            a.append(np.median(h1)); b.append(np.median(h2))
    if len(a) >= 10:
        print(f'{site:14s} {len(a):4d} {float(np.corrcoef(a, b)[0,1]):18.3f}')

print()
print('TEST 3 — consecuencia ARITMÉTICA sobre el LRR publicado (no es evidencia')
print('independiente: si θ es persistente, la tasa baja por cos(θ) por construcción)')
print(f"{'sitio':14s} {'n':>4s} {'|LRR| pub':>10s} {'|LRR| proy':>11s} {'reducción':>10s}")
for site, f in SITES:
    an, per_t = store.get(site, (None, {}))
    if an is None or not per_t:
        continue
    rates = an.get('rates_corrected', an.get('rates_all'))
    if rates is None:
        continue
    col = 'LRR_m_yr'
    m = dict(zip(rates['transect'], rates[col]))
    raw, proj = [], []
    for n, acc in per_t.items():
        if n not in m or not np.isfinite(m[n]):
            continue
        th = np.median([v for _, v in acc])
        raw.append(abs(m[n]))
        proj.append(abs(m[n]) * np.cos(np.radians(th)))
    if len(raw) >= 10:
        raw = np.array(raw); proj = np.array(proj)
        print(f'{site:14s} {len(raw):4d} {np.median(raw):9.3f}  {np.median(proj):10.3f}  '
              f'{100*(1-np.median(proj)/np.median(raw)):9.1f}%')
