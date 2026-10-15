//! Pure, off-thread row protocol. Qt only applies these already-indexed edits.
use std::collections::HashSet;

pub type RoleMask = u64;
pub trait Row: Clone + PartialEq + Send + Sync + 'static {
    fn key(&self) -> &str;
    fn diff(&self, old: &Self) -> RoleMask;
}
impl<T: Row> Row for std::sync::Arc<T> {
    fn key(&self) -> &str {
        (**self).key()
    }
    fn diff(&self, old: &Self) -> RoleMask {
        if std::sync::Arc::ptr_eq(self, old) {
            0
        } else {
            (**self).diff(old)
        }
    }
}
#[derive(Debug, Clone)]
pub enum ModelDelta<T> {
    Insert {
        index: usize,
        row: T,
    },
    Remove {
        index: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
    Change {
        index: usize,
        row: T,
        roles: RoleMask,
    },
    ChangeRange {
        index: usize,
        rows: Vec<T>,
        roles: RoleMask,
    },
}

/// Baseline must be the last committed/queued model, never a skipped watch value.
pub fn diff<T: Row>(before: &[T], after: &[T]) -> Vec<ModelDelta<T>> {
    // Fast path: same keys in the same order (the common title/focus-churn case).
    // No inserts/removes/moves are possible, so skip the key HashSet/HashMap and
    // emit only per-row role changes. `Row::diff` for Arc rows is a pointer
    // compare, so unchanged rows cost nothing and an unchanged list yields no edits.
    if before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .all(|(old, new)| old.key() == new.key())
    {
        let mut edits = Vec::new();
        for (index, (new, old)) in after.iter().zip(before).enumerate() {
            let roles = new.diff(old);
            if roles != 0 {
                edits.push(ModelDelta::Change {
                    index,
                    row: new.clone(),
                    roles,
                });
            }
        }
        return coalesce(edits);
    }
    coalesce(full_diff(before, after))
}

/// General insert/remove/move/change diff. Used when membership or order changed.
fn full_diff<T: Row>(before: &[T], after: &[T]) -> Vec<ModelDelta<T>> {
    let wanted: HashSet<_> = after.iter().map(Row::key).collect();
    assert_eq!(wanted.len(), after.len(), "duplicate row key");
    let mut keys: Vec<_> = before.iter().map(Row::key).collect();
    let mut edits = Vec::new();
    for index in (0..keys.len()).rev() {
        if !wanted.contains(keys[index]) {
            keys.remove(index);
            edits.push(ModelDelta::Remove { index });
        }
    }
    let old: std::collections::HashMap<_, _> = before.iter().map(|r| (r.key(), r)).collect();
    for (index, row) in after.iter().enumerate() {
        if let Some(from) = keys[index..]
            .iter()
            .position(|key| *key == row.key())
            .map(|i| i + index)
        {
            if from != index {
                let key = keys.remove(from);
                keys.insert(index, key);
                edits.push(ModelDelta::Move { from, to: index });
            }
            let roles = row.diff(old[row.key()]);
            if roles != 0 {
                edits.push(ModelDelta::Change {
                    index,
                    row: row.clone(),
                    roles,
                });
            }
        } else {
            keys.insert(index, row.key());
            edits.push(ModelDelta::Insert {
                index,
                row: row.clone(),
            });
        }
    }
    edits
}

/// One Qt notification for adjacent Change edits with the same role mask. The
/// coalescing decision is made here, never by a main-thread row diff.
fn coalesce<T: Row>(edits: Vec<ModelDelta<T>>) -> Vec<ModelDelta<T>> {
    let mut compact = Vec::new();
    for edit in edits {
        if let ModelDelta::Change { index, row, roles } = edit {
            match compact.last_mut() {
                Some(ModelDelta::ChangeRange {
                    index: start,
                    rows,
                    roles: previous,
                }) if *start + rows.len() == index && *previous == roles => {
                    rows.push(row);
                    continue;
                }
                Some(ModelDelta::Change {
                    index: start,
                    roles: previous,
                    ..
                }) if *start + 1 == index && *previous == roles => {
                    let ModelDelta::Change {
                        index: start,
                        row: first,
                        ..
                    } = compact.pop().unwrap()
                    else {
                        unreachable!()
                    };
                    compact.push(ModelDelta::ChangeRange {
                        index: start,
                        rows: vec![first, row],
                        roles,
                    });
                    continue;
                }
                _ => {}
            }
            compact.push(ModelDelta::Change { index, row, roles });
        } else {
            compact.push(edit);
        }
    }
    compact
}

macro_rules! row {
    ($ty:ty, $key:ident, $($field:ident),+ $(,)?) => {
        impl Row for $ty {
            fn key(&self) -> &str { &self.$key }
            fn diff(&self, old: &Self) -> RoleMask {
                let mut mask = 0; let mut bit = 1u64;
                $(if self.$field != old.$field { mask |= bit; } bit <<= 1;)+
                let _ = bit;
                mask
            }
        }
    }
}
use inir_types::desktop::*;
row!(
    NiriWindow,
    id,
    id,
    title,
    app_id,
    pid,
    workspace_id,
    focused,
    floating,
    urgent,
    focus_serial,
    column,
    row,
    tile_width,
    tile_height,
    window_width,
    window_height,
    tile_pos_x,
    tile_pos_y,
    tile_pos_valid,
    window_offset_x,
    window_offset_y,
    focus_timestamp,
    title_present,
    app_id_present
);
row!(
    NiriWorkspace,
    id,
    id,
    index,
    name,
    output,
    active,
    focused,
    active_window_id,
    urgent,
    name_present
);
row!(
    AccessPoint,
    id,
    id,
    device,
    ssid,
    bssid,
    strength,
    frequency,
    rate,
    security,
    active
);
row!(Backlight, id, id, kind, raw, maximum, value);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_role_updates_are_compacted_off_thread() {
        let before: Vec<_> = (0..100)
            .map(|id| NiriWindow {
                id: id.to_string(),
                ..Default::default()
            })
            .collect();
        let mut after = before.clone();
        for row in &mut after {
            row.title = "changed".into();
        }
        let edits = diff(&before, &after);
        assert!(matches!(edits.as_slice(),
            [ModelDelta::ChangeRange { index: 0, rows, roles: 2 }] if rows.len() == 100));
        // A different mask breaks a range; notifications must remain narrow.
        after[50].urgent = true;
        let edits = diff(&before, &after);
        assert!(matches!(edits.as_slice(), [
            ModelDelta::ChangeRange { index: 0, rows: first, roles: 2 },
            ModelDelta::Change { index: 50, roles: 130, .. },
            ModelDelta::ChangeRange { index: 51, rows: last, roles: 2 },
        ] if first.len() == 50 && last.len() == 49));
    }
    #[test]
    fn same_order_fast_path_emits_only_changed_rows() {
        let before: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|id| NiriWindow {
                id: (*id).into(),
                ..Default::default()
            })
            .collect();
        // Unchanged list: no edits (the empty-apply skip relies on this).
        assert!(diff(&before, &before).is_empty());
        // One row changes in place: a single narrow Change, no moves.
        let mut after = before.clone();
        after[1].title = "x".into();
        assert!(matches!(
            diff(&before, &after).as_slice(),
            [ModelDelta::Change { index: 1, roles: 2, .. }]
        ));
    }
    #[test]
    fn edits_preserve_rows_and_reorder() {
        let a = NiriWindow {
            id: "a".into(),
            ..Default::default()
        };
        let b = NiriWindow {
            id: "b".into(),
            ..Default::default()
        };
        let c = NiriWindow {
            id: "c".into(),
            ..Default::default()
        };
        let mut changed = b.clone();
        changed.title = "new".into();
        let mut rows = vec![a.clone(), b.clone()];
        let next = vec![changed, c, a];
        for edit in diff(&rows, &next) {
            match edit {
                ModelDelta::Insert { index, row } => rows.insert(index, row),
                ModelDelta::Remove { index } => {
                    rows.remove(index);
                }
                ModelDelta::Move { from, to } => {
                    let row = rows.remove(from);
                    rows.insert(to, row);
                }
                ModelDelta::Change { index, row, roles } => {
                    assert_eq!(roles, 2);
                    rows[index] = row;
                }
                ModelDelta::ChangeRange {
                    index,
                    rows: changed,
                    ..
                } => {
                    for (offset, row) in changed.into_iter().enumerate() {
                        rows[index + offset] = row;
                    }
                }
            }
        }
        assert_eq!(rows, next);
        assert!(diff(&rows, &next).is_empty());
        assert!(matches!(
            diff(&rows, &[])[0],
            ModelDelta::Remove { index: 2 }
        ));
    }
}
