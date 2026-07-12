"""Valida la intersección Rust contra el algoritmo vectorizado del paper,
sobre shorelines sintéticas realistas, y compara tiempos."""
import csv, time, subprocess, numpy as np

# 1. cargar transectos generados por Rust
ts = {}
with open('spike/transects/transects_rust.csv') as f:
    for row in csv.DictReader(f):
        ts[row['name']] = np.array([[float(row['origin_e']),float(row['origin_n'])],
                                    [float(row['end_e']),float(row['end_n'])]])
keys = sorted(ts.keys())

# 2. línea base para construir shorelines sintéticas
base = []
with open('spike/transects/reference_shoreline.csv') as f:
    for row in csv.DictReader(f):
        base.append((float(row['easting']), float(row['northing'])))
base = np.array(base)

# densificar la línea base y desplazarla cross-shore para simular N shorelines
from numpy import interp
dx=np.diff(base[:,0]); dy=np.diff(base[:,1])
seg=np.sqrt(dx**2+dy**2); cum=np.concatenate([[0],np.cumsum(seg)]); total=cum[-1]
dense_d=np.arange(0,total,10.0)  # cada 10 m
dense_e=interp(dense_d,cum,base[:,0]); dense_n=interp(dense_d,cum,base[:,1])
# normal aprox hacia el oeste (seaward): perpendicular a tangente
te=np.gradient(dense_e); tn=np.gradient(dense_n)
tl=np.sqrt(te**2+tn**2); tex,tny=te/tl,tn/tl
# normal (ty,-tx) orientada al oeste
nx=tny; ny=-tex
flip = nx>0
nx[flip]*=-1; ny[flip]*=-1

np.random.seed(7)
N_SHORE=200
shorelines=[]
for i in range(N_SHORE):
    # desplazamiento cross-shore variable en el tiempo + ruido espacial
    off = 30*np.sin(i/12) + np.random.randn()*3
    noise = np.random.randn(len(dense_e))*1.5
    disp = off + noise
    se = dense_e + disp*nx
    sn = dense_n + disp*ny
    shorelines.append(np.column_stack([se,sn]))

# 3. escribir shorelines a CSV para Rust
with open('spike/transects/shorelines_synth.csv','w') as f:
    w=csv.writer(f); w.writerow(['shoreline_id','easting','northing'])
    for i,sl in enumerate(shorelines):
        for e,n in sl: w.writerow([f'S{i:03d}', f'{e:.4f}', f'{n:.4f}'])

# 4. algoritmo del paper (vectorizado) — copiado de 02_transects_rates.py
def compute_intersection_paper(shorelines, transects_keys, ts, settings):
    along=settings['along_dist']; cross={}
    n=len(shorelines)
    for key in transects_keys:
        std_a=np.zeros(n); med_a=np.zeros(n); max_a=np.zeros(n); min_a=np.zeros(n); n_a=np.zeros(n)
        p1=ts[key][0]; p2=ts[key][-1]; temp=p2-p1
        seg_len=np.linalg.norm(temp); phi=np.arctan2(temp[1],temp[0])
        cph,sph=np.cos(phi),np.sin(phi)
        for i in range(n):
            sl=shorelines[i]; dp=sl-p1
            crs=dp[:,0]*temp[1]-dp[:,1]*temp[0]; d_line=np.abs(crs)/seg_len
            d_o=np.linalg.norm(dp,axis=1)
            idx=np.where((d_line<=along)&(d_o<=1000))[0]
            if len(idx)==0:
                std_a[i]=med_a[i]=max_a[i]=min_a[i]=n_a[i]=np.nan
            else:
                xyc=sl[idx]-p1; xr=cph*xyc[:,0]+sph*xyc[:,1]
                xr[xr<settings['min_chainage']]=np.nan
                std_a[i]=np.nanstd(xr); med_a[i]=np.nanmedian(xr)
                max_a[i]=np.nanmax(xr); min_a[i]=np.nanmin(xr); n_a[i]=np.sum(~np.isnan(xr))
        c1=std_a<=settings['max_std']; c2=(max_a-min_a)<=settings['max_range']; c3=n_a>=settings['min_points']
        good=c1&c2&c3
        prc=np.nansum(std_a>settings['max_std'])/n
        if prc>settings['auto_prc']:
            med_a[~good]=max_a[~good]; med_a[~c3]=np.nan
        else:
            med_a[~good]=np.nan
        cross[key]=med_a
    return cross

settings={'along_dist':25,'min_points':3,'max_std':15,'max_range':30,'min_chainage':-100,'multiple_inter':'auto','auto_prc':0.1}
t0=time.perf_counter()
cross_paper=compute_intersection_paper(shorelines, keys, ts, settings)
t_paper=time.perf_counter()-t0
print(f"paper (Python vectorizado): {t_paper:.3f} s  [{len(keys)} transectos x {N_SHORE} shorelines]")

# 5. Rust
t0=time.perf_counter()
subprocess.run(['./target/release/strandline','intersect',
    '--transects','spike/transects/transects_rust.csv',
    '--shorelines','spike/transects/shorelines_synth.csv',
    '--along-dist','25','-o','spike/transects/cross_rust.csv'],check=True)
t_rust_wall=time.perf_counter()-t0

# 6. cargar Rust y comparar
rust={}
with open('spike/transects/cross_rust.csv') as f:
    for row in csv.DictReader(f):
        rust.setdefault(row['transect'],{})[row['shoreline_id']]=float(row['chainage_m'])

diffs=[]
for ti,key in enumerate(keys):
    med=cross_paper[key]
    for si in range(N_SHORE):
        sid=f'S{si:03d}'
        pv=med[si]
        rv=rust.get(key,{}).get(sid,np.nan)
        if np.isnan(pv) and np.isnan(rv): continue
        if np.isnan(pv)!=np.isnan(rv):
            diffs.append(('mismatch-nan',key,sid,pv,rv)); continue
        diffs.append(abs(pv-rv))
numeric=[d for d in diffs if isinstance(d,float)]
nanmis=[d for d in diffs if not isinstance(d,float)]
print(f"comparación: {len(numeric)} cruces numéricos, {len(nanmis)} mismatch NaN")
print(f"error max chainage: {max(numeric):.6f} m, mediana: {np.median(numeric):.6f} m")
if nanmis[:3]: print("ejemplos mismatch NaN:", nanmis[:3])
