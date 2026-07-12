import csv, subprocess, numpy as np
# leer reference_shoreline
ref=[]
with open('spike/transects/reference_shoreline.csv') as f:
    for row in csv.DictReader(f): ref.append((float(row['easting']), float(row['northing'])))
# leer transectos de referencia
ref_t={}
with open('spike/transects/transects_ref.csv') as f:
    for row in csv.DictReader(f):
        ref_t[row['name']]=((float(row['origin_e']),float(row['origin_n'])),(float(row['end_e']),float(row['end_n'])))
print(f"reference: {len(ref)} pts, {len(ref_t)} transectos esperados")
