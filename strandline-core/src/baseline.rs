//! Selección automática de línea base entre varias escenas candidatas
//! (S2/S6 escala: `fetch` trae series de decenas a cientos de escenas y
//! el usuario necesita UNA línea base limpia para `transects`).
//!
//! Dos heurísticas ingenuas fallaron con datos reales de una serie S2 de
//! Viña antes de llegar a esta:
//! - **"más vértices totales"**: elige el contorno más fragmentado/ruidoso
//!   (nubes residuales, contraste bajo), no el más limpio.
//! - **"menos fragmentos, punto"** (primera versión de este módulo):
//!   sobre-corrige y elige una escena de 224 m con 1 fragmento por sobre
//!   alternativas de 9-16 km con un puñado de piezas legítimas (ríos,
//!   espigones) — descarta cobertura real por evitar fragmentación menor.
//!
//! La métrica que sobrevivió a los datos reales de Viña (`spike/slope_s2`,
//! 215 escenas): longitud del fragmento más largo, penalizada por raíz
//! cuadrada del número de fragmentos — penaliza con fuerza creciente la
//! fragmentación patológica (cientos de trozos = escena mayormente nube)
//! sin castigar de más un puñado de piezas adicionales razonables.

use std::path::Path;

/// Métricas de calidad de la shoreline de una escena candidata a línea
/// base.
#[derive(Debug, Clone)]
pub struct BaselineCandidate {
    /// Identificador de la escena (nombre de archivo o etiqueta libre).
    pub label: String,
    /// Polilínea más larga (por longitud de arco) de entre las líneas
    /// extraídas de la escena — la representante para servir de baseline.
    pub line: Vec<(f64, f64)>,
    /// Número de polilíneas separadas que produjo la extracción: más
    /// fragmentos = costa más discontinua (nubes, ruido, cortes de
    /// escena) — señal de mala calidad para servir de línea base.
    pub n_fragments: usize,
    /// Longitud de arco (m) de `line`.
    pub arc_length_m: f64,
}

impl BaselineCandidate {
    /// Puntaje de calidad para comparar candidatas: longitud del
    /// fragmento representante, penalizada por raíz cuadrada del número
    /// de fragmentos. La raíz cuadrada crece lento al principio (1 pieza
    /// extra no descarta una escena por lo demás excelente) pero aplasta
    /// la fragmentación de cientos de trozos (escenas mayormente nube).
    pub fn quality_score(&self) -> f64 {
        self.arc_length_m / (self.n_fragments as f64).sqrt()
    }
}

fn arc_length(line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// Evalúa una escena (conjunto de polilíneas ya extraídas) como candidata
/// a línea base. `min_fragment_length_m` descarta trozos de ruido antes
/// de contar fragmentos — necesario porque `extract_shoreline` parte las
/// polilíneas por el buffer de nubes DESPUÉS del filtro de largo y, por
/// paridad con CoastSat, no re-filtra los trozos resultantes: sin este
/// segundo filtro, una escena casi toda nube con un puñado de trocitos
/// diminutos "gana" por tener pocos fragmentos.
/// Devuelve `None` si ninguna línea supera el mínimo.
pub fn evaluate_candidate(
    label: impl Into<String>,
    lines: &[Vec<(f64, f64)>],
    min_fragment_length_m: f64,
) -> Option<BaselineCandidate> {
    let kept: Vec<&Vec<(f64, f64)>> = lines
        .iter()
        .filter(|l| arc_length(l) >= min_fragment_length_m)
        .collect();
    let longest = kept.iter().max_by(|a, b| {
        arc_length(a)
            .partial_cmp(&arc_length(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    Some(BaselineCandidate {
        label: label.into(),
        line: (*longest).clone(),
        n_fragments: kept.len(),
        arc_length_m: arc_length(longest),
    })
}

/// Elige la mejor línea base entre candidatas por [`BaselineCandidate::quality_score`]
/// (mayor gana): longitud primero, fragmentación como penalización suave.
pub fn select_baseline(candidates: Vec<BaselineCandidate>) -> Option<BaselineCandidate> {
    candidates.into_iter().max_by(|a, b| {
        a.quality_score()
            .partial_cmp(&b.quality_score())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Escribe la línea base elegida como CSV `easting,northing` — el formato
/// que consume `transects --baseline`.
pub fn write_baseline_csv(line: &[(f64, f64)], path: &Path) -> std::io::Result<()> {
    let mut out = String::from("easting,northing\n");
    for (x, y) in line {
        out.push_str(&format!("{x:.3},{y:.3}\n"));
    }
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empate_de_longitud_prefiere_menos_fragmentos() {
        // A: una sola línea de 500 m.
        let a: Vec<Vec<(f64, f64)>> = vec![(0..=50).map(|i| (i as f64 * 10.0, 0.0)).collect()];
        // B: el mismo fragmento representante (500 m) más 3 piezas
        // menores (ríos, espigones) — mismo "longest", más fragmentos.
        let mut b: Vec<Vec<(f64, f64)>> =
            vec![(0..=50).map(|i| (i as f64 * 10.0, 100.0)).collect()];
        for k in 0..3 {
            b.push(
                (0..=20)
                    .map(|i| (k as f64 * 1000.0 + i as f64 * 10.0, 200.0))
                    .collect(),
            );
        }
        let cand_a = evaluate_candidate("A", &a, 0.0).unwrap();
        let cand_b = evaluate_candidate("B", &b, 0.0).unwrap();
        assert_eq!(cand_a.n_fragments, 1);
        assert_eq!(cand_b.n_fragments, 4);
        assert!((cand_a.arc_length_m - cand_b.arc_length_m).abs() < 1e-6);
        let chosen = select_baseline(vec![cand_a, cand_b]).unwrap();
        assert_eq!(
            chosen.label, "A",
            "a igual longitud del representante, menos fragmentos debe ganar"
        );
    }

    #[test]
    fn longitud_domina_sobre_fragmentacion_moderada() {
        // Caso real (Viña, spike/slope_s2): una escena de 224 m con 1
        // fragmento NO debe ganarle a una de 9785 m con 7 fragmentos —
        // la primera versión de este módulo (fragmentos como criterio
        // léxico primario) sí lo hacía, y era la escena inútil como
        // baseline para 4-6 km de costa real.
        let a: Vec<Vec<(f64, f64)>> = vec![(0..=10).map(|i| (i as f64 * 10.0, 0.0)).collect()]; // 100 m
        let mut b: Vec<Vec<(f64, f64)>> =
            vec![(0..=50).map(|i| (i as f64 * 10.0, 100.0)).collect()]; // 500 m
        for k in 0..2 {
            b.push(
                (0..=5)
                    .map(|i| (k as f64 * 1000.0 + i as f64 * 10.0, 200.0))
                    .collect(),
            );
        }
        let cand_a = evaluate_candidate("A", &a, 0.0).unwrap();
        let cand_b = evaluate_candidate("B", &b, 0.0).unwrap();
        let chosen = select_baseline(vec![cand_a, cand_b]).unwrap();
        assert_eq!(
            chosen.label, "B",
            "una escena con más cobertura real debe ganar pese a fragmentación modesta"
        );
    }

    #[test]
    fn fragmentacion_patologica_pierde_pese_a_mayor_longitud() {
        // A: 7 fragmentos, representante de 9785 (escala Viña real).
        let mut a: Vec<Vec<(f64, f64)>> = vec![(0..=978).map(|i| (i as f64 * 10.0, 0.0)).collect()]; // ~9780 m
        for k in 0..6 {
            a.push(
                (0..=20)
                    .map(|i| (k as f64 * 2000.0 + i as f64 * 10.0, 500.0))
                    .collect(),
            );
        }
        // B: 168 fragmentos (fragmentación patológica: escena mayormente
        // nube), representante más largo (16530 m).
        let mut b: Vec<Vec<(f64, f64)>> =
            vec![(0..=1653).map(|i| (i as f64 * 10.0, 1000.0)).collect()];
        for k in 0..167 {
            b.push(
                (0..=20)
                    .map(|i| (k as f64 * 200.0 + i as f64 * 10.0, 1500.0))
                    .collect(),
            );
        }
        let cand_a = evaluate_candidate("A", &a, 0.0).unwrap();
        let cand_b = evaluate_candidate("B", &b, 0.0).unwrap();
        assert!(cand_b.arc_length_m > cand_a.arc_length_m);
        let chosen = select_baseline(vec![cand_a, cand_b]).unwrap();
        assert_eq!(
            chosen.label, "A",
            "fragmentación de cientos de trozos debe perder pese a mayor longitud bruta"
        );
    }

    #[test]
    fn empate_en_fragmentos_desempata_por_longitud() {
        let a: Vec<Vec<(f64, f64)>> = vec![(0..=10).map(|i| (i as f64 * 10.0, 0.0)).collect()];
        let b: Vec<Vec<(f64, f64)>> = vec![(0..=30).map(|i| (i as f64 * 10.0, 100.0)).collect()];
        let cand_a = evaluate_candidate("A", &a, 0.0).unwrap();
        let cand_b = evaluate_candidate("B", &b, 0.0).unwrap();
        let chosen = select_baseline(vec![cand_a, cand_b]).unwrap();
        assert_eq!(chosen.label, "B");
    }

    #[test]
    fn evaluate_candidate_vacio_es_none() {
        assert!(evaluate_candidate("vacia", &[], 0.0).is_none());
    }

    #[test]
    fn descarta_trozos_de_ruido_bajo_el_minimo() {
        // escena real: buffer de nubes parte una línea larga y deja
        // trocitos de 2-3 vértices (~10-20 m) que extract_shoreline NO
        // re-filtra (paridad CoastSat) — sin el segundo filtro, una
        // escena "toda nube" con 3 trocitos gana a una limpia con 1 línea
        // larga por tener "menos fragmentos".
        let clean: Vec<Vec<(f64, f64)>> = vec![(0..=90).map(|i| (i as f64 * 10.0, 0.0)).collect()]; // 900 m, 1 fragmento
        let cloudy: Vec<Vec<(f64, f64)>> = vec![
            vec![(0.0, 100.0), (15.0, 100.0)],    // 15 m: ruido
            vec![(100.0, 100.0), (112.0, 100.0)], // 12 m: ruido
            vec![(200.0, 100.0), (208.0, 100.0)], // 8 m: ruido
        ];
        let cand_clean = evaluate_candidate("clean", &clean, 200.0).unwrap();
        assert_eq!(cand_clean.n_fragments, 1);
        // sin el filtro de largo, "cloudy" pasaría con n_fragments=3 y
        // ganaría por continuidad aparente; con el filtro no produce
        // ninguna candidata válida
        assert!(evaluate_candidate("cloudy", &cloudy, 200.0).is_none());

        let chosen = select_baseline(vec![cand_clean]).unwrap();
        assert_eq!(chosen.label, "clean");
    }
}
