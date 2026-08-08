"""Desalineación shore-normal de los transectos fijos — v3.

Fallos de las versiones previas, diagnosticados:
  v1: tangente por PCA de una bola de radio fijo. Las shorelines de CoastSat
      son la concatenación de contornos disjuntos → la bola mezclaba
      fragmentos, y con vértices espaciados 184 m casi no juntaba puntos.
  v2: tangente sobre vértices consecutivos, pero `gap=100 m` despedazaba la
      línea de referencia (espaciado 184 m) → el control no podía correr; y
      el cruce se exigía dentro del segmento [0,L], cuando estos transectos
      se usan con semántica de recta infinita acotada por max_origin_dist.

v3: cruce EXACTO segmento-segmento (sin corredor ni proyecciones), tangente
sobre vértices consecutivos con gap adaptativo al espaciado real de cada
línea. El control contra la línea de referencia debe dar ~0°.
"""
import pickle
import numpy as np

BASE = '/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output'
KING = '/mnt/kingston'
SITES = [
    ('VINA_DEL_MAR', 'VINA_DEL_MAR_tidal_corrected.pkl'),
    ('COQUIMBO', 'COQUIMBO_analysis.pkl'),
    ('CONSTITUCION', 'CONSTITUCION_analysis.pkl'),
    ('VALDIVIA', 'VALDIVIA_analysis.pkl'),
]
HALF_WIN = 60.0
MIN_PTS = 3
MAX_DATES = 120


def piece_bounds(p, gap):
    d = np.linalg.norm(np.diff(p, axis=0), axis=1)
    cuts = np.flatnonzero(d > gap) + 1
    b = np.concatenate(([0], cuts, [len(p)]))
    return b


def tangent_at(p, b, k, gap):
    """Tangente por PCA de vértices consecutivos del fragmento que contiene k."""
    j = np.searchsorted(b, k, side='right') - 1
    a, c = b[j], b[j + 1]
    if c - a < MIN_PTS:
        return None
    lo = hi = k
    acc = 0.0
    while lo - 1 >= a and acc < HALF_WIN:
        acc += np.linalg.norm(p[lo] - p[lo - 1]); lo -= 1
    acc = 0.0
    while hi + 1 < c and acc < HALF_WIN:
        acc += np.linalg.norm(p[hi + 1] - p[hi]); hi += 1
    sel = p[lo:hi + 1]
    if len(sel) < MIN_PTS:
        return None
    cc = sel - sel.mean(axis=0)
    w, v = np.linalg.eigh(cc.T @ cc / len(cc))
    if w[1] <= 0:
        return None
    return v[:, 1] / np.linalg.norm(v[:, 1])


def cross(a, b):
    return a[..., 0] * b[..., 1] - a[..., 1] * b[..., 0]


def angles(transects, lines):
    out, tried, crossed = [], 0, 0
    prepared = []
    for p in lines:
        if len(p) < 3:
            continue
        d = np.linalg.norm(np.diff(p, axis=0), axis=1)
        gap = max(100.0, 3.0 * float(np.median(d)))
        prepared.append((p, piece_bounds(p, gap), gap, p[:-1], np.diff(p, axis=0)))

    for tr in transects.values():
        A = np.asarray(tr, dtype=float)
        o, e = A[0], A[1]
        d1 = e - o
        if not np.any(d1):
            continue
        u = d1 / np.linalg.norm(d1)
        for p, b, gap, q, d2 in prepared:
            tried += 1
            den = cross(np.broadcast_to(d1, d2.shape), d2)
            ok = den != 0
            if not ok.any():
                continue
            w = o - q
            t = np.full(len(d2), np.nan)
            s = np.full(len(d2), np.nan)
            t[ok] = cross(-w[ok], d2[ok]) / den[ok]     # param a lo largo del transecto
            s[ok] = cross(-w[ok], np.broadcast_to(d1, d2.shape)[ok]) / den[ok]
            hit = ok & (t >= 0) & (t <= 1) & (s >= 0) & (s <= 1)
            if not hit.any():
                continue
            crossed += 1
            k = int(np.flatnonzero(hit)[0])            # primer cruce
            tg = tangent_at(p, b, k, gap)
            if tg is None:
                continue
            out.append(np.degrees(np.arcsin(min(abs(float(u @ tg)), 1.0))))
    return np.asarray(out), tried, crossed


print('CONTROL (transecto vs normal de su propia línea de referencia; esperado ~0°)')
ok_control = True
for site, f in SITES:
    an = pickle.load(open(f'{BASE}/{f}', 'rb'))
    if 'reference_shoreline' in an:
        ref = np.asarray(an['reference_shoreline'], dtype=float)
    else:
        ref = np.array([np.asarray(t, dtype=float).mean(axis=0)
                        for t in an['transects'].values()])
    th, tried, crossed = angles(an['transects'], [ref])
    if len(th):
        med = float(np.median(th))
        ok_control &= med < 5.0
        print(f'  {site:14s} n={len(th):4d}/{tried:4d} cruces={crossed:4d}  '
              f'mediana={med:5.2f}°  p90={np.percentile(th,90):5.2f}°')
    else:
        ok_control = False
        print(f'  {site:14s} sin datos')
print(f'  -> control {"OK" if ok_control else "FALLA"}')

if ok_control:
    print()
    print(f"{'sitio':14s} {'n':>6s} {'cruce%':>7s} {'θ med':>7s} {'θ p75':>7s} {'θ p90':>7s} "
          f"{'infl med':>9s} {'%>10°':>7s} {'%>20°':>7s}")
    allt = []
    for site, f in SITES:
        an = pickle.load(open(f'{BASE}/{f}', 'rb'))
        out = pickle.load(open(f'{KING}/{site}/{site}_output.pkl', 'rb'))
        lines = [np.asarray(s, dtype=float) for s in out['shorelines']
                 if s is not None and len(s) > 10]
        if len(lines) > MAX_DATES:
            idx = np.linspace(0, len(lines) - 1, MAX_DATES).astype(int)
            lines = [lines[i] for i in idx]
        th, tried, crossed = angles(an['transects'], lines)
        if not len(th):
            print(f'{site:14s} sin cruces'); continue
        allt.append(th)
        infl = 1.0 / np.cos(np.radians(th))
        print(f'{site:14s} {len(th):6d} {100*crossed/tried:6.1f}% {np.median(th):6.1f}° '
              f'{np.percentile(th,75):6.1f}° {np.percentile(th,90):6.1f}° '
              f'{np.median(infl):8.3f} {100*np.mean(th>10):6.1f}% {100*np.mean(th>20):6.1f}%')
    th = np.concatenate(allt)
    infl = 1.0 / np.cos(np.radians(th))
    print()
    print(f'GLOBAL n={len(th)}  θ mediana={np.median(th):.1f}°  p75={np.percentile(th,75):.1f}°  '
          f'p90={np.percentile(th,90):.1f}°')
    print(f'Inflación 1/cos(θ): mediana={np.median(infl):.4f} ({100*(np.median(infl)-1):.2f}%)  '
          f'p90={np.percentile(infl,90):.3f}')
    print(f'θ>10°: {100*np.mean(th>10):.1f}%   θ>20°: {100*np.mean(th>20):.1f}%   '
          f'θ>30°: {100*np.mean(th>30):.1f}%')
