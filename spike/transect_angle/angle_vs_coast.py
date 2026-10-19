import json, pickle, csv, statistics as st, numpy as np
from pyproj import Transformer
from shapely.geometry import LineString, Point
from shapely.ops import unary_union
P4 = '/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera'
BASE = f'{P4}/output'
EPSG = json.load(open(f'{P4}/data_reprocessed/site_epsg.json'))
SITES = [('VINA_DEL_MAR','VINA_DEL_MAR_tidal_corrected.pkl','Viña del Mar'),('COQUIMBO','COQUIMBO_analysis.pkl','Coquimbo'),
         ('CONSTITUCION','CONSTITUCION_analysis.pkl','Constitución'),('VALDIVIA','VALDIVIA_analysis.pkl','Valdivia')]
def osm_coast(site):
    j = json.load(open(f'{P4}/data_reprocessed/{site}/coastline_osm.json'))
    tr = Transformer.from_crs('EPSG:4326', f'EPSG:{EPSG[site]}', always_xy=True); L=[]
    for e in j['elements']:
        g = e.get('geometry')
        if g and len(g) > 1: L.append(LineString(np.array(tr.transform([p['lon'] for p in g],[p['lat'] for p in g])).T))
    return unary_union(L)
def tnum(n):
    d=''.join(c for c in n if c.isdigit()); return int(d) if d else -1
th = {}
for r in csv.DictReader(open('spike/transect_angle/theta_persist.csv')):
    th[(r['site'], int(r['transect_num']))] = float(r['theta_pers'])
allon, alloff = [], []
for site, f, lab in SITES:
    an = pickle.load(open(f'{BASE}/{f}','rb')); coast = osm_coast(site)
    on, off = [], []
    for name, t in an['transects'].items():
        k=(lab,tnum(name))
        if k not in th: continue
        t=np.asarray(t,float); d=coast.distance(Point(*t[0]))
        (on if d<=200 else off).append(th[k])
    allon+=on; alloff+=off
    m=lambda v: f'{st.median(v):5.1f}°' if v else '  —  '
    print(f'{lab:13s} sobre costa n={len(on):3d} θ={m(on)} | fuera n={len(off):3d} θ={m(off)} | % sobre costa={100*len(on)/max(1,len(on)+len(off)):.0f}%')
m=lambda v: st.median(v)
print(f'TOTAL sobre costa n={len(allon)} θ={m(allon):.1f}° | fuera n={len(alloff)} θ={m(alloff):.1f}°')
import math
print('inflación implicada sobre costa: %.0f%%' % (100*(1/math.cos(math.radians(m(allon)))-1)))
