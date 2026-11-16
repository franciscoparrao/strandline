"""Valida `strandline angle` contra el estimador del spike (export_for_figure.py).

Exporta los transectos, las shorelines por fecha y la línea de referencia de un
conjunto (SDS_DATASET=original|ocean) a CSV, corre el subcomando con los mismos
parámetros (ventana ±100 m, ≥10 fechas, 300 fechas, nula a 15 m y píxel 15 m) y
compara, transecto por transecto, con theta_persist.csv y theta_null.csv.
"""
import os, subprocess, sys
import numpy as np, pandas as pd
import datasets

BIN = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'target', 'release', 'strandline')
WORK = os.path.join(datasets.OUT_DIR, 'cli'); os.makedirs(WORK, exist_ok=True)


def tnum(name):
    d = ''.join(ch for ch in name if ch.isdigit())
    return int(d) if d else -1


def main():
    pers = pd.read_csv(f'{datasets.OUT_DIR}/theta_persist.csv')
    null = pd.read_csv(f'{datasets.OUT_DIR}/theta_null.csv')
    print(f'conjunto {datasets.DATASET}')
    worst = 0.0
    for site in datasets.SITES:
        label = datasets.LABELS[site]
        an, out = datasets.load(site)
        with open(f'{WORK}/{site}_tr.csv', 'w') as f:
            f.write('name,origin_e,origin_n,end_e,end_n\n')
            for k, t in an['transects'].items():
                t = np.asarray(t, float); f.write(f'{k},{float(t[0,0])!r},{float(t[0,1])!r},{float(t[1,0])!r},{float(t[1,1])!r}\n')
        with open(f'{WORK}/{site}_sl.csv', 'w') as f:
            f.write('shoreline_id,easting,northing\n')
            for i, s in enumerate(out['shorelines']):
                if s is None or len(s) == 0:
                    continue
                for x, y in np.asarray(s, float)[:, :2]:
                    f.write(f'{i},{float(x)!r},{float(y)!r}\n')
        ref = (np.asarray(an['reference_shoreline'], float) if 'reference_shoreline' in an else
               np.array([np.asarray(t, float).mean(axis=0) for t in an['transects'].values()]))
        with open(f'{WORK}/{site}_ref.csv', 'w') as f:
            f.write('easting,northing\n')
            for x, y in ref[:, :2]:
                f.write(f'{float(x)!r},{float(y)!r}\n')
        r = subprocess.run([BIN, 'angle', '--transects', f'{WORK}/{site}_tr.csv', '--shorelines', f'{WORK}/{site}_sl.csv',
                            '--reference', f'{WORK}/{site}_ref.csv', '--max-dates', '300',
                            '--null-output', f'{WORK}/{site}_null.csv', '-o', f'{WORK}/{site}_angle.csv'],
                           check=True, capture_output=True, text=True)
        R = pd.read_csv(f'{WORK}/{site}_angle.csv'); R['num'] = R.transect.map(tnum)
        S = pers[pers.site == label].set_index('transect_num')
        R = R.set_index('num')
        same = set(R.index) == set(S.index)
        d = {c: float((R[c + '_deg'] - S.loc[R.index, c]).abs().max()) for c in ('theta_pers', 'theta_mad')}
        dn = int((R.n_dates - S.loc[R.index, 'n_dates']).abs().max())
        N = pd.read_csv(f'{WORK}/{site}_null.csv')
        Sn = null[null.site == label].theta.values
        dnull = float(np.abs(np.sort(N.theta_deg.values) - np.sort(Sn)).max()) if len(N) == len(Sn) else float('nan')
        worst = max(worst, d['theta_pers'], d['theta_mad'])
        print(f'{label:14s} transectos {len(R):4d} (spike {len(S):4d}, mismos={same})  '
              f'max|Δθ_pers|={d["theta_pers"]:.1e}  max|ΔMAD|={d["theta_mad"]:.1e}  max|Δn|={dn}  '
              f'nula {len(N)} vs {len(Sn)} max|Δ|={dnull:.1e}')
        for line in r.stderr.strip().splitlines():
            print('    ' + line)
    print(f'peor diferencia en θ: {worst:.1e}°')


if __name__ == '__main__':
    sys.exit(main())
