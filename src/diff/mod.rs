//! DeepDiff 8.6.2の既定設定に合わせたJSON差分。
//!
//! scalar配列はSequenceMatcher相当の連続一致を使い、差分件数が同数以下なら
//! 位置比較を優先する。辞書・配列を含む配列は最初から位置比較する。

mod path;
mod scalar;

pub use path::{PathSegment, format_path, get_by_diff_key};

use serde_json::Value;
use std::collections::{HashMap, HashSet};

use scalar::{ScalarKey, number_equal};

/// ソート済みの差分パス。indexの基準はDeepDiffのopcodeに従う。
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffKeys {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

impl DiffKeys {
    fn len(&self) -> usize {
        self.added.len() + self.changed.len() + self.removed.len()
    }

    fn extend(&mut self, other: Self) {
        self.added.extend(other.added);
        self.changed.extend(other.changed);
        self.removed.extend(other.removed);
    }
}

/// 2つのJSON値を比較する。`__`で始まる辞書キーはDeepDiffと同じく無視する。
pub fn diff(one: &Value, other: &Value) -> crate::compat::Result<DiffKeys> {
    let depth = crate::compat::json::validate(one)?.max(crate::compat::json::validate(other)?);
    crate::compat::json::with_stack(depth, || Ok(diff_validated(one, other)))
}

fn diff_validated(one: &Value, other: &Value) -> DiffKeys {
    let mut result = DiffKeys::default();
    compare(one, other, "root", &mut result);
    for paths in [&mut result.added, &mut result.changed, &mut result.removed] {
        paths.sort();
        paths.dedup();
    }
    result
}

fn compare(one: &Value, other: &Value, path: &str, result: &mut DiffKeys) {
    match (one, other) {
        (Value::Object(a), Value::Object(b)) => {
            let a_keys: Vec<_> = a.keys().filter(|key| !key.starts_with("__")).collect();
            let b_keys: Vec<_> = b.keys().filter(|key| !key.starts_with("__")).collect();
            let common = a_keys.iter().filter(|key| b.contains_key(**key)).count();
            let union = a_keys.len() + b_keys.len() - common;
            // threshold_to_diff_deeper=0.33。空辞書→単一キーだけは追加となる。
            if union > 1 && (common as f64) / (union as f64) < 0.33 {
                result.changed.push(path.into());
                return;
            }
            for key in a_keys {
                let child = path::key_path(path, key);
                if let Some(value) = b.get(key) {
                    compare(&a[key], value, &child, result);
                } else {
                    result.removed.push(child);
                }
            }
            for key in b_keys {
                if !a.contains_key(key) {
                    result.added.push(path::key_path(path, key));
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            let scalar = |v: &Value| !v.is_array() && !v.is_object();
            let mut changes = if a.iter().all(scalar) && b.iter().all(scalar) {
                let matched = compare_sequence(a, b, path);
                if matched.len() > 1 {
                    let positional = compare_pairs(a, b, 0, 0, path);
                    if positional.len() <= matched.len() {
                        positional
                    } else {
                        matched
                    }
                } else {
                    matched
                }
            } else {
                compare_pairs(a, b, 0, 0, path)
            };
            // DeepDiffのmutual_add_removes_to_become_value_changesに対応する。
            let removed: HashSet<_> = changes.removed.iter().collect();
            let mutual: HashSet<_> = changes
                .added
                .iter()
                .filter(|p| removed.contains(p))
                .cloned()
                .collect();
            changes.added.retain(|p| !mutual.contains(p));
            changes.removed.retain(|p| !mutual.contains(p));
            changes.changed.extend(mutual);
            result.extend(changes);
        }
        (Value::Number(a), Value::Number(b)) => {
            if !number_equal(a, b) {
                result.changed.push(path.into());
            }
        }
        _ if one != other => result.changed.push(path.into()),
        _ => {}
    }
}

fn compare_pairs(a: &[Value], b: &[Value], a_start: usize, b_start: usize, path: &str) -> DiffKeys {
    let mut result = DiffKeys::default();
    for i in 0..a.len().max(b.len()) {
        let child = format!("{path}<{}>", a_start + i);
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => compare(x, y, &child, &mut result),
            (Some(_), None) => result.removed.push(child),
            (None, Some(_)) => result.added.push(format!("{path}<{}>", b_start + i)),
            (None, None) => unreachable!(),
        }
    }
    result
}

/// difflibの最長連続一致を分割統治で列挙する。同長なら左側の最初の一致を選ぶ。
/// b2jは全体から1回構築する。各探索区間では一致候補だけをj2lenで伸ばす。
/// 全体のコストは各探索区間のaの走査・候補位置の検索・一致候補数の総和。
/// 不一致の組み合わせは走査しないが、重複や分割次第で線形時間にはならない。
/// メモリは入力長に対して線形。
/// autojunk=Falseなので頻出要素も省かず、i・jの昇順で同長一致の優先順位を保つ。
fn matching_blocks(a: &[Value], b: &[Value]) -> Vec<(usize, usize, usize)> {
    let a: Vec<_> = a.iter().map(ScalarKey::from).collect();
    let mut b2j: HashMap<ScalarKey, Vec<usize>> = HashMap::new();
    for (j, value) in b.iter().enumerate() {
        b2j.entry(ScalarKey::from(value)).or_default().push(j);
    }
    let mut pending = vec![(0, a.len(), 0, b.len())];
    let mut blocks = Vec::new();
    while let Some((alo, ahi, blo, bhi)) = pending.pop() {
        let (mut ai, mut bi, mut size) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        let mut next = HashMap::new();
        for (i, x) in a.iter().enumerate().take(ahi).skip(alo) {
            next.clear();
            if let Some(positions) = b2j.get(x) {
                let start = positions.partition_point(|&j| j < blo);
                for &j in &positions[start..] {
                    if j >= bhi {
                        break;
                    }
                    let length = j
                        .checked_sub(1)
                        .and_then(|j| j2len.get(&j))
                        .copied()
                        .unwrap_or(0)
                        + 1;
                    next.insert(j, length);
                    if length > size {
                        (ai, bi, size) = (i + 1 - length, j + 1 - length, length);
                    }
                }
            }
            std::mem::swap(&mut j2len, &mut next);
        }
        if size != 0 {
            blocks.push((ai, bi, size));
            if alo < ai && blo < bi {
                pending.push((alo, ai, blo, bi));
            }
            if ai + size < ahi && bi + size < bhi {
                pending.push((ai + size, ahi, bi + size, bhi));
            }
        }
    }
    blocks.sort_unstable();
    blocks.push((a.len(), b.len(), 0));
    blocks
}

fn compare_sequence(a: &[Value], b: &[Value], path: &str) -> DiffKeys {
    let mut result = DiffKeys::default();
    let (mut ai, mut bi) = (0, 0);
    for (i, j, size) in matching_blocks(a, b) {
        if ai < i && bi < j {
            result.extend(compare_pairs(&a[ai..i], &b[bi..j], ai, bi, path));
        } else if ai < i {
            result
                .removed
                .extend((ai..i).map(|n| format!("{path}<{n}>")));
        } else if bi < j {
            result.added.extend((bi..j).map(|n| format!("{path}<{n}>")));
        }
        ai = i + size;
        bi = j + size;
    }
    result
}
