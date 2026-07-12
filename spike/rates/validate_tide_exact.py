import pickle, csv, numpy as np
Pt='/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output/VINA_DEL_MAR_tidal_corrected.pkl'
with open(Pt,'rb') as f: dt=pickle.load(f)
rc=dt['rates_corrected']  # DataFrame del script 03 (con filtro dt_years>=20)
print("rates_corrected columns:", list(rc.columns)[:12])
print("n transectos rates_corrected:", len(rc))

rust={}
with open('spike/rates/rates_tidecorr.csv') as f:
    for row in csv.DictReader(f): rust[row['transect']]=row

# comparar LRR y WLR corregidos (transectos comunes)
cols=['LRR_m_yr','WLR_m_yr','NSM_m']
maxerr={c:0.0 for c in cols}; n=0; miss=0
for _,prow in rc.iterrows():
    tk=prow['transect']
    if tk not in rust: miss+=1; continue
    n+=1
    for c in cols:
        if c in prow: maxerr[c]=max(maxerr[c], abs(float(prow[c])-float(rust[tk][c])))
print(f"comparados {n}, no presentes en rust {miss}")
for c in cols: print(f"  {c:10s} error máx = {maxerr[c]:.2e}")
