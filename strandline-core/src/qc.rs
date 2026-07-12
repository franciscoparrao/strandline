//! Control de calidad de tasas por transecto y clasificación del cambio.
//!
//! Aplica los cinco criterios geomorfológicos calibrados en el paper de
//! erosión costera (siguiendo Toimil 2020, Castelle 2022) para descartar
//! transectos que cruzan promontorios rocosos, estructuras portuarias o
//! revestimientos, cuyos "tasas" no son señal de playa. Los criterios se
//! evalúan **en orden**; la razón de exclusión es el primero que falla.

use crate::rates::Rates;

/// Umbrales de los cinco criterios de calidad. Los defaults son los
/// calibrados en el paper (Viña del Mar).
#[derive(Debug, Clone, Copy)]
pub struct QcCriteria {
    /// Tasa lineal máxima admisible en valor absoluto (m/año). Por encima
    /// es señal no-playa (roca, estructuras).
    pub max_abs_lrr: f64,
    /// Envolvente de cambio (SCE) máxima admisible (m). Por encima indica
    /// morfología no arenosa.
    pub max_sce: f64,
    /// Span temporal mínimo (años) para cobertura adecuada.
    pub min_years: f64,
    /// Observaciones válidas mínimas para una regresión robusta.
    pub min_valid: usize,
    /// R² mínimo del LRR (filtra ruido puro).
    pub min_r2: f64,
}

impl Default for QcCriteria {
    fn default() -> Self {
        Self {
            max_abs_lrr: 5.0,
            max_sce: 500.0,
            min_years: 20.0,
            min_valid: 50,
            min_r2: 0.01,
        }
    }
}

/// Criterio de calidad que un transecto no cumple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QcReason {
    /// `|LRR|` supera `max_abs_lrr`.
    AbsLrr,
    /// `SCE` supera `max_sce`.
    Sce,
    /// El span temporal es menor a `min_years`.
    Span,
    /// Hay menos de `min_valid` observaciones.
    NValid,
    /// El R² del LRR es menor a `min_r2`.
    R2,
}

impl QcReason {
    /// Etiqueta corta idéntica a la del pipeline de referencia.
    pub fn label(self) -> &'static str {
        match self {
            QcReason::AbsLrr => "|LRR|>5",
            QcReason::Sce => "SCE>500",
            QcReason::Span => "span<20yr",
            QcReason::NValid => "n<50",
            QcReason::R2 => "R²<0.01",
        }
    }
}

/// Evalúa los criterios de calidad sobre las tasas de un transecto,
/// **en orden**. Devuelve `None` si el transecto pasa todos, o
/// `Some(reason)` con el primer criterio que falla.
pub fn evaluate(rates: &Rates, c: &QcCriteria) -> Option<QcReason> {
    if rates.lrr.abs() > c.max_abs_lrr {
        Some(QcReason::AbsLrr)
    } else if rates.sce > c.max_sce {
        Some(QcReason::Sce)
    } else if rates.years < c.min_years {
        Some(QcReason::Span)
    } else if rates.n_valid < c.min_valid {
        Some(QcReason::NValid)
    } else if rates.lrr_r2 < c.min_r2 {
        Some(QcReason::R2)
    } else {
        None
    }
}

/// Clasificación cualitativa del cambio costero según el LRR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeClass {
    /// Erosión: `LRR < −threshold`.
    Erosion,
    /// Estable: `−threshold ≤ LRR ≤ threshold`.
    Stable,
    /// Acreción: `LRR > threshold`.
    Accretion,
}

/// Clasifica el cambio a partir del LRR (umbral típico 0.5 m/año).
pub fn classify(lrr: f64, threshold: f64) -> ChangeClass {
    if lrr < -threshold {
        ChangeClass::Erosion
    } else if lrr > threshold {
        ChangeClass::Accretion
    } else {
        ChangeClass::Stable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates_with(lrr: f64, sce: f64, years: f64, n: usize, r2: f64) -> Rates {
        Rates {
            n_valid: n,
            years,
            nsm: 0.0,
            sce,
            epr: 0.0,
            lrr,
            lrr_r2: r2,
            lrr_ci95: 0.0,
            wlr: 0.0,
            wlr_ci95: 0.0,
        }
    }

    #[test]
    fn passes_all_criteria() {
        let r = rates_with(-0.5, 200.0, 40.0, 668, 0.3);
        assert_eq!(evaluate(&r, &QcCriteria::default()), None);
    }

    #[test]
    fn first_failing_criterion_wins() {
        // falla |LRR| y SCE a la vez → se reporta |LRR| (primero en orden)
        let r = rates_with(6.0, 800.0, 40.0, 668, 0.3);
        assert_eq!(evaluate(&r, &QcCriteria::default()), Some(QcReason::AbsLrr));
        // pasa |LRR| pero falla SCE
        let r = rates_with(1.0, 800.0, 40.0, 668, 0.3);
        assert_eq!(evaluate(&r, &QcCriteria::default()), Some(QcReason::Sce));
        // pasa |LRR| y SCE, falla span
        let r = rates_with(1.0, 200.0, 10.0, 668, 0.3);
        assert_eq!(evaluate(&r, &QcCriteria::default()), Some(QcReason::Span));
        // pasa hasta n_valid
        let r = rates_with(1.0, 200.0, 40.0, 20, 0.3);
        assert_eq!(evaluate(&r, &QcCriteria::default()), Some(QcReason::NValid));
        // pasa hasta R²
        let r = rates_with(1.0, 200.0, 40.0, 668, 0.005);
        assert_eq!(evaluate(&r, &QcCriteria::default()), Some(QcReason::R2));
    }

    #[test]
    fn classification_thresholds() {
        assert_eq!(classify(-1.0, 0.5), ChangeClass::Erosion);
        assert_eq!(classify(0.0, 0.5), ChangeClass::Stable);
        assert_eq!(classify(0.5, 0.5), ChangeClass::Stable); // borde inclusivo
        assert_eq!(classify(1.0, 0.5), ChangeClass::Accretion);
    }
}
