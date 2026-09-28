//! A minimal compressed-sparse-row matrix: just enough linear algebra (`matvec`, `rmatvec`,
//! column means) for the ridge solve in [`super::ridge`]. Values are one-hot design-matrix
//! entries (mostly `1.0`), so a dense matrix would waste memory at 100k+ rows.

/// A sparse matrix in compressed-sparse-row form: row `i`'s entries are
/// `col_idx[row_ptr[i]..row_ptr[i + 1]]` paired with `values[row_ptr[i]..row_ptr[i + 1]]`.
#[derive(Debug, Clone)]
pub(crate) struct Csr {
    n_rows: usize,
    n_cols: usize,
    row_ptr: Vec<usize>,
    col_idx: Vec<u32>,
    values: Vec<f64>,
}

/// Builds a [`Csr`] one row at a time, in row order.
#[derive(Debug, Default)]
pub(crate) struct CsrBuilder {
    n_cols: usize,
    row_ptr: Vec<usize>,
    col_idx: Vec<u32>,
    values: Vec<f64>,
}

impl CsrBuilder {
    pub(crate) fn new() -> Self {
        Self { row_ptr: vec![0], ..Self::default() }
    }

    /// Appends one row's nonzero `(column, value)` entries and closes the row.
    pub(crate) fn push_row(&mut self, entries: impl IntoIterator<Item = (u32, f64)>) {
        for (col, value) in entries {
            self.n_cols = self.n_cols.max(col as usize + 1);
            self.col_idx.push(col);
            self.values.push(value);
        }
        self.row_ptr.push(self.col_idx.len());
    }

    pub(crate) fn finish(self) -> Csr {
        let n_rows = self.row_ptr.len() - 1;
        Csr { n_rows, n_cols: self.n_cols, row_ptr: self.row_ptr, col_idx: self.col_idx, values: self.values }
    }
}

impl Csr {
    fn row(&self, i: usize) -> (&[u32], &[f64]) {
        let (start, end) = (self.row_ptr[i], self.row_ptr[i + 1]);
        (&self.col_idx[start..end], &self.values[start..end])
    }

    /// `X @ v`.
    pub(crate) fn matvec(&self, v: &[f64]) -> Vec<f64> {
        (0..self.n_rows)
            .map(|i| {
                let (cols, vals) = self.row(i);
                cols.iter().zip(vals).map(|(&c, &x)| x * v[c as usize]).sum()
            })
            .collect()
    }

    /// `Xᵀ @ v`.
    pub(crate) fn rmatvec(&self, v: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.n_cols];
        for (i, &vi) in v.iter().enumerate().take(self.n_rows) {
            let (cols, vals) = self.row(i);
            for (&c, &x) in cols.iter().zip(vals) {
                out[c as usize] += x * vi;
            }
        }
        out
    }

    /// The mean of each column across all rows (columns with no entry in a row count as 0 there).
    pub(crate) fn col_means(&self) -> Vec<f64> {
        let mut sums = vec![0.0; self.n_cols];
        for (&c, &v) in self.col_idx.iter().zip(&self.values) {
            sums[c as usize] += v;
        }
        let n = self.n_rows.max(1) as f64;
        sums.iter().map(|s| s / n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::CsrBuilder;

    fn sample() -> super::Csr {
        // [[1, 0, 2], [0, 3, 0]]
        let mut b = CsrBuilder::new();
        b.push_row([(0, 1.0), (2, 2.0)]);
        b.push_row([(1, 3.0)]);
        b.finish()
    }

    #[test]
    fn matvec_multiplies_by_the_dense_equivalent() {
        let m = sample();
        assert_eq!(m.matvec(&[1.0, 1.0, 1.0]), vec![3.0, 3.0]);
        assert_eq!(m.matvec(&[2.0, 0.0, 5.0]), vec![12.0, 0.0]);
    }

    #[test]
    fn rmatvec_multiplies_by_the_transpose() {
        let m = sample();
        assert_eq!(m.rmatvec(&[1.0, 1.0]), vec![1.0, 3.0, 2.0]);
    }

    #[test]
    fn col_means_average_over_every_row_including_implicit_zeros() {
        let m = sample();
        assert_eq!(m.col_means(), vec![0.5, 1.5, 1.0]);
    }

    #[test]
    fn shape_matches_rows_pushed_and_the_highest_column_seen() {
        let m = sample();
        assert_eq!(m.matvec(&[0.0, 0.0, 0.0]).len(), 2); // 2 rows
        assert_eq!(m.col_means().len(), 3); // 3 columns
    }
}
