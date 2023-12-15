// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! `Array.prototype.sort` as V8 runs it, including comparators that return `NaN` or disagree with themselves: the
//! result depends on the exact sequence of comparisons, so this follows V8's TimSort step by step.

use std::cmp::Ordering;

/// Sorts `items` in place with a JavaScript comparator (negative: `a` first; `NaN` counts as zero).
pub fn sort_by<T: Clone>(items: &mut [T], mut compare_fn: impl FnMut(&T, &T) -> f64) {
    let mut compare = |a: &T, b: &T| {
        let order = compare_fn(a, b);
        if order.is_nan() {
            0.0
        } else {
            order
        }
    };
    let length = items.len();
    if length < 2 {
        return;
    }
    let min_run = min_run_length(length);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut low = 0;
    let mut remaining = length;
    while remaining != 0 {
        let mut run = count_and_make_run(items, low, low + remaining, &mut compare);
        if run < min_run {
            let forced = min_run.min(remaining);
            binary_insertion_sort(items, low, low + run, low + forced, &mut compare);
            run = forced;
        }
        runs.push((low, run));
        merge_collapse(items, &mut runs, &mut compare);
        low += run;
        remaining -= run;
    }
    while runs.len() > 1 {
        let n = runs.len();
        let at = if n > 2 && runs[n - 3].1 < runs[n - 1].1 {
            n - 3
        } else {
            n - 2
        };
        merge_at(items, &mut runs, at, &mut compare);
    }
}

/// The default `Array.prototype.sort()` order of strings: by UTF-16 code units.
pub fn compare_units(a: &[u16], b: &[u16]) -> f64 {
    match a.cmp(b) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

fn min_run_length(mut n: usize) -> usize {
    let mut r = 0;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

fn count_and_make_run<T>(items: &mut [T], low: usize, high: usize, compare: &mut impl FnMut(&T, &T) -> f64) -> usize {
    let next = low + 1;
    if next == high {
        return 1;
    }
    let mut run = 2;
    let descending = compare(&items[next], &items[low]) < 0.0;
    let mut previous = next;
    for index in next + 1..high {
        let order = compare(&items[index], &items[previous]);
        if descending {
            if order >= 0.0 {
                break;
            }
        } else if order < 0.0 {
            break;
        }
        previous = index;
        run += 1;
    }
    if descending {
        items[low..low + run].reverse();
    }
    run
}

fn binary_insertion_sort<T: Clone>(
    items: &mut [T],
    low: usize,
    start: usize,
    high: usize,
    compare: &mut impl FnMut(&T, &T) -> f64,
) {
    let mut start = if low == start { start + 1 } else { start };
    while start < high {
        let pivot = items[start].clone();
        let (mut left, mut right) = (low, start);
        while left < right {
            let mid = left + ((right - left) >> 1);
            if compare(&pivot, &items[mid]) < 0.0 {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        let mut p = start;
        while p > left {
            items[p] = items[p - 1].clone();
            p -= 1;
        }
        items[left] = pivot;
        start += 1;
    }
}

fn merge_collapse<T: Clone>(items: &mut [T], runs: &mut Vec<(usize, usize)>, compare: &mut impl FnMut(&T, &T) -> f64) {
    while runs.len() > 1 {
        let n = runs.len();
        let mut at = n - 2;
        let needs_merge = |i: usize, runs: &Vec<(usize, usize)>| -> bool {
            (i >= 1 && runs[i - 1].1 <= runs[i].1 + runs[i + 1].1)
                || (i >= 2 && runs[i - 2].1 <= runs[i].1 + runs[i - 1].1)
        };
        if needs_merge(at, runs) {
            if runs[at - 1].1 < runs[at + 1].1 {
                at -= 1;
            }
        } else if runs[at].1 > runs[at + 1].1 {
            break;
        }
        merge_at(items, runs, at, compare);
    }
}

/// Merges the runs at `at` and `at + 1`. A plain stable merge: V8's galloping takes the same decisions for
/// consistent comparators, which are the only ones long enough arrays meet here.
fn merge_at<T: Clone>(
    items: &mut [T],
    runs: &mut Vec<(usize, usize)>,
    at: usize,
    compare: &mut impl FnMut(&T, &T) -> f64,
) {
    let (base_a, length_a) = runs[at];
    let (base_b, length_b) = runs[at + 1];
    runs[at] = (base_a, length_a + length_b);
    runs.remove(at + 1);
    let left: Vec<T> = items[base_a..base_a + length_a].to_vec();
    let right: Vec<T> = items[base_b..base_b + length_b].to_vec();
    let (mut i, mut j, mut k) = (0, 0, base_a);
    while i < left.len() && j < right.len() {
        if compare(&right[j], &left[i]) < 0.0 {
            items[k] = right[j].clone();
            j += 1;
        } else {
            items[k] = left[i].clone();
            i += 1;
        }
        k += 1;
    }
    for item in left[i..].iter().chain(right[j..].iter()) {
        items[k] = item.clone();
        k += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_numbers_stably() {
        let mut items = vec![(3, 'a'), (1, 'b'), (3, 'c'), (2, 'd'), (1, 'e')];
        sort_by(&mut items, |a, b| f64::from(a.0 - b.0));
        assert_eq!(items, vec![(1, 'b'), (1, 'e'), (2, 'd'), (3, 'a'), (3, 'c')]);
    }

    #[test]
    fn nan_comparisons_follow_v8() {
        let shown = |items: Vec<f64>| -> Vec<String> {
            let mut items = items;
            sort_by(&mut items, |a, b| a - b);
            items.iter().map(|n| n.to_string()).collect()
        };
        assert_eq!(shown(vec![3.0, f64::NAN, 1.0]), ["3", "NaN", "1"]);
        assert_eq!(shown(vec![5.0, 2.0, f64::NAN, 4.0, 1.0]), ["1", "2", "4", "5", "NaN"]);
        assert_eq!(shown(vec![f64::NAN, 3.0, 1.0, 2.0]), ["NaN", "1", "2", "3"]);
    }

    #[test]
    fn long_arrays_sort() {
        let mut items: Vec<i32> = (0..200).map(|i| (i * 37) % 101).collect();
        sort_by(&mut items, |a, b| f64::from(a - b));
        assert!(items.windows(2).all(|w| w[0] <= w[1]));
    }
}
