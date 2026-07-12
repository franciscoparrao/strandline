import pickle, csv, subprocess, numpy as np
P='/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output/VINA_DEL_MAR_transects_rates.pkl'
with open(P,'rb') as f: d=pickle.load(f)
cdc=d['cross_distance_clean']; dates=d['dates']; df=d['rates_df']

# 1. intersecciones en formato largo (transect, shoreline_id, chainage_m)
with open('spike/rates/intersections.csv','w') as f:
    w=csv.writer(f); w.writerow(['transect','shoreline_id','chainage_m'])
    for tkey in sorted(cdc.keys()):
        ch=cdc[tkey]
        for si,v in enumerate(ch):
            if np.isfinite(v):
                w.writerow([tkey, f'S{si:03d}', f'{v:.6f}'])

# 2. fechas + peso por shoreline. Peso del paper: L5/L7 -> 1/15, L8/L9 -> 1/10.
#    corte L5->L8 ~ 2013-04 (lanzamiento L8). Antes: 1/15, después: 1/10.
with open('spike/rates/dates.csv','w') as f:
    w=csv.writer(f); w.writerow(['shoreline_id','date','weight'])
    for si,dt in enumerate(dates):
        weight = 1.0/15.0 if dt.year < 2013 or (dt.year==2013 and dt.month<4) else 1.0/10.0
        w.writerow([f'S{si:03d}', dt.strftime('%Y-%m-%dT%H:%M:%S'), f'{weight:.6f}'])
print(f"exportado: {len(cdc)} transectos, {len(dates)} fechas")

# 3. Rust
subprocess.run(['./target/release/strandline','rates',
    '--intersections','spike/rates/intersections.csv',
    '--dates','spike/rates/dates.csv','--min-valid','10',
    '-o','spike/rates/rates_rust.csv'],check=True)

# 4. comparar con rates_df
rust={}
with open('spike/rates/rates_rust.csv') as f:
    for row in csv.DictReader(f): rust[row['transect']]=row
cols=['NSM_m','SCE_m','EPR_m_yr','LRR_m_yr','LRR_r2','LRR_ci95','WLR_m_yr','WLR_ci95']
print(f"\ntransectos: paper={len(df)}, rust={len(rust)}")
maxerr={c:0.0 for c in cols}
n=0
for _,prow in df.iterrows():
    tk=prow['transect']
    if tk not in rust: 
        print("FALTA en rust:", tk); continue
    n+=1
    for c in cols:
        pv=float(prow[c]); rv=float(rust[tk][c])
        maxerr[c]=max(maxerr[c], abs(pv-rv))
print(f"comparados: {n}")
for c in cols:
    print(f"  {c:12s} error máx = {maxerr[c]:.2e}")
