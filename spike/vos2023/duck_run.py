"""strandline en Duck (Vos et al. 2023, SDS_Benchmark): exactitud contra
levantamientos, con el protocolo del benchmark y lado a lado con CoastSat.

Pasos (configuración equivalente a la entrega de CoastSat al benchmark,
``algorithms/COASTSAT/settings_*.txt``):
1. ``extract`` sobre cada escena Landsat C2 L2 bajada con ``fetch``, con la
   referencia del benchmark y una banda de 100 m (``max_dist_ref``), largo
   mínimo 500 m; umbral por defecto del motor (sin modo compat: SR). Se
   descartan las escenas con más de 10 % de nubes en el área (``cloud_thresh``).
   Variante B: lo mismo con ``--subpixel-refine``.
2. ``intersect`` con los transectos del benchmark (``along_dist`` 25 m y el QC
   por defecto, que es el de CoastSat).
3. Despiking ``identify_outliers`` de CoastSat (40 m), como en
   ``compute_tidal_correction_COASTSAT.py``, y corrección mareal con las mareas
   FES2014 del benchmark, pendiente 0.1, al MSL: chainage + marea/0.1 (los
   transectos van de tierra a mar).
4. Evaluación con ``compare_timeseries2`` del benchmark (``utils.py``),
   reimplementada sin gráficos: min_days 3, max_days 10; estadísticos RMSE,
   media, desviación, P90 de |error| y R² por transecto y agrupados, sobre los
   transectos seleccionados del benchmark para Duck y sobre todos.
   La entrega de CoastSat (``team_CoastSat/DUCK/tidally_corrected_timeseries_MSL``)
   se evalúa con la misma función, sobre las mismas fechas de terreno.

Uso: python3 spike/vos2023/duck_run.py [--skip-extract]
Escribe spike/vos2023/duck_results.json y duck_results.txt
"""
import glob
import json
import os
import subprocess
import sys
import urllib.request
from datetime import datetime, timedelta, timezone

import numpy as np
import pandas as pd
import rasterio
from pyproj import Transformer
from rasterio.features import geometry_mask
from scipy import interpolate, stats

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.path.join(ROOT, 'target', 'release', 'strandline')
HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = '/mnt/kingston/VOS2023/SDS_Benchmark'
DS = f'{BENCH}/datasets/DUCK'
WORK = '/mnt/kingston/VOS2023/DUCK'
SCENES = f'{WORK}/scenes'
SELECTED = ['-91', '1', '1006', '1097']
SET = dict(min_days=3, max_days=10, max_dist_ref=100, min_length=500, along_dist=25,
           max_cross_change=40, cloud_thresh=0.10, beach_slope=0.1, ref_elev=0.0)
SENSOR = {'LT04': 'L4', 'LT05': 'L5', 'LE07': 'L7', 'LC08': 'L8', 'LC09': 'L9'}


# ------------------------------------------------------------ escenas
def scene_list():
    out = []
    for ms in sorted(glob.glob(f'{SCENES}/*_ms.tif')):
        stem = os.path.basename(ms)[:-len('_ms.tif')]
        date, item = stem[:10], stem[11:]
        out.append(dict(stem=stem, date=date, item=item, sensor=SENSOR.get(item[:4], item[:4]),
                        ms=ms, mask=f'{SCENES}/{stem}_mask.tif'))
    return out


def acquisition_times(items):
    """Hora de adquisición de cada ítem desde el STAC de Planetary Computer
    (``fetch`` guarda solo la fecha en el nombre)."""
    cache = f'{WORK}/acquisition_times.json'
    got = json.load(open(cache)) if os.path.exists(cache) else {}
    todo = [i for i in items if i not in got]
    for k in range(0, len(todo), 100):
        body = json.dumps({'collections': ['landsat-c2-l2'], 'ids': todo[k:k + 100], 'limit': 100}).encode()
        req = urllib.request.Request('https://planetarycomputer.microsoft.com/api/stac/v1/search', data=body,
                                     headers={'Content-Type': 'application/json'})
        for f in json.load(urllib.request.urlopen(req, timeout=120))['features']:
            got[f['id']] = f['properties']['datetime']
    json.dump(got, open(cache, 'w'))
    missing = [i for i in items if i not in got]
    if missing:
        raise SystemExit(f'{len(missing)} ítems sin hora en el STAC, p. ej. {missing[:3]}')
    return got


def cloud_fraction(mask_path, polygon_lonlat):
    """Fracción de píxeles con nube, sombra o nube dilatada (bits 1, 3 y 4 de
    QA_PIXEL) dentro del polígono del benchmark, sobre los píxeles con dato."""
    with rasterio.open(mask_path) as src:
        qa = src.read(1).astype(np.uint32)
        tr = Transformer.from_crs(4326, src.crs.to_epsg(), always_xy=True)
        ring = [tr.transform(p[0], p[1]) for p in polygon_lonlat]
        inside = ~geometry_mask([{'type': 'Polygon', 'coordinates': [ring]}], src.shape, src.transform)
    fill = (qa & 1) == 1
    cloud = ((qa >> 1) & 1) | ((qa >> 3) & 1) | ((qa >> 4) & 1)
    ok = inside & ~fill
    return float(cloud[ok].mean()) if ok.any() else 1.0


# ------------------------------------------------------------ CoastSat
def identify_outliers(chainage, dates, cross_change):
    """Copia de CoastSat ``SDS_transects.identify_outliers`` (GPL-3.0)."""
    chainage_temp = chainage.copy()
    dates_temp = dates.copy()
    k = 0
    while k < len(chainage_temp):
        for k in range(len(chainage_temp)):
            if k == 0:
                diff = chainage_temp[k] - chainage_temp[k + 1]
                if np.abs(diff) > cross_change:
                    chainage_temp.pop(k); dates_temp.pop(k); break
            elif k == len(chainage_temp) - 1:
                diff = chainage_temp[k] - chainage_temp[k - 1]
                if np.abs(diff) > cross_change:
                    chainage_temp.pop(k); dates_temp.pop(k); break
            else:
                diff_m1 = chainage_temp[k] - chainage_temp[k - 1]
                diff_p1 = chainage_temp[k] - chainage_temp[k + 1]
                condition1 = np.abs(diff_m1) > cross_change
                condition2 = np.abs(diff_p1) > cross_change
                condition3 = np.sign(diff_p1) == np.sign(diff_m1)
                if np.logical_and(np.logical_and(condition1, condition2), condition3):
                    chainage_temp.pop(k); dates_temp.pop(k); break
                if k >= 2 and k < len(chainage_temp) - 2:
                    diff_m2 = chainage_temp[k - 1] - chainage_temp[k - 2]
                    diff_p2 = chainage_temp[k + 1] - chainage_temp[k + 2]
                    condition4 = np.abs(diff_m2) > cross_change
                    condition5 = np.abs(diff_p2) > cross_change
                    condition6 = np.sign(diff_m1) == np.sign(diff_p2)
                    condition7 = np.sign(diff_p1) == np.sign(diff_m2)
                    if np.logical_and(np.logical_and(condition1, condition5), condition6):
                        chainage_temp.pop(k); dates_temp.pop(k); break
                    elif np.logical_and(np.logical_and(condition2, condition4), condition7):
                        chainage_temp.pop(k); dates_temp.pop(k); break
                    else:
                        condition4b = np.abs(diff_m2) > 1.5 * cross_change
                        condition5b = np.abs(diff_p2) > 1.5 * cross_change
                        condition8 = np.sign(diff_m2) == np.sign(diff_p2)
                        if np.logical_and(np.logical_and(np.logical_and(condition4b, condition5b),
                                                         np.logical_and(~condition1, ~condition2)),
                                          condition8):
                            chainage_temp.pop(k); dates_temp.pop(k); break
        k = k + 1
    return chainage_temp, dates_temp


def closest_tide(dates, tdates, tides):
    """Como ``get_closest_datapoint`` del benchmark: el primer dato de marea
    estrictamente posterior a cada fecha."""
    idx = np.searchsorted(tdates, np.array(dates, dtype='datetime64[ns]'), side='right')
    out = np.full(len(idx), np.nan)
    ok = idx < len(tides)  # después del fin de la serie de marea: sin corrección posible
    out[ok] = tides[idx[ok]]
    return out


# ------------------------------------------------------------ evaluación
def compare_timeseries2(dates_sat, chain_sat, dates_sur, chain_sur, s=SET):
    """``utils.compare_timeseries2`` del benchmark sin gráficos: para cada fecha
    de terreno, el satélite más cercano si está a < min_days, si no la
    interpolación entre el anterior y el siguiente si el más cercano está a
    <= max_days."""
    chain_sur = np.asarray(chain_sur, float)
    ok = ~np.isnan(chain_sur)
    dates_sur = [d for d, k in zip(dates_sur, ok) if k]
    chain_sur = chain_sur[ok]
    sat, sur, when = [], [], []
    for k, date in enumerate(dates_sur):
        days = np.array([(d - date).days for d in dates_sat])
        if len(days) == 0 or np.min(np.abs(days)) > s['max_days']:
            continue
        if np.min(np.abs(days)) < s['min_days']:
            i = np.where(np.abs(days) == np.min(np.abs(days)))[0][0]
            sat.append(chain_sat[i]); sur.append(chain_sur[k]); when.append(date)
        else:
            if sum(days < 0) == 0 or sum(days > 0) == 0:
                continue
            ia = np.where(days > 0)[0][0]; ib = ia - 1
            x = [dates_sat[ib].toordinal(), dates_sat[ia].toordinal()]
            f = interpolate.interp1d(x, [chain_sat[ib], chain_sat[ia]], bounds_error=True)
            try:
                sat.append(float(f(date.toordinal()))); sur.append(chain_sur[k]); when.append(date)
            except ValueError:
                continue
    sat, sur = np.array(sat), np.array(sur)
    m = ~np.isnan(sat)
    return sat[m], sur[m], [d for d, k in zip(when, m) if k]


def error_stats(sat, sur):
    e = sat - sur
    if len(e) < 2:
        return dict(n=len(e))
    r = stats.linregress(sur, sat).rvalue
    return dict(n=int(len(e)), rmse=float(np.sqrt(np.mean(e ** 2))), mean=float(np.mean(e)),
                std=float(np.std(e)), q90=float(np.percentile(np.abs(e), 90)), r2=float(r ** 2))


# ------------------------------------------------------------ pipeline
def run_strandline(scenes, ref, tag, extra):
    out_dir = f'{WORK}/shorelines_{tag}'
    os.makedirs(out_dir, exist_ok=True)
    for s in scenes:
        dst = f'{out_dir}/{s["stem"]}.geojson'
        if os.path.exists(dst):
            continue
        cmd = [BIN, 'extract', '--ms', s['ms'], '--mask', s['mask'], '--reference', ref,
               '--reference-epsg', '4326', '--max-dist-ref', str(SET['max_dist_ref']),
               '--min-length', str(SET['min_length']), *extra, '-o', dst]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            open(dst, 'w').write('{"type":"FeatureCollection","features":[]}')
    return out_dir


def shorelines_csv(scenes, sl_dir, path):
    epsg = None
    with open(path, 'w') as fh:
        fh.write('shoreline_id,easting,northing\n')
        n = 0
        for s in scenes:
            g = json.load(open(f'{sl_dir}/{s["stem"]}.geojson'))
            crs = g.get('crs', {}).get('properties', {}).get('name', '')
            if crs and epsg is None:
                epsg = int(crs.split(':')[-1])
            for f in g['features']:
                geom = f['geometry']
                lines = [geom['coordinates']] if geom['type'] == 'LineString' else geom['coordinates']
                for line in lines:
                    for x, y, *_ in line:
                        fh.write(f'{s["stem"]},{x!r},{y!r}\n')
                    n += 1
    return epsg


def transects_csv(epsg, path):
    tr = Transformer.from_crs(4326, epsg, always_xy=True)
    g = json.load(open(f'{DS}/DUCK_transects.geojson'))
    with open(path, 'w') as fh:
        fh.write('name,origin_e,origin_n,end_e,end_n\n')
        for f in g['features']:
            (x0, y0), (x1, y1) = f['geometry']['coordinates'][0], f['geometry']['coordinates'][-1]
            a, b = tr.transform(x0, y0); c, d = tr.transform(x1, y1)
            fh.write(f'{f["properties"]["name"]},{a!r},{b!r},{c!r},{d!r}\n')


def corrected_series(cross_csv, scenes, times, tdates, tides):
    when = {s['stem']: datetime.fromisoformat(times[s['item']].replace('Z', '+00:00')).replace(tzinfo=None)
            for s in scenes}
    C = pd.read_csv(cross_csv)
    out = {}
    for key, g in C.groupby('transect'):
        g = g.dropna(subset=['chainage_m']).copy()
        g['when'] = g.shoreline_id.map(when)
        g = g.sort_values('when')
        if len(g) < 2:
            continue
        ch, dt = identify_outliers(list(g.chainage_m), list(g.when), SET['max_cross_change'])
        tide = closest_tide(dt, tdates, tides)
        corr = np.array(ch) + (tide - SET['ref_elev']) / SET['beach_slope']
        keep = ~np.isnan(corr)
        out[str(key)] = ([d for d, k in zip(dt, keep) if k], corr[keep])
    return out


def coastsat_series():
    out = {}
    folder = f'{BENCH}/submissions/team_CoastSat/DUCK/tidally_corrected_timeseries_MSL'
    for fn in os.listdir(folder):
        key = fn.split('_')[0].split('.csv')[0]
        df = pd.read_csv(os.path.join(folder, fn), sep=',', parse_dates=['dates'])
        col = [c for c in df.columns if c not in ('dates', 'satname')][0]
        df = df.dropna(subset=[col])
        out[key] = ([d.to_pydatetime().replace(tzinfo=None) for d in df.dates], np.array(df[col], float))
    return out


def evaluate(series, gt, keys, period=None):
    pooled_sat, pooled_sur, per = [], [], {}
    for key in keys:
        if key not in series or key not in gt:
            continue
        ds, cs = series[key]
        dg, cg = gt[key]
        if period:
            sel = [(d, c) for d, c in zip(dg, cg) if period[0] <= d <= period[1]]
            if not sel:
                continue
            dg, cg = [d for d, _ in sel], [c for _, c in sel]
        sat, sur, _ = compare_timeseries2(list(ds), np.asarray(cs), dg, cg)
        per[key] = error_stats(sat, sur)
        pooled_sat += list(sat); pooled_sur += list(sur)
    return dict(pooled=error_stats(np.array(pooled_sat), np.array(pooled_sur)), per_transect=per)


def paired(series_list, gt, keys):
    """Evaluación pareada: solo los (transecto, fecha de terreno) que TODAS las
    series emparejan, para comparar errores sobre el mismo soporte."""
    matched = []
    for series in series_list:
        m = {}
        for key in keys:
            if key not in series or key not in gt:
                continue
            ds, cs = series[key]
            dg, cg = gt[key]
            sat, sur, when = compare_timeseries2(list(ds), np.asarray(cs), dg, cg)
            for a, b, w in zip(sat, sur, when):
                m[(key, w)] = (a, b)
        matched.append(m)
    common = set.intersection(*[set(m) for m in matched])
    out = []
    for m in matched:
        sat = np.array([m[k][0] for k in sorted(common)]); sur = np.array([m[k][1] for k in sorted(common)])
        out.append(error_stats(sat, sur))
    # diferencia pareada de error absoluto (primera serie menos la segunda), bootstrap por fecha
    if len(matched) == 2 and common:
        keys_c = sorted(common)
        ea = np.array([abs(matched[0][k][0] - matched[0][k][1]) for k in keys_c])
        eb = np.array([abs(matched[1][k][0] - matched[1][k][1]) for k in keys_c])
        dates_u = sorted({k[1] for k in keys_c})
        by_date = {d: [i for i, k in enumerate(keys_c) if k[1] == d] for d in dates_u}
        rng = np.random.default_rng(1)
        boots = []
        for _ in range(2000):
            pick = rng.choice(len(dates_u), len(dates_u))
            idx = [i for j in pick for i in by_date[dates_u[j]]]
            boots.append(np.median(ea[idx] - eb[idx]))
        return out, dict(n=len(keys_c), n_dates=len(dates_u), median_abs_err_diff=float(np.median(ea - eb)),
                         ci95=[float(np.percentile(boots, 2.5)), float(np.percentile(boots, 97.5))])
    return out, None


def main():
    scenes = scene_list()
    poly = json.load(open(f'{DS}/DUCK_polygon.geojson'))['features'][0]['geometry']['coordinates'][0]
    cf_cache = f'{WORK}/cloud_fraction.json'
    cf = json.load(open(cf_cache)) if os.path.exists(cf_cache) else {}
    for s in scenes:
        if s['stem'] not in cf:
            cf[s['stem']] = cloud_fraction(s['mask'], poly)
    json.dump(cf, open(cf_cache, 'w'))
    kept = [s for s in scenes if cf[s['stem']] <= SET['cloud_thresh'] and s['sensor'] in ('L5', 'L7', 'L8', 'L9')]
    print(f'{len(scenes)} escenas, {len(kept)} con nubes <= {SET["cloud_thresh"]:.0%} en el área', flush=True)
    times = acquisition_times([s['item'] for s in kept])

    T = pd.read_csv(f'{DS}/DUCK_tides.csv', parse_dates=['dates'])
    tdates = np.array([d.tz_localize(None) if d.tzinfo else d for d in T.dates], dtype='datetime64[ns]')
    tides = np.array(T.tides, float)

    G = pd.read_csv(f'{WORK}/DUCK_groundtruth_MSL.csv', dtype={'transect': str})
    gt = {k: ([datetime.strptime(d, '%Y-%m-%d') for d in g.date], np.array(g.chainage_m))
          for k, g in G.sort_values('date').groupby('transect')}

    cs = coastsat_series()
    all_cs = [d for v in cs.values() for d in v[0]]
    period = (min(all_cs), max(all_cs))

    res = {'settings': SET, 'n_scenes': len(scenes), 'n_scenes_kept': len(kept),
           'coastsat_period': [period[0].isoformat(), period[1].isoformat()], 'runs': {}}
    ref = f'{DS}/DUCK_reference_shoreline.geojson'
    runs = {'strandline': [], 'strandline_subpixel': ['--subpixel-refine']}
    for tag, extra in runs.items():
        if '--skip-extract' not in sys.argv:
            run_strandline(kept, ref, tag, extra)
        sl_dir = f'{WORK}/shorelines_{tag}'
        epsg = shorelines_csv(kept, sl_dir, f'{WORK}/{tag}_shorelines.csv')
        transects_csv(epsg, f'{WORK}/transects_{epsg}.csv')
        subprocess.run([BIN, 'intersect', '--transects', f'{WORK}/transects_{epsg}.csv',
                        '--shorelines', f'{WORK}/{tag}_shorelines.csv', '--along-dist', str(SET['along_dist']),
                        '-o', f'{WORK}/{tag}_cross.csv'], check=True, capture_output=True)
        series = corrected_series(f'{WORK}/{tag}_cross.csv', kept, times, tdates, tides)
        res.setdefault('_series', {})[tag] = series
        res['runs'][tag] = {
            'selected': evaluate(series, gt, SELECTED),
            'selected_coastsat_period': evaluate(series, gt, SELECTED, period),
            'all': evaluate(series, gt, sorted(gt)),
            'all_coastsat_period': evaluate(series, gt, sorted(gt), period)}
    res['runs']['coastsat_submission'] = {
        'selected': evaluate(cs, gt, SELECTED), 'all': evaluate(cs, gt, sorted(gt))}

    res['paired'] = {}
    for tag in runs:
        S = res['_series'][tag]
        for name, keys in (('selected', SELECTED), ('all', sorted(gt))):
            st, diff = paired([S, cs], gt, keys)
            res['paired'][f'{tag}_vs_coastsat_{name}'] = dict(strandline=st[0], coastsat=st[1],
                                                               abs_err_diff_strandline_minus_coastsat=diff)
    del res['_series']
    json.dump(res, open(f'{HERE}/duck_results.json', 'w'), indent=1, default=str)
    lines = [f'Duck, MSL, Landsat; período CoastSat {period[0]:%Y-%m-%d} a {period[1]:%Y-%m-%d}',
             f'{"serie":22s} {"conjunto":26s} {"n":>5s} {"RMSE":>6s} {"media":>6s} {"desv":>6s} {"P90":>6s} {"R2":>5s}']
    for tag, r in res['runs'].items():
        for k, v in r.items():
            p = v['pooled']
            if p.get('n', 0) >= 2:
                lines.append(f'{tag:22s} {k:26s} {p["n"]:5d} {p["rmse"]:6.1f} {p["mean"]:6.1f} '
                             f'{p["std"]:6.1f} {p["q90"]:6.1f} {p["r2"]:5.2f}')
    lines.append('')
    lines.append('Pareado (mismos transecto x fecha de terreno en ambas series):')
    for k, v in res['paired'].items():
        a, b, d = v['strandline'], v['coastsat'], v['abs_err_diff_strandline_minus_coastsat']
        lines.append(f'  {k:38s} n={a["n"]:5d}  RMSE {a["rmse"]:.1f} vs {b["rmse"]:.1f}  sesgo {a["mean"]:+.1f} vs {b["mean"]:+.1f}  '
                     f'P90 {a["q90"]:.1f} vs {b["q90"]:.1f}  dif. mediana |error| {d["median_abs_err_diff"]:+.2f} m '
                     f'[IC95 {d["ci95"][0]:+.2f}, {d["ci95"][1]:+.2f}] ({d["n_dates"]} fechas)')
    open(f'{HERE}/duck_results.txt', 'w').write('\n'.join(lines) + '\n')
    print('\n'.join(lines))


if __name__ == '__main__':
    main()
