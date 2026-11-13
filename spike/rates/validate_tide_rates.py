import pickle, csv, subprocess, numpy as np
Pt='/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera/output/VINA_DEL_MAR_tidal_corrected.pkl'
with open(Pt,'rb') as f: dt=pickle.load(f)
correction=np.asarray(dt['correction'])  # por shoreline (770)
# CSV de corrección mareal por shoreline
with open('spike/rates/tide_correction.csv','w') as f:
    w=csv.writer(f); w.writerow(['shoreline_id','correction_m'])
    for si,c in enumerate(correction): w.writerow([f'S{si:03d}', f'{c:.6f}'])

# tasas SIN corrección (ya calculadas)
def load_rates(path):
    r={}
    with open(path) as f:
        for row in csv.DictReader(f): r[row['transect']]=row
    return r
rust_uncorr=load_rates('spike/rates/rates_rust.csv')

# tasas CON corrección mareal
subprocess.run(['./target/release/strandline','rates',
    '--intersections','spike/rates/intersections.csv',
    '--dates','spike/rates/dates.csv','--min-valid','10',
    '--tide-correction','spike/rates/tide_correction.csv',
    # El pickle de referencia restó la corrección (scripts/03_tidal_correction.py
    # de propuesta-4). Sus transectos van de tierra a mar, así que ese signo era
    # el equivocado; 'landward' reproduce la referencia tal cual, no la física.
    '--chainage-direction','landward',
    '-o','spike/rates/rates_tidecorr.csv'],check=True)
rust_corr=load_rates('spike/rates/rates_tidecorr.csv')

# efecto: reducción de SCE media (métrica clave de calidad de corrección mareal)
common=set(rust_uncorr)&set(rust_corr)
sce_u=np.mean([float(rust_uncorr[k]['SCE_m']) for k in common])
sce_c=np.mean([float(rust_corr[k]['SCE_m']) for k in common])
print(f"SCE media sin corrección: {sce_u:.1f} m")
print(f"SCE media con corrección mareal: {sce_c:.1f} m")
print(f"reducción: {sce_u-sce_c:.1f} m ({100*(sce_u-sce_c)/sce_u:.1f}%)")
