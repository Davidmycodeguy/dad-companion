//! Hand-rolled, dependency-free logistic regression, replacing scikit-learn's
//! `LogisticRegression` for this crate. See the `model` module doc comment for the full rationale
//! for not depending on an ML crate or reimplementing gradient-boosted trees.
//!
//! Trains by batch gradient descent on the weighted, L2-regularized log-loss:
//!
//! ```text
//! L(w, b) = sum_i weight_i * log_loss(y_i, sigmoid(w·x_i + b)) + (l2 / 2) * ||w||^2
//! ```
//!
//! This is the same objective scikit-learn's `LogisticRegression(solver="lbfgs")` minimizes; only
//! the optimizer differs (gradient descent here vs. L-BFGS there), so fitted coefficients will not
//! match bit-for-bit but converge to the same decision boundary on separable or near-separable
//! data, which is all the ported behaviour needs (exact numeric parity is explicitly not required).

/// Number of gradient-descent passes over the full dataset. Small datasets (a few dozen to a few
/// thousand samples, which is the range `sort_model.py`'s `MIN_*_SAMPLES` and event-store caps put
/// us in) converge well within this budget at the learning rate below; this intentionally trades a
/// little accuracy on pathological inputs for a bounded, fast, allocation-light training step that
/// is safe to run on every local retrain.
const ITERATIONS: usize = 500;

/// Gradient-descent step size. Features are small, roughly-normalized ratios (0..1) or small
/// counts, so a moderate fixed rate converges without needing line search.
const LEARNING_RATE: f64 = 0.5;

/// L2 penalty strength, matching scikit-learn's default inverse-regularization `C=1.0` (a penalty
/// coefficient of `1/C = 1.0`).
const L2_PENALTY: f64 = 1.0;

/// Smallest allowed per-sample weight, matching `sort_model.py`'s `max(0.1, weight)`: a zero or
/// negative weight would let a sample cancel out or overpower others in a way no caller intends.
/// Callers are expected to clamp with this before calling `fit_logistic_regression`.
pub const MIN_SAMPLE_WEIGHT: f64 = 0.1;

fn sigmoid(x: f64) -> f64 {
    if x > 20.0 {
        1.0
    } else if x < -20.0 {
        0.0
    } else {
        1.0 / (1.0 + (-x).exp())
    }
}

/// Fits weighted, L2-regularized logistic regression by batch gradient descent.
///
/// `features` is one row per sample (every row the same length); `labels[i]` is the class for
/// sample `i`; `weights[i]` is that sample's importance. Returns `(coefficients, intercept)` such
/// that `sigmoid(dot(coefficients, x) + intercept)` estimates `P(label = true | x)`.
///
/// Coefficients start at zero and the intercept starts at the weighted log-odds of the positive
/// class in the training set, which reaches a good fit in far fewer iterations than starting both
/// at zero, since most of the "average" signal is captured before the first gradient step.
pub fn fit_logistic_regression(
    features: &[Vec<f64>],
    labels: &[bool],
    weights: &[f64],
) -> (Vec<f64>, f64) {
    let n_features = features.first().map_or(0, Vec::len);
    if features.is_empty() || n_features == 0 {
        return (vec![0.0; n_features], 0.0);
    }

    let total_weight: f64 = weights.iter().sum();
    let positive_weight: f64 =
        weights.iter().zip(labels).filter_map(|(w, &y)| y.then_some(*w)).sum();
    let prior_log_odds =
        (positive_weight.max(1e-6) / (total_weight - positive_weight).max(1e-6)).ln();

    let mut coefficients = vec![0.0_f64; n_features];
    let mut intercept = if prior_log_odds.is_finite() { prior_log_odds } else { 0.0 };
    let step = LEARNING_RATE / total_weight.max(1.0);

    for _ in 0..ITERATIONS {
        let mut grad_w = vec![0.0_f64; n_features];
        let mut grad_b = 0.0_f64;

        for ((row, &label), &weight) in features.iter().zip(labels).zip(weights) {
            let z: f64 = row.iter().zip(&coefficients).map(|(x, c)| x * c).sum::<f64>() + intercept;
            let target = if label { 1.0 } else { 0.0 };
            let error = (sigmoid(z) - target) * weight;
            for (g, x) in grad_w.iter_mut().zip(row) {
                *g += error * x;
            }
            grad_b += error;
        }

        for (c, g) in coefficients.iter_mut().zip(&grad_w) {
            *c -= step * (g + L2_PENALTY * *c);
        }
        intercept -= step * grad_b;
    }

    (coefficients, intercept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_two_well_separated_clusters() {
        // Arrange: points left of x=0 are negative, right of x=0 are positive.
        let features =
            vec![vec![-3.0], vec![-2.0], vec![-1.5], vec![-1.0], vec![1.0], vec![1.5], vec![2.0], vec![3.0]];
        let labels = vec![false, false, false, false, true, true, true, true];
        let weights = vec![1.0; 8];

        // Act
        let (coefficients, intercept) = fit_logistic_regression(&features, &labels, &weights);

        // Assert: a positive slope on x, and points near each training cluster classify correctly.
        assert!(coefficients[0] > 0.0);
        let predict = |x: f64| sigmoid(x * coefficients[0] + intercept);
        assert!(predict(-2.0) < 0.5);
        assert!(predict(2.0) > 0.5);
    }

    #[test]
    fn heavily_weighted_minority_sample_still_classifies_correctly() {
        // Arrange: mostly-negative data, but one positive sample carries a huge weight.
        let features = vec![vec![-1.0], vec![-1.0], vec![-1.0], vec![1.0]];
        let labels = vec![false, false, false, true];
        let weights = vec![1.0, 1.0, 1.0, 50.0];

        // Act
        let (coefficients, intercept) = fit_logistic_regression(&features, &labels, &weights);

        // Assert: the heavily-weighted positive sample still classifies as positive.
        let predict = |x: f64| sigmoid(x * coefficients[0] + intercept);
        assert!(predict(1.0) > 0.5);
    }

    #[test]
    fn empty_dataset_returns_zeroed_weights_without_panicking() {
        // Arrange / Act
        let (coefficients, intercept) = fit_logistic_regression(&[], &[], &[]);

        // Assert
        assert!(coefficients.is_empty());
        assert_eq!(intercept, 0.0);
    }
}
