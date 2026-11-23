"""Verdad de terreno de Duck (Vos et al. 2023, SDS_Benchmark), reproducida.

Replica las celdas 11-15 de ``1_preprocess_datasets.ipynb`` del benchmark
(commit clonado en /mnt/kingston/VOS2023/SDS_Benchmark): baja los perfiles del
FRF desde su servidor THREDDS y extrae, por perfil y fecha, la distancia
cross-shore (xFRF) del contorno MSL (-0.128 m NAVD88), interpolando la
elevación cada 1 m. Mismo listado de perfiles y misma exclusión (19890523).

Diferencias con el notebook, ambas sin efecto en la comparación:
- el servidor se mudó de chlthredds a chldata.erdc.dren.mil;
- solo se bajan levantamientos desde 1984 (antes no hay Landsat con qué
  emparejar a ±10 días).

Uso: python3 spike/vos2023/duck_groundtruth.py
Escribe /mnt/kingston/VOS2023/DUCK/DUCK_groundtruth_MSL.csv
(transect, date, chainage_m) y deja los .nc en /mnt/kingston/VOS2023/DUCK/raw.
"""
import os
import re
import sys
import time
import urllib.request
from datetime import datetime

import numpy as np
from netCDF4 import Dataset
from scipy import interpolate

BASE = 'https://chldata.erdc.dren.mil/thredds'
CATALOG = f'{BASE}/catalog/frf/geomorphology/elevationTransects/survey/data/catalog.html'
FILES = f'{BASE}/fileServer/frf/geomorphology/elevationTransects/survey/data/'
OUT = '/mnt/kingston/VOS2023/DUCK'
RAW = os.path.join(OUT, 'raw')
PROFILES = [-91, -46, 1, 46, 91, 137, 183, 229, 274, 320, 366, 411, 457, 558, 594, 640, 686, 731,
            777, 823, 869, 914, 960, 1006, 1052, 1097, 1157, 1217, 1277, 1337]
CONTOUR_MSL = -0.128  # NAVD88 -> MSL, contour_dict['DUCK']['MSL'] del notebook
FIRST = '19840101'


def download():
    os.makedirs(RAW, exist_ok=True)
    html = urllib.request.urlopen(CATALOG, timeout=120).read().decode()
    names = sorted(set(re.findall(r'FRF_geomorphology_elevationTransects_survey_\d{8}\.nc', html)))
    names = [n for n in names if n[-11:-3] >= FIRST]
    print(f'{len(names)} levantamientos desde {FIRST}', flush=True)
    for i, n in enumerate(names):
        dst = os.path.join(RAW, n)
        if os.path.exists(dst) and os.path.getsize(dst) > 0:
            continue
        for attempt in range(5):
            try:
                tmp = dst + '.part'
                urllib.request.urlretrieve(FILES + n, tmp)
                os.replace(tmp, dst)
                break
            except Exception as e:  # el servidor corta a veces: reintentar
                print(f'  reintento {attempt + 1} {n}: {e}', flush=True)
                time.sleep(5 * (attempt + 1))
        else:
            raise SystemExit(f'no se pudo bajar {n}')
        if (i + 1) % 50 == 0:
            print(f'  {i + 1}/{len(names)}', flush=True)


def process():
    rows = []
    survey = {}
    for fn in sorted(os.listdir(RAW)):
        if not fn.endswith('.nc'):
            continue
        date_str = fn[-11:].split('.nc')[0]
        if date_str == '19890523':  # levantamiento problemático (notebook)
            continue
        d = Dataset(os.path.join(RAW, fn))
        survey[fn] = dict(
            date=datetime.strptime(date_str, '%Y%m%d').strftime('%Y-%m-%d'),
            x=np.array(d.variables['xFRF'][:]), z=np.array(d.variables['elevation'][:]),
            profile=np.array(d.variables['profileNumber'][:]))
        d.close()
    for p in PROFILES:
        for s in survey.values():
            idx = np.where(s['profile'] == p)[0]
            if len(idx) < 3:
                continue
            ch, el = s['x'][idx], s['z'][idx]
            o = np.argsort(ch)
            ch, el = ch[o], el[o]
            ci = np.arange(np.min(ch) - 1, np.max(ch) + 1, 1)
            ei = np.interp(ci, ch, el)
            f = interpolate.interp1d(ei, ci, bounds_error=False)
            rows.append((str(p), s['date'], float(f(CONTOUR_MSL))))
    path = os.path.join(OUT, 'DUCK_groundtruth_MSL.csv')
    with open(path, 'w') as fh:
        fh.write('transect,date,chainage_m\n')
        for t, dte, c in rows:
            fh.write(f'{t},{dte},{c:.4f}\n')
    print(f'{len(rows)} posiciones de terreno ({len(survey)} levantamientos) -> {path}')


if __name__ == '__main__':
    if '--process-only' not in sys.argv:
        download()
    process()
