//! A faithful port of the two pieces of Python's `difflib` this module needs: the Ratcliff/
//! Obershelp similarity ratio and `get_close_matches`. A best match here picks a game-data stat id
//! or item id, so this matches CPython's algorithm exactly (including its quick-reject prefilters
//! and the `heapq.nlargest` tie-break), not just an approximation of it.
//!
//! `tooltip_parser.py` never passes `isjunk` to `SequenceMatcher`, so unlike CPython there is no
//! junk-only match-extension pass here: `bjunk` would always be empty, making that pass dead code.

use std::collections::HashMap;

/// A matching block: `a[ai..ai+size]` equals `b[bi..bi+size]`.
type Block = (usize, usize, usize);

/// Above this length, `SequenceMatcher`'s "autojunk" starts treating very common characters in `b`
/// as noise (see `b2j`). Matches CPython; nothing in this crate ever compares strings this long, so
/// the path is here for fidelity rather than because it fires in practice.
const AUTOJUNK_MIN_LEN: usize = 200;

/// Every index at which each character of `b` occurs, for `find_longest_match`. CPython's
/// "autojunk": once `b` has at least `AUTOJUNK_MIN_LEN` characters, one that makes up more than
/// roughly 1% of `b` is dropped, since a character that common is noise, not a real match.
fn b2j(b: &[char]) -> HashMap<char, Vec<usize>> {
    let mut index: HashMap<char, Vec<usize>> = HashMap::new();
    for (i, &c) in b.iter().enumerate() {
        index.entry(c).or_default().push(i);
    }
    if b.len() >= AUTOJUNK_MIN_LEN {
        let ntest = b.len() / 100 + 1;
        index.retain(|_, idxs| idxs.len() <= ntest);
    }
    index
}

/// CPython's `SequenceMatcher.find_longest_match`, restricted to `a[alo..ahi]` vs `b[blo..bhi]`:
/// the longest run common to both, preferring the earliest such run in `a` and, among ties there,
/// in `b`, then extended in both directions through any further equal elements.
fn find_longest_match(
    a: &[char],
    b: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> Block {
    let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for (i, ai) in a.iter().enumerate().take(ahi).skip(alo) {
        let mut new_j2len: HashMap<usize, usize> = HashMap::new();
        if let Some(js) = b2j.get(ai) {
            for &j in js {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break; // indices for one character are stored in ascending order
                }
                let prev = if j == 0 { 0 } else { j2len.get(&(j - 1)).copied().unwrap_or(0) };
                let k = prev + 1;
                new_j2len.insert(j, k);
                if k > bestsize {
                    (besti, bestj, bestsize) = (i + 1 - k, j + 1 - k, k);
                }
            }
        }
        j2len = new_j2len;
    }
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
        bestsize += 1;
    }
    (besti, bestj, bestsize)
}

/// CPython's `SequenceMatcher.get_matching_blocks`: every maximal matching block between `a` and
/// `b`, left to right, found by recursively splitting around each longest match in turn
/// (Ratcliff/Obershelp), then merging blocks that turned out to be adjacent.
fn matching_blocks(a: &[char], b: &[char], b2j: &HashMap<char, Vec<usize>>) -> Vec<Block> {
    let (la, lb) = (a.len(), b.len());
    let mut stack = vec![(0, la, 0, lb)];
    let mut found = Vec::new();
    while let Some((alo, ahi, blo, bhi)) = stack.pop() {
        let (i, j, k) = find_longest_match(a, b, b2j, alo, ahi, blo, bhi);
        if k > 0 {
            found.push((i, j, k));
            if alo < i && blo < j {
                stack.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                stack.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    found.sort_unstable();

    let mut blocks = Vec::with_capacity(found.len());
    let (mut i1, mut j1, mut k1) = (0usize, 0usize, 0usize);
    for (i2, j2, k2) in found {
        if i1 + k1 == i2 && j1 + k1 == j2 {
            k1 += k2;
        } else {
            if k1 > 0 {
                blocks.push((i1, j1, k1));
            }
            (i1, j1, k1) = (i2, j2, k2);
        }
    }
    if k1 > 0 {
        blocks.push((i1, j1, k1));
    }
    blocks
}

/// CPython's `_calculate_ratio`: twice the matched elements over the total length of both
/// sequences (so two empty sequences are declared identical, ratio 1.0).
fn calculate_ratio(matches: usize, length: usize) -> f64 {
    if length == 0 {
        1.0
    } else {
        2.0 * matches as f64 / length as f64
    }
}

fn char_counts(s: &[char]) -> HashMap<char, usize> {
    let mut counts = HashMap::new();
    for &c in s {
        *counts.entry(c).or_insert(0) += 1;
    }
    counts
}

/// CPython's `SequenceMatcher.quick_ratio`: an upper bound on `ratio` from how much `a` and `b`'s
/// elements overlap as multisets, ignoring order (cheaper than a real ratio, and never smaller).
fn quick_ratio(a: &[char], b: &[char]) -> f64 {
    let (count_a, count_b) = (char_counts(a), char_counts(b));
    let matches: usize = count_a.iter().map(|(c, &n)| n.min(count_b.get(c).copied().unwrap_or(0))).sum();
    calculate_ratio(matches, a.len() + b.len())
}

/// CPython's `SequenceMatcher.real_quick_ratio`: the cheapest possible upper bound on `ratio`,
/// from the two lengths alone.
fn real_quick_ratio(a: &[char], b: &[char]) -> f64 {
    calculate_ratio(a.len().min(b.len()), a.len() + b.len())
}

/// CPython's `difflib.SequenceMatcher(None, a, b).ratio()`.
pub fn ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let matches: usize = matching_blocks(&a, &b, &b2j(&b)).iter().map(|&(_, _, k)| k).sum();
    calculate_ratio(matches, a.len() + b.len())
}

/// CPython's `difflib.get_close_matches(word, possibilities, n, cutoff)`: the `n` possibilities
/// that best match `word` at or above `cutoff` (real_quick_ratio, then quick_ratio, then the full
/// ratio, each an upper bound on the next, tried cheapest first), best match first. Ties keep
/// whichever candidate sorts lexicographically last, matching `heapq.nlargest`'s ordering of the
/// `(ratio, candidate)` tuples it collects.
pub fn get_close_matches<'a>(word: &str, possibilities: &[&'a str], n: usize, cutoff: f64) -> Vec<&'a str> {
    let b: Vec<char> = word.chars().collect();
    let index = b2j(&b);
    let mut scored: Vec<(f64, &str)> = Vec::new();
    for &candidate in possibilities {
        let a: Vec<char> = candidate.chars().collect();
        if real_quick_ratio(&a, &b) < cutoff || quick_ratio(&a, &b) < cutoff {
            continue;
        }
        let matches: usize = matching_blocks(&a, &b, &index).iter().map(|&(_, _, k)| k).sum();
        let r = calculate_ratio(matches, a.len() + b.len());
        if r >= cutoff {
            scored.push((r, candidate));
        }
    }
    scored.sort_by(|x, y| y.0.total_cmp(&x.0).then_with(|| y.1.cmp(x.1)));
    scored.truncate(n);
    scored.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_of_identical_strings_is_one() {
        assert_eq!(ratio("woolencap", "woolencap"), 1.0);
    }

    #[test]
    fn ratio_matches_python_for_a_slightly_misread_title() {
        // difflib.SequenceMatcher(None, "occultistrobe", "occutistrohe").ratio() == 0.88
        assert_eq!(ratio("occultistrobe", "occutistrohe"), 0.88);
    }

    #[test]
    fn ratio_can_be_asymmetric_like_cpython() {
        // difflib.SequenceMatcher(None, a, b).ratio() picks the first-scanned tie among equally
        // long candidate matches, so swapping a and b can (rarely) change which blocks are found.
        assert_eq!(ratio("baba", "bbaabca"), 0.7272727272727273);
        assert_eq!(ratio("bbaabca", "baba"), 0.5454545454545454);
    }

    #[test]
    fn ratio_ignores_extremely_common_characters_once_b_is_long_enough() {
        let a = "x".repeat(10) + &"y".repeat(10);
        let b = "x".repeat(150) + &"y".repeat(60); // len 210: autojunk removes both 'x' and 'y'
        assert_eq!(ratio(&a, &b), 0.08695652173913043);
    }

    #[test]
    fn get_close_matches_picks_the_best_scoring_candidate() {
        let candidates = ["occultistrobe", "woolencap", "armingsword", "gemringperfect", "quarterstaff"];
        assert_eq!(get_close_matches("occutistrohe", &candidates, 1, 0.85), vec!["occultistrobe"]);
    }

    #[test]
    fn get_close_matches_rejects_everything_below_cutoff() {
        let candidates = ["luck"];
        assert_eq!(get_close_matches("brandnewstat", &candidates, 1, 0.8), Vec::<&str>::new());
    }

    #[test]
    fn get_close_matches_ties_favour_the_lexicographically_greatest_candidate() {
        // difflib.get_close_matches("cab", ["cab1", "cab2"], n=1, cutoff=0.1) == ["cab2"]
        assert_eq!(get_close_matches("cab", &["cab1", "cab2"], 1, 0.1), vec!["cab2"]);
        // difflib.get_close_matches("ab", ["ac", "bc"], n=1, cutoff=0.1) == ["bc"]
        assert_eq!(get_close_matches("ab", &["ac", "bc"], 1, 0.1), vec!["bc"]);
    }

    #[test]
    fn get_close_matches_can_return_more_than_one() {
        // difflib.get_close_matches("ab", ["ab", "ac", "zz"], n=2, cutoff=0.4) == ["ab", "ac"]
        assert_eq!(get_close_matches("ab", &["ab", "ac", "zz"], 2, 0.4), vec!["ab", "ac"]);
    }
}
