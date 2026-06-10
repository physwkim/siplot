//! Data-distribution histogram of a scalar image, faithful to silx
//! `ColormapDialog.computeHistogram` (`gui/dialog/ColormapDialog.py:1227-1295`).
//!
//! Shared between the modal [`ColormapDialog`](crate::ColormapDialog) and the
//! inline `HistogramColorBar`; this is the single home for the binning logic so
//! the two never drift.

/// Compute the data-distribution histogram of a flattened scalar image.
///
/// `data` is the flattened scalar image. `range` optionally fixes the histogram
/// extent `(min, max)`; when `None` the finite min/max of `data` is used. `log`
/// selects logarithmic binning (silx `scale == LOGARITHM`): the samples and the
/// range are taken to `log10` and binned uniformly in log space, but the
/// returned edges are mapped back to linear (`10**edge`) so they plot on a log
/// x-axis.
///
/// Returns `(counts, edges)` with `counts.len() + 1 == edges.len()`, or `None`
/// when there is no finite data / no valid range (silx returns `(None, None)`).
/// The bin count is `clamp(2, min(256, floor(sqrt(N))))` — silx `nbins` (the
/// integer-data 256-bin special case does not apply to scalar `f64` images).
pub fn compute_histogram(
    data: &[f64],
    range: Option<(f64, f64)>,
    log: bool,
) -> Option<(Vec<u64>, Vec<f64>)> {
    if data.is_empty() {
        return None;
    }
    // In log mode silx transforms the samples (and the range) to log10 first,
    // bins uniformly in log space, then converts the edges back to linear.
    let xform = |v: f64| if log { v.log10() } else { v };

    // Histogram extent in the (log-)transformed space.
    let (mut xmin, mut xmax) = match range {
        Some((lo, hi)) => (xform(lo), xform(hi)),
        None => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for &v in data {
                let t = xform(v);
                if t.is_finite() {
                    lo = lo.min(t);
                    hi = hi.max(t);
                }
            }
            (lo, hi)
        }
    };
    if !xmin.is_finite() || !xmax.is_finite() {
        return None;
    }
    if xmax < xmin {
        std::mem::swap(&mut xmin, &mut xmax);
    }

    // silx: nbins = max(2, min(256, int(sqrt(N)))).
    let nbins = ((data.len() as f64).sqrt().floor() as usize).clamp(2, 256);
    let span = xmax - xmin;
    let mut counts = vec![0u64; nbins];
    if span > 0.0 {
        for &v in data {
            let t = xform(v);
            if !t.is_finite() || t < xmin || t > xmax {
                continue;
            }
            // Last bin is inclusive of `xmax` (numpy/Histogramnd convention).
            let idx = ((((t - xmin) / span) * nbins as f64) as usize).min(nbins - 1);
            counts[idx] += 1;
        }
    } else {
        // Degenerate (all-equal) range: every finite sample lands in bin 0.
        for &v in data {
            if xform(v).is_finite() {
                counts[0] += 1;
            }
        }
    }

    // Edges in the transformed space, mapped back to linear when log.
    let inv = |e: f64| if log { 10f64.powf(e) } else { e };
    let edges: Vec<f64> = (0..=nbins)
        .map(|i| inv(xmin + span * (i as f64) / nbins as f64))
        .collect();

    Some((counts, edges))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_histogram_bin_count_follows_silx_sqrt_rule() {
        // nbins = clamp(2, min(256, floor(sqrt(N)))).
        // N=100 -> floor(sqrt)=10 bins; edges has nbins+1 entries.
        let data: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let (counts, edges) = compute_histogram(&data, None, false).expect("histogram");
        assert_eq!(counts.len(), 10);
        assert_eq!(edges.len(), 11);
        // Every finite sample is binned exactly once.
        assert_eq!(counts.iter().sum::<u64>(), 100);
        // Extent spans the finite data min/max.
        assert_eq!(edges[0], 0.0);
        assert_eq!(*edges.last().unwrap(), 99.0);

        // Cap at 256 bins for large N (floor(sqrt(1_000_000)) = 1000 -> 256).
        let big = vec![0.5; 1_000_000];
        let (c, _) = compute_histogram(&big, Some((0.0, 1.0)), false).expect("histogram");
        assert_eq!(c.len(), 256);

        // Floor below 2 is lifted to 2 (N=1 -> floor(sqrt)=1 -> 2).
        let (c1, e1) = compute_histogram(&[3.0], Some((0.0, 6.0)), false).expect("histogram");
        assert_eq!(c1.len(), 2);
        assert_eq!(e1.len(), 3);
    }

    #[test]
    fn compute_histogram_uses_supplied_range_and_counts_in_range() {
        // With an explicit range, out-of-range samples are dropped; the last bin
        // is inclusive of the upper edge.
        let data = vec![-5.0, 0.0, 2.5, 5.0, 5.0, 10.0];
        let (counts, edges) = compute_histogram(&data, Some((0.0, 5.0)), false).expect("histogram");
        assert_eq!(edges[0], 0.0);
        assert_eq!(*edges.last().unwrap(), 5.0);
        // -5.0 and 10.0 are outside [0, 5]; 0.0, 2.5, 5.0, 5.0 are inside.
        assert_eq!(counts.iter().sum::<u64>(), 4);
    }

    #[test]
    fn compute_histogram_log_bins_uniformly_in_log_space() {
        // Decade-spaced data over [1, 1000]: log10 -> [0, 3] uniform bins, edges
        // mapped back to linear (10**edge).
        let data = vec![1.0, 10.0, 100.0, 1000.0];
        let (counts, edges) = compute_histogram(&data, None, true).expect("histogram");
        // floor(sqrt(4)) = 2 bins -> edges 10^0, 10^1.5, 10^3.
        assert_eq!(counts.len(), 2);
        assert!((edges[0] - 1.0).abs() < 1e-9, "{}", edges[0]);
        assert!((edges[1] - 10f64.powf(1.5)).abs() < 1e-6, "{}", edges[1]);
        assert!((edges[2] - 1000.0).abs() < 1e-6, "{}", edges[2]);
        assert_eq!(counts.iter().sum::<u64>(), 4);
    }

    #[test]
    fn compute_histogram_empty_or_nonfinite_is_none() {
        assert!(compute_histogram(&[], None, false).is_none());
        assert!(compute_histogram(&[f64::NAN, f64::INFINITY], None, false).is_none());
    }

    #[test]
    fn compute_histogram_degenerate_range_counts_all_in_first_bin() {
        // All-equal data: zero-width extent -> every finite sample into bin 0.
        let data = vec![7.0; 5];
        let (counts, edges) = compute_histogram(&data, None, false).expect("histogram");
        assert_eq!(counts[0], 5);
        assert_eq!(counts.iter().sum::<u64>(), 5);
        assert_eq!(edges[0], 7.0);
        assert_eq!(*edges.last().unwrap(), 7.0);
    }
}
