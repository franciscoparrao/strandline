"""Desalineación shore-normal de los transectos fijos — v4 (nula explícita).

Mide el ángulo θ entre cada transecto FIJO del estudio de referencia y la
normal local de la shoreline instantánea, sobre los datos publicados (sin
re-extraer nada, sin confound SR-vs-TOA).

Por qué importa: la distancia medida a lo largo de un transecto desalineado un
ángulo θ respecto de la normal es la perpendicular verdadera dividida por
cos(θ). Como 1/cos(θ) ≥ 1 SIEMPRE, cualquier desalineación infla — incluso una
simétrica y aleatoria produce sesgo positivo sistemático en el chainage y, con
transectos fijos, en la tasa de cambio.

## El problema metodológico y cómo se resuelve

Estimar la normal local de un contorno de marching squares es intrínsecamente
incierto: el contorno está escalonado a escala de píxel, así que el estimador
tiene un error propio de varios grados — comparable al efecto buscado. Además
θ = arcsin|u·t| es una cantidad RECTIFICADA: el ruido del estimador infla la
mediana medida aunque el θ verdadero sea 0.

Por eso no basta con "que el control dé 0". Lo correcto es construir una
**distribución nula**: aplicar el MISMO estimador a una línea cuyo θ verdadero
es 0 por construcción (la línea de referencia desde la que se generaron los
transectos), re-muestreada a la misma densidad de vértices que las shorelines
reales y cuantizada a la grilla de píxel para imitar el escalonado. La señal es
el exceso de la distribución medida sobre esa nula.

Se barre el tamaño de ventana del estimador: una ventana larga baja el ruido
pero difumina la curvatura real, así que un resultado creíble debe sostenerse
en varias ventanas.
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
MIN_PTS = 3
MAX_DATES = 300  # igualado al export de la Fig. 3 (una sola corrida para todo 5.5)
WINDOWS = [60.0, 100.0, 150.0, 200.0]
PIXEL = 15.0          # escalonado de marching squares a imitar en la nula
VERTEX_SPACING = 15.0  # densidad de vértices de las shorelines reales


# ---------------------------------------------------------------- estimador
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


def angles(transects, lines, half_win, with_ids=False):
    """θ (grados) entre cada transecto y la normal local, por cruce exacto.
    Con `with_ids`, devuelve (ids, θ) para poder separar la componente
    persistente por transecto de la que fluctúa entre fechas."""
    out = []
    ids = []
    prep = []
    for p in lines:
        if len(p) < 3:
            continue
        d = np.linalg.norm(np.diff(p, axis=0), axis=1)
        gap = max(100.0, 3.0 * float(np.median(d)))
        prep.append((p, piece_bounds(p, gap), p[:-1], np.diff(p, axis=0)))

    for tname, tr in transects.items():
        A = np.asarray(tr, dtype=float)
        o, e = A[0], A[1]
        d1 = e - o
        n1 = np.linalg.norm(d1)
        if n1 == 0:
            continue
        u = d1 / n1
        for p, b, q, d2 in prep:
            den = cross2(np.broadcast_to(d1, d2.shape), d2)
            ok = den != 0
            if not ok.any():
                continue
            w = o - q
            t = np.full(len(d2), np.nan)
            s = np.full(len(d2), np.nan)
            t[ok] = cross2(-w[ok], d2[ok]) / den[ok]
            s[ok] = cross2(-w[ok], np.broadcast_to(d1, d2.shape)[ok]) / den[ok]
            hit = ok & (t >= 0) & (t <= 1) & (s >= 0) & (s <= 1)
            if not hit.any():
                continue
            k = int(np.flatnonzero(hit)[0])
            tg = tangent_at(p, b, k, half_win)
            if tg is None:
                continue
            out.append(np.degrees(np.arcsin(min(abs(float(u @ tg)), 1.0))))
            ids.append(tname)
    if with_ids:
        return np.asarray(ids), np.asarray(out)
    return np.asarray(out)


# ------------------------------------------------------------------- nula
def resample(line, step):
    d = np.linalg.norm(np.diff(line, axis=0), axis=1)
    s = np.concatenate(([0.0], np.cumsum(d)))
    if s[-1] <= 0:
        return line.copy()
    tgt = np.arange(0.0, s[-1], step)
    x = np.interp(tgt, s, line[:, 0])
    y = np.interp(tgt, s, line[:, 1])
    return np.column_stack([x, y])


def quantize(line, pixel):
    """Imita el escalonado de marching squares: vértices sobre aristas de la
    grilla de píxel (una coordenada cae en un múltiplo exacto del píxel)."""
    q = line.copy()
    snap_x = np.arange(len(q)) % 2 == 0
    q[snap_x, 0] = np.round(q[snap_x, 0] / pixel) * pixel
    q[~snap_x, 1] = np.round(q[~snap_x, 1] / pixel) * pixel
    return q


def reference_line(an):
    if 'reference_shoreline' in an:
        return np.asarray(an['reference_shoreline'], dtype=float)
    return np.array([np.asarray(t, dtype=float).mean(axis=0)
                     for t in an['transects'].values()])


def summarize(th):
    infl = 1.0 / np.cos(np.radians(th))
    return (np.median(th), np.percentile(th, 75), np.percentile(th, 90),
            100 * (np.median(infl) - 1))


print('θ = ángulo entre el transecto fijo y la normal local de la costa.')
print('NULA = misma medición sobre la línea de referencia re-muestreada a '
      f'{VERTEX_SPACING:.0f} m y cuantizada a {PIXEL:.0f} m (θ verdadero = 0).')
print('SEÑAL = shorelines instantáneas de CoastSat.\n')

data = {}
for site, f in SITES:
    an = pickle.load(open(f'{BASE}/{f}', 'rb'))
    out = pickle.load(open(f'{KING}/{site}/{site}_output.pkl', 'rb'))
    lines = [np.asarray(s, dtype=float) for s in out['shorelines']
             if s is not None and len(s) > 10]
    if len(lines) > MAX_DATES:
        idx = np.linspace(0, len(lines) - 1, MAX_DATES).astype(int)
        lines = [lines[i] for i in idx]
    ref = reference_line(an)
    null_line = quantize(resample(ref, VERTEX_SPACING), PIXEL)
    data[site] = (an['transects'], lines, null_line)

for hw in WINDOWS:
    print(f'--- ventana ±{hw:.0f} m ' + '-' * 52)
    print(f"{'sitio':14s} {'nula med':>9s} {'nula p90':>9s} | {'señal med':>10s} "
          f"{'señal p90':>10s} | {'exceso':>7s} {'infl%':>7s}")
    ns, ss = [], []
    for site, _ in SITES:
        T, lines, null_line = data[site]
        tn = angles(T, [null_line], hw)
        ts = angles(T, lines, hw)
        if not len(tn) or not len(ts):
            print(f'{site:14s} sin datos'); continue
        ns.append(tn); ss.append(ts)
        n_med, _, n_p90, _ = summarize(tn)
        s_med, _, s_p90, s_infl = summarize(ts)
        print(f'{site:14s} {n_med:8.1f}° {n_p90:8.1f}° | {s_med:9.1f}° '
              f'{s_p90:9.1f}° | {s_med-n_med:6.1f}° {s_infl:6.1f}%')
    if ns and ss:
        tn = np.concatenate(ns); ts = np.concatenate(ss)
        n_med, _, n_p90, _ = summarize(tn)
        s_med, _, s_p90, s_infl = summarize(ts)
        try:
            from scipy import stats
            p = stats.mannwhitneyu(ts, tn, alternative='greater').pvalue
            ptxt = f'  (Mann-Whitney p={p:.2e})'
        except Exception:
            ptxt = ''
        print(f'{"GLOBAL":14s} {n_med:8.1f}° {n_p90:8.1f}° | {s_med:9.1f}° '
              f'{s_p90:9.1f}° | {s_med-n_med:6.1f}° {s_infl:6.1f}%'
              f'   n_nula={len(tn)} n_señal={len(ts)}{ptxt}')
    print()


# ------------------------------------------------------ persistente vs fluctuante
print()
print('=' * 78)
print('DESCOMPOSICIÓN: ¿la desalineación es PERSISTENTE por transecto o fluctúa')
print('entre fechas? Solo la persistente sesga la TASA (una inflación que cambia')
print('al azar entre fechas agrega varianza, pero su offset no altera la pendiente).')
print('=' * 78)
HW = 100.0
print(f"{'sitio':14s} {'n_tr':>5s} {'θ persist':>10s} {'θ p90':>7s} | "
      f"{'fluct (MAD)':>12s} | {'infl tasa':>10s}")
persist_all, fluct_all = [], []
for site, _ in SITES:
    T, lines, null_line = data[site]
    ids, th = angles(T, lines, HW, with_ids=True)
    if not len(th):
        continue
    per, mad = [], []
    for t in np.unique(ids):
        v = th[ids == t]
        if len(v) < 10:
            continue
        m = float(np.median(v))
        per.append(m)
        mad.append(float(np.median(np.abs(v - m))))
    if not per:
        continue
    per = np.asarray(per); mad = np.asarray(mad)
    persist_all.append(per); fluct_all.append(mad)
    infl = 100 * (np.median(1.0 / np.cos(np.radians(per))) - 1)
    print(f'{site:14s} {len(per):5d} {np.median(per):9.1f}° {np.percentile(per,90):6.1f}° | '
          f'{np.median(mad):11.1f}° | {infl:9.1f}%')
per = np.concatenate(persist_all); mad = np.concatenate(fluct_all)
infl = 1.0 / np.cos(np.radians(per))
print(f'{"GLOBAL":14s} {len(per):5d} {np.median(per):9.1f}° {np.percentile(per,90):6.1f}° | '
      f'{np.median(mad):11.1f}° | {100*(np.median(infl)-1):9.1f}%')
print()
print(f'Inflación de tasa implicada por la componente PERSISTENTE 1/cos(θ_persist):')
print(f'  mediana {100*(np.median(infl)-1):.1f}%   p75 {100*(np.percentile(infl,75)-1):.1f}%   '
      f'p90 {100*(np.percentile(infl,90)-1):.1f}%')
print(f'Fracción de transectos con θ_persist > 20°: {100*np.mean(per>20):.1f}%  '
      f'> 30°: {100*np.mean(per>30):.1f}%')
