"""Valida `strandline sensor-offset` contra el test de sensores pareados de las
60 playas (propuesta-4, scripts/58_paper1_new_diagnostics.py, parte A).

Para cada playa y cada modo de umbral (compat y nivel 0) arma un dates.csv con
satname (de los nombres de escena), filtra las intersecciones a los transectos
retenidos en ambos modos (como el script 58), corre el subcomando y junta los
desfases por transecto. Compara el agregado con new_diagnostics.json y cada
transecto con la reimplementación en pandas de la función `offsets`.
"""
import glob, json, os, re, subprocess, sys, tempfile
import numpy as np, pandas as pd

P4 = os.path.expanduser('~/proyectos/zenodo-gaps/propuesta-4-erosion-costera')
BIN = os.path.join(os.path.dirname(__file__), '..', '..', 'target', 'release', 'strandline')
SEN = {'LT04': 'L5', 'LT05': 'L5', 'LE07': 'L7', 'LC08': 'L8', 'LC09': 'L9'}
PAIRS = [('L5', 'L7', 2003, 2011), ('L7', 'L8', 2013, 2021), ('L8', 'L9', 2022, 2025)]


def sensors(site):
    out = {}
    for f in glob.glob(f'{P4}/data_gradient/scenes/{site}/*_ms.tif'):
        m = re.match(r'(\d{4}-\d{2}-\d{2})_(L[CTE]0[4-9])_', os.path.basename(f))
        if m:
            out[m.group(1)] = SEN[m.group(2)]
    return out


def retained(f):
    R = pd.read_csv(f)
    return set(R[R['excluded'].astype(str).str.lower().isin(['false', '0'])].transect)


def py_offsets(W, sen, keep):
    D = pd.read_csv(f'{W}/dates.csv'); D['sensor'] = D.date.map(sen); D['year'] = D.date.str[:4].astype(int)
    I = pd.read_csv(f'{W}/intersections_tide.csv').merge(D[['shoreline_id', 'sensor', 'year']], on='shoreline_id')
    I = I[I.transect.isin(keep)]
    out = {}
    for a, b, y0, y1 in PAIRS:
        m = I[(I.year >= y0) & (I.year <= y1) & I.sensor.isin([a, b])].groupby(['transect', 'sensor']).chainage_m.median().unstack()
        out[f'{b}-{a}'] = (m[b] - m[a]).dropna() if (a in m and b in m) else pd.Series(dtype=float)
    return out


def rust_offsets(W, sen, keep, tmp):
    D = pd.read_csv(f'{W}/dates.csv'); D['satname'] = D.date.map(sen)
    D = D.dropna(subset=['satname'])
    D[['shoreline_id', 'date', 'satname']].to_csv(f'{tmp}/dates.csv', index=False)
    I = pd.read_csv(f'{W}/intersections_tide.csv'); I = I[I.transect.isin(keep)]
    I.to_csv(f'{tmp}/inter.csv', index=False)
    subprocess.run([BIN, 'sensor-offset', '--intersections', f'{tmp}/inter.csv', '--dates', f'{tmp}/dates.csv',
                    '--chainage-direction', 'seaward', '-o', f'{tmp}/off.csv'], check=True, capture_output=True)
    O = pd.read_csv(f'{tmp}/off.csv')
    return {k: g.set_index('transect').offset_m for k, g in O.groupby('pair')}


def main():
    ref = json.load(open(f'{P4}/manuscript_v2/figures/new_diagnostics.json'))['threshold_transfer']
    sites = [s for s in json.load(open(f'{P4}/data_gradient/gradient_sites.json')) if not s.startswith(('ALGARROBO_OTSU', 'ALGARROBO_L0'))]
    pooled = {k: {'compat': [], 'level0': []} for k in ['L7-L5', 'L8-L7', 'L9-L8']}
    maxdiff, n_sites = 0.0, 0
    with tempfile.TemporaryDirectory() as tmp:
        for s in sites:
            Wc, Wl = f'{P4}/output_gradient/_compat_backup/{s}', f'{P4}/output_gradient/{s}'
            if not (os.path.exists(f'{Wc}/intersections_tide.csv') and os.path.exists(f'{Wl}/intersections_tide.csv')):
                continue
            n_sites += 1
            sen = sensors(s)
            keep = retained(f'{Wc}/rates_tide.csv') & retained(f'{Wl}/rates_tide.csv')
            for mode, W in (('compat', Wc), ('level0', Wl)):
                r, p = rust_offsets(W, sen, keep, tmp), py_offsets(W, sen, keep)
                for k in pooled:
                    rk = r.get(k, pd.Series(dtype=float))
                    assert set(rk.index) == set(p[k].index), (s, mode, k)
                    if len(rk):
                        maxdiff = max(maxdiff, float((rk - p[k].reindex(rk.index)).abs().max()))
                    pooled[k][mode] += list(rk.values)
    print(f'{n_sites} playas; máxima diferencia por transecto contra pandas: {maxdiff:.2e} m')
    print(f"{'par':6s} {'modo':7s} {'n':>5s} {'mediana':>9s} {'q25':>8s} {'q75':>8s}   referencia (new_diagnostics.json)")
    for k, d in pooled.items():
        for m, v in d.items():
            r = ref[k][m]
            print(f'{k:6s} {m:7s} {len(v):5d} {np.median(v):9.3f} {np.percentile(v,25):8.3f} {np.percentile(v,75):8.3f}'
                  f'   n={r["n"]} {r["median"]:.3f} {r["q25"]:.3f} {r["q75"]:.3f}')


if __name__ == '__main__':
    sys.exit(main())
