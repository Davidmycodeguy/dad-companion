//! Ridge regression on a sparse design matrix, solved the way scikit-learn's `Ridge(solver=
//! "sparse_cg")` does: conjugate gradient on the normal equations of the *mean-centered* matrix,
//! which keeps the intercept unpenalized without densifying `X` or adding an intercept column.
//!
//! For a sparse `X`, sklearn's `_preprocess_data` computes `X_offset` (each column's mean) and
//! `y_offset` (`mean(y)`) but leaves `X` itself sparse and uncentered. `_solve_sparse_cg` then
//! multiplies by the *implicitly* centered `X` through a linear operator: `(X - X_offset) @ v =
//! X @ v - (X_offset · v)` (and the transpose analogously), so CG solves
//! `(Xᵀ_c X_c + αI) β = Xᵀ_c y_c` without ever materializing `X_c`. Finally
//! `intercept = y_offset - X_offset · β` (`Ridge._set_intercept`). Predictions then use the
//! original, uncentered `X`: `ŷ = X @ β + intercept`.

use super::sparse::Csr;

const RIDGE_TOL: f64 = 1e-6;
const RIDGE_MAX_ITER: usize = 5000;

pub(crate) struct RidgeFit {
    pub(crate) coef: Vec<f64>,
    pub(crate) intercept: f64,
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// `(X - offset) @ v`, without forming the centered matrix.
fn centered_matvec(x: &Csr, offset: &[f64], v: &[f64]) -> Vec<f64> {
    let shift = dot(offset, v);
    x.matvec(v).into_iter().map(|r| r - shift).collect()
}

/// `(X - offset)ᵀ @ v`, without forming the centered matrix.
fn centered_rmatvec(x: &Csr, offset: &[f64], v: &[f64]) -> Vec<f64> {
    let sum_v: f64 = v.iter().sum();
    x.rmatvec(v).into_iter().zip(offset).map(|(r, o)| r - o * sum_v).collect()
}

/// Unpreconditioned conjugate gradient for a symmetric positive-definite `apply` (a matrix-vector
/// product), starting from zero. Stops once the residual is small relative to `rhs`, matching
/// scipy's `cg(..., rtol=tol)` closely enough that both solvers converge to the same optimum (the
/// ridge normal equations have a unique minimizer regardless of solver internals).
///
/// With `alpha > 0` (as `TrainOptions::default()` and every real caller use) `apply` is strictly
/// positive-definite and this never divides by zero. A caller passing `alpha: 0.0` against a
/// rank-deficient design (routine for a sparse one-hot matrix) could otherwise hit a `p·Ap == 0`
/// direction; rather than divide and poison every coefficient with NaN, that's treated as
/// converged-as-far-as-possible and returns the best `x` found so far.
fn conjugate_gradient(apply: impl Fn(&[f64]) -> Vec<f64>, rhs: &[f64], tol: f64, max_iter: usize) -> Vec<f64> {
    let n = rhs.len();
    let bnorm = dot(rhs, rhs).sqrt();
    let mut x = vec![0.0; n];
    if bnorm == 0.0 {
        return x;
    }
    let threshold = tol * bnorm;
    let mut r = rhs.to_vec();
    let mut p = r.clone();
    let mut rs_old = dot(&r, &r);
    for _ in 0..max_iter {
        if rs_old.sqrt() <= threshold {
            break;
        }
        let ap = apply(&p);
        let denom = dot(&p, &ap);
        if denom == 0.0 {
            break;
        }
        let alpha = rs_old / denom;
        for i in 0..n {
            x[i] += alpha * p[i];
        }
        for i in 0..n {
            r[i] -= alpha * ap[i];
        }
        let rs_new = dot(&r, &r);
        if rs_new.sqrt() <= threshold {
            break;
        }
        let beta = rs_new / rs_old;
        for i in 0..n {
            p[i] = r[i] + beta * p[i];
        }
        rs_old = rs_new;
    }
    x
}

/// A single ridge fit of `y ~ X`, with an unpenalized intercept handled by centering.
pub(crate) fn fit_ridge(x: &Csr, y: &[f64], alpha: f64) -> RidgeFit {
    let offset = x.col_means();
    let y_offset = mean(y);
    let yc: Vec<f64> = y.iter().map(|v| v - y_offset).collect();
    let rhs = centered_rmatvec(x, &offset, &yc);
    let apply = |v: &[f64]| -> Vec<f64> {
        let xv = centered_matvec(x, &offset, v);
        let xtxv = centered_rmatvec(x, &offset, &xv);
        xtxv.iter().zip(v).map(|(a, b)| a + alpha * b).collect()
    };
    let coef = conjugate_gradient(apply, &rhs, RIDGE_TOL, RIDGE_MAX_ITER);
    let intercept = y_offset - dot(&offset, &coef);
    RidgeFit { coef, intercept }
}

/// `X @ coef + intercept`, on the original (uncentered) `X` — matches sklearn's `predict`.
pub(crate) fn predict(x: &Csr, fit: &RidgeFit) -> Vec<f64> {
    x.matvec(&fit.coef).into_iter().map(|v| v + fit.intercept).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worth_train::sparse::CsrBuilder;

    /// y = 2*x0 - x1 + 5, no noise: ridge with a tiny alpha should recover it closely.
    fn linear_data() -> (Csr, Vec<f64>) {
        let mut b = CsrBuilder::new();
        let mut y = Vec::new();
        let points = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (2.0, 1.0), (1.0, 3.0), (3.0, 2.0), (4.0, 0.0), (2.0, 2.0)];
        for (x0, x1) in points {
            b.push_row([(0u32, x0), (1u32, x1)]);
            y.push(2.0 * x0 - x1 + 5.0);
        }
        (b.finish(), y)
    }

    #[test]
    fn recovers_a_noiseless_linear_relationship_with_small_alpha() {
        let (x, y) = linear_data();
        let fit = fit_ridge(&x, &y, 1e-8);
        assert!((fit.coef[0] - 2.0).abs() < 1e-4, "coef0 = {}", fit.coef[0]);
        assert!((fit.coef[1] + 1.0).abs() < 1e-4, "coef1 = {}", fit.coef[1]);
        assert!((fit.intercept - 5.0).abs() < 1e-4, "intercept = {}", fit.intercept);
    }

    #[test]
    fn predict_matches_fitted_values_closely_for_a_well_fit_model() {
        let (x, y) = linear_data();
        let fit = fit_ridge(&x, &y, 1e-8);
        let predicted = predict(&x, &fit);
        for (p, actual) in predicted.iter().zip(&y) {
            assert!((p - actual).abs() < 1e-3, "predicted {p}, actual {actual}");
        }
    }

    #[test]
    fn larger_alpha_shrinks_coefficients_toward_zero() {
        let (x, y) = linear_data();
        let loose = fit_ridge(&x, &y, 1e-8);
        let tight = fit_ridge(&x, &y, 1000.0);
        assert!(tight.coef[0].abs() < loose.coef[0].abs());
        assert!(tight.coef[1].abs() < loose.coef[1].abs());
    }

    #[test]
    fn zero_alpha_on_a_rank_deficient_design_stays_finite() {
        // Two columns that are always equal (perfectly collinear): X^T X is singular, and with
        // alpha = 0 the normal-equations operator is only positive *semi*-definite. Regressed
        // against sklearn's own `Ridge(alpha=0)` behavior on singular input, the goal here is
        // narrower: never emit NaN/Infinity (which `serde_json` cannot serialize).
        let mut b = CsrBuilder::new();
        let mut y = Vec::new();
        for i in 0..6 {
            b.push_row([(0u32, i as f64), (1u32, i as f64)]); // column 1 always equals column 0
            y.push(2.0 * i as f64 + 1.0);
        }
        let fit = fit_ridge(&b.finish(), &y, 0.0);
        assert!(fit.intercept.is_finite());
        assert!(fit.coef.iter().all(|c| c.is_finite()), "{:?}", fit.coef);
    }
}
