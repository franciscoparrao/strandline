import pickle, csv, subprocess, numpy as np
P='/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output/VINA_DEL_MAR_transects_rates.pkl'
with open(P,'rb') as f: d=pickle.load(f)
cd_raw=d['cross_distance']; cdc=d['cross_distance_clean']; dates=d['dates']; df=d['rates_df']

# intersecciones desde el cross_distance RAW (sin limpiar) — solo los transectos
# que el paper conservó (>=30 válidos), para comparar el pipeline completo
keep = set(cdc.keys())
with open('spike/rates/intersections_raw.csv','w') as f:
    w=csv.writer(f); w.writerow(['transect','shoreline_id','chainage_m'])
    for tkey in sorted(cd_raw.keys()):
        if tkey not in keep: continue
        for si,v in enumerate(cd_raw[tkey]):
            if np.isfinite(v): w.writerow([tkey, f'S{si:03d}', f'{v:.6f}'])

# Rust: aplicar despike (40m) y calcular tasas
subprocess.run(['./target/release/strandline','rates',
    '--intersections','spike/rates/intersections_raw.csv',
    '--dates','spike/rates/dates.csv','--min-valid','10',
    '--despike-max-change','40','--despike-min-valid','30',
    '-o','spike/rates/rates_despiked.csv'],check=True)

rust={}
with open('spike/rates/rates_despiked.csv') as f:
    for row in csv.DictReader(f): rust[row['transect']]=row
cols=['NSM_m','SCE_m','EPR_m_yr','LRR_m_yr','WLR_m_yr']
maxerr={c:0.0 for c in cols}; n=0; missing=0
for _,prow in df.iterrows():
    tk=prow['transect']
    if tk not in rust: missing+=1; continue
    n+=1
    for c in cols: maxerr[c]=max(maxerr[c], abs(float(prow[c])-float(rust[tk][c])))
print(f"pipeline raw->despike->rates: comparados {n}/{len(df)}, faltantes {missing}")
for c in cols: print(f"  {c:10s} error máx = {maxerr[c]:.2e}")
