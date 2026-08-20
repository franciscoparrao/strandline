"""Reconstruye la geometria (x, z) de los perfiles de terreno y recorta a la
banda de marea que el estimador satelital efectivamente muestrea.

El problema que resuelve: los perfiles de tesis reportan la inclinacion del
PRIMER segmento (la cara de berma), mientras que `strandline slope` estima una
pendiente EFECTIVA sobre el rango de marea barrido por la serie satelital.
Comparar uno con otro mide el desacuerdo entre dos definiciones, no la calidad
del estimador. Aqui se encadenan los segmentos y se calcula la pendiente sobre
la misma banda vertical.

Convencion de los perfiles (verificada contra las figuras de ambas tesis): el
mar esta a la izquierda, x=0 es el extremo mas seaward y z crece hacia tierra
hasta la cresta de berma.

LIMITACION CENTRAL, que condiciona todo lo demas: los perfiles no tienen datum
vertical. No se sabe a que nivel de marea corresponde x=0 — solo la fecha de la
campana, no la hora. Se asume que el levantamiento parte en la linea de agua del
momento y se reporta la sensibilidad a esa suposicion deslizando la banda hacia
arriba.

Uso: python3 spike/slope_field/clip_to_tidal_range.py
"""
import csv
import math
import os
import statistics as st

import numpy as np

D = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(D))
TIDES = os.path.join(ROOT, "spike", "slope_s2", "tides.csv")


def tidal_band():
    """Banda de marea barrida por la serie S2 de Vina (GOT).

    Se usa la misma para Algarrobo y El Quisco: ambos estan a menos de 60 km de
    Vina en la misma costa micro-mareal de Chile central, donde el rango es
    espacialmente coherente a esa escala.
    """
    t = np.array([float(r["tide_m"]) for r in csv.DictReader(open(TIDES))])
    return t.max() - t.min(), np.percentile(t, 95) - np.percentile(t, 5)


def load(path, season_key):
    rows = [r for r in csv.DictReader(l for l in open(path) if not l.startswith("#"))]
    prof = {}
    for r in rows:
        key = (r["profile"], r[season_key])
        prof.setdefault(key, []).append((float(r["length_m"]), float(r["manteo_deg"])))
    return prof


def chain(segs, slope_distance=True):
    """Encadena segmentos desde el extremo de mar. Devuelve (xs, zs).

    slope_distance=True trata el largo medido como distancia a lo largo del
    terreno (cinta sobre la superficie, que es la practica de terreno normal);
    False lo trata como proyeccion horizontal, que es como lo dibujan las
    figuras de Rodriguez. La diferencia es <8% bajo 22 grados pero llega a 30%
    en el segmento de 45 grados de PPEQ-3.
    """
    xs, zs = [0.0], [0.0]
    for L, deg in segs:
        a = math.radians(deg)
        dx = L * math.cos(a) if slope_distance else L
        dz = L * math.sin(a) if slope_distance else L * math.tan(a)
        xs.append(xs[-1] + dx)
        zs.append(zs[-1] + dz)
    return np.array(xs), np.array(zs)


def x_at_z(xs, zs, z_target):
    """x donde el perfil cruza z_target, en el primer tramo ascendente."""
    for i in range(len(zs) - 1):
        if zs[i + 1] < zs[i]:          # se alcanzo la cresta de berma
            break
        if zs[i] <= z_target <= zs[i + 1]:
            if zs[i + 1] == zs[i]:
                return xs[i]
            f = (z_target - zs[i]) / (zs[i + 1] - zs[i])
            return xs[i] + f * (xs[i + 1] - xs[i])
    return None


def crest_rise(zs):
    """Ascenso vertical disponible antes de la cresta."""
    top = zs[0]
    for i in range(len(zs) - 1):
        if zs[i + 1] < zs[i]:
            break
        top = zs[i + 1]
    return top - zs[0]


def effective_slope(xs, zs, z_lo, z_hi):
    a, b = x_at_z(xs, zs, z_lo), x_at_z(xs, zs, z_hi)
    if a is None or b is None or b <= a:
        return None
    return (z_hi - z_lo) / (b - a)


def main():
    full, p5p95 = tidal_band()
    print(f"banda de marea barrida por la serie S2 (GOT, Vina): "
          f"{full:.2f} m total, {p5p95:.2f} m entre p5 y p95\n")

    sets = [
        ("Algarrobo", os.path.join(D, "algarrobo_profiles_2023.csv"), "campaign"),
        ("El Quisco", os.path.join(D, "el_quisco_profiles_2023.csv"), "season"),
    ]
    for site, path, key in sets:
        prof = load(path, key)
        print(f"=== {site} " + "=" * 58)
        print(f"{'perfil':22s} {'campana':12s} {'tan S0':>7s} {'asc. a':>7s} "
              f"{'tan ef':>7s} {'tan ef':>7s}")
        print(f"{'':22s} {'':12s} {'(berma)':>7s} {'cresta':>7s} "
              f"{'banda':>7s} {'+0.5 m':>7s}")
        by_season = {}
        for (name, season), segs in sorted(prof.items()):
            xs, zs = chain(segs)
            rise = crest_rise(zs)
            s_band = effective_slope(xs, zs, 0.0, full)
            s_shift = effective_slope(xs, zs, 0.5, 0.5 + full)
            t0 = math.tan(math.radians(segs[0][1]))
            f = lambda v: f"{v:7.3f}" if v is not None else "      -"
            print(f"{name:22s} {season:12s} {t0:7.3f} {rise:7.2f} "
                  f"{f(s_band)} {f(s_shift)}")
            if s_band is not None:
                by_season.setdefault(season, []).append(s_band)
        print()
        for season, v in by_season.items():
            print(f"  mediana de la pendiente efectiva, {season:12s} "
                  f"{st.median(v):.3f}   (n={len(v)})")
        print()


if __name__ == "__main__":
    main()
