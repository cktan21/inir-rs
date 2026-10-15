//! Generic Rust-owned model. Row diffing and edit selection happen in inir-core.
use cxx_qt_lib::{QString, QStringList, QVariant};
use inir_core::model::{ModelDelta, Row as CoreRow};
use std::sync::Arc;

pub trait Row: CoreRow {
    fn role_names() -> &'static [&'static str];
    fn data(&self, role: i32) -> QVariant;
}
impl<T: Row> Row for Arc<T> {
    fn role_names() -> &'static [&'static str] {
        T::role_names()
    }
    fn data(&self, role: i32) -> QVariant {
        (**self).data(role)
    }
}
#[derive(Default)]
pub struct Model<T> {
    pub rows: Vec<T>,
}
pub struct Pending<S, T, M> {
    pub scheduled: bool,
    pub baseline: Arc<S>,
    pub target: Option<Arc<S>>,
    pub edits: Vec<ModelDelta<T>>,
    pub metadata: Option<M>,
    changed: bool,
}
impl<S: Default, T, M> Default for Pending<S, T, M> {
    fn default() -> Self {
        Self {
            scheduled: false,
            baseline: Arc::new(S::default()),
            target: None,
            edits: Vec::new(),
            metadata: None,
            changed: false,
        }
    }
}
impl<S, T, M> Pending<S, T, M> {
    /// Replace the pending payload relative to the last applied baseline. A
    /// change followed by a revert before Qt drains must cancel the payload,
    /// while leaving the already queued callback in charge of draining it.
    pub fn stage(&mut self, state: Arc<S>, edits: Vec<ModelDelta<T>>, metadata: M, changed: bool) -> bool {
        if !changed && !self.scheduled {
            self.baseline = state;
            return false;
        }
        self.target = Some(state);
        self.edits = edits;
        self.metadata = Some(metadata);
        self.changed = changed;
        let queue = !self.scheduled;
        self.scheduled = true;
        queue
    }

    pub fn drain(&mut self) -> Option<(M, Vec<ModelDelta<T>>)> {
        self.baseline = self.target.take().expect("queued publication has a target");
        self.scheduled = false;
        let metadata = self.metadata.take().expect("queued publication has metadata");
        let edits = std::mem::take(&mut self.edits);
        self.changed.then_some((metadata, edits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_preserves_coalesced_updates_status_and_reverts() {
        let mut p = Pending::<i32, i32, bool>::default();
        assert!(!p.stage(Arc::new(0), vec![], false, false));
        assert!(!p.scheduled);
        assert!(p.stage(Arc::new(1), vec![], true, true)); // status-only update
        assert!(!p.stage(Arc::new(2), vec![], true, true)); // latest status replaces it
        assert_eq!(p.drain().unwrap().0, true);
        assert_eq!(*p.baseline, 2);
        assert!(p.stage(Arc::new(3), vec![], true, true));
        assert!(!p.stage(Arc::new(2), vec![], false, false)); // revert before Qt runs
        assert!(p.drain().is_none());
        assert!(!p.scheduled);
        assert!(p.stage(Arc::new(4), vec![], true, true)); // next callback still works
        assert!(p.drain().is_some());
    }
}
pub fn roles<T: Row>() -> QStringList {
    let mut names = QStringList::default();
    for name in T::role_names() {
        names.append(QString::from(*name));
    }
    names
}
pub fn text(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}
pub fn number(s: &str) -> QVariant {
    QVariant::from(&s.parse::<f64>().unwrap_or(-1.0))
}
