//! Scoped diagnostic production budget. Never reconstruct validity from retained length.
use std::{cell::RefCell, rc::Rc};

use crate::{Finding, Severity};

/// Ordinary validation and CLI diagnostic ceiling; raising is not exposed.
pub const DEFAULT_FINDINGS_LIMIT: usize = 1000;

#[derive(Debug)]
struct Budget {
    limit: usize,
    produced: usize,
    incomplete: bool,
    had_error: bool,
}

#[derive(Debug)]
pub(crate) struct Findings {
    budget: Rc<RefCell<Budget>>,
    pub(crate) execution: crate::ExecutionBudget,
    retained: Vec<Finding>,
    admissions: Vec<usize>,
    errors: usize,
}

impl Findings {
    pub fn new(limit: usize) -> Self {
        Self::with_execution(limit, crate::ExecutionBudget::default())
    }
    pub fn with_execution(limit: usize, execution: crate::ExecutionBudget) -> Self {
        Self {
            execution,
            budget: Rc::new(RefCell::new(Budget {
                limit: limit.clamp(1, DEFAULT_FINDINGS_LIMIT),
                produced: 0,
                incomplete: false,
                had_error: false,
            })),
            retained: Vec::new(),
            admissions: Vec::new(),
            errors: 0,
        }
    }

    /// Isolated transcript, sharing the enclosing run's non-refundable allowance.
    pub fn fork(&self) -> Self {
        Self {
            budget: Rc::clone(&self.budget),
            execution: self.execution.clone(),
            retained: Vec::new(),
            admissions: Vec::new(),
            errors: 0,
        }
    }

    pub fn cancelled(&self) -> bool {
        self.execution.checkpoint().is_err() || self.budget.borrow().incomplete
    }
    pub fn had_error(&self) -> bool {
        self.errors != 0
    }
    pub fn as_slice(&self) -> &[Finding] {
        &self.retained
    }
    pub fn is_empty(&self) -> bool {
        self.retained.is_empty()
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.retained.len()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, Finding> {
        self.retained.iter()
    }
    pub fn sort_and_dedup(&mut self) {
        // Deduplicate in admission order; presentation sorting belongs at export.
        // Keep the first admitted copy and never refund the production allowance.
        let mut i = 0;
        while i < self.retained.len() {
            let duplicate = self.retained[..i].iter().any(|a| {
                let b = &self.retained[i];
                a.error_code == b.error_code
                    && a.json_pointer == b.json_pointer
                    && a.message == b.message
            });
            if duplicate {
                self.retained.remove(i);
                self.admissions.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Severity is supplied before conversion so even an unretained error is sticky.
    /// The first excess diagnostic is a bounded probe; its message is never built.
    pub fn emit(&mut self, severity: Severity, make: impl FnOnce() -> Finding) {
        if severity == Severity::Error {
            self.errors = self.errors.saturating_add(1);
        }
        if self.execution.checkpoint().is_err() {
            return;
        }
        let mut budget = self.budget.borrow_mut();
        budget.had_error |= severity == Severity::Error;
        if budget.incomplete {
            return;
        }
        if budget.produced == budget.limit {
            budget.incomplete = true;
            return;
        }
        budget.produced += 1;
        let admission = budget.produced;
        drop(budget);
        let finding = make();
        assert_eq!(
            finding.severity, severity,
            "producer severity must match budget admission"
        );
        self.retained.push(finding);
        self.admissions.push(admission);
    }
    pub fn push(&mut self, make: impl FnOnce() -> Finding) {
        self.emit(Severity::Error, make);
    }

    /// Consume at most the remaining allowance plus the single overflow probe.
    pub fn collect_errors<T>(
        &mut self,
        mut errors: impl Iterator<Item = T>,
        mut make: impl FnMut(T) -> Finding,
    ) {
        while !self.cancelled() {
            let Some(error) = errors.next() else {
                break;
            };
            self.push(|| make(error));
        }
    }

    /// Move already charged findings, without granting or debiting a fresh budget.
    pub fn absorb(&mut self, other: Self) {
        assert!(Rc::ptr_eq(&self.budget, &other.budget));
        self.errors = self.errors.saturating_add(other.errors);
        self.retained.extend(other.retained);
        self.admissions.extend(other.admissions);
    }

    pub fn finish(self) -> (Vec<Finding>, bool, bool) {
        let mut output = self.into_output();
        output.finalize_marker();
        (
            output.retained,
            output.had_error,
            !output.incomplete && output.stop.is_none(),
        )
    }

    pub fn into_output(self) -> FindingOutput {
        let stop = self.execution.checkpoint().err();
        let budget = self.budget.borrow();
        FindingOutput {
            retained: self.retained,
            admissions: self.admissions,
            stop,
            limit: budget.limit,
            produced: budget.produced,
            incomplete: budget.incomplete,
            had_error: budget.had_error,
        }
    }
}

/// A finalized validation transcript with its original, non-refundable allowance.
/// Diagnostic metadata stays private; callers cannot replenish it from displayed length.
#[derive(Debug)]
pub struct FindingOutput {
    stop: Option<crate::ExecutionStop>,
    retained: Vec<Finding>,
    admissions: Vec<usize>,
    limit: usize,
    produced: usize,
    incomplete: bool,
    had_error: bool,
}

impl FindingOutput {
    pub(crate) fn sort(&mut self) {
        let mut paired: Vec<_> = self
            .retained
            .drain(..)
            .zip(self.admissions.drain(..))
            .collect();
        paired.sort_by(|(a, _), (b, _)| {
            (
                a.severity,
                a.source_file.as_ref(),
                a.json_pointer.as_str(),
                a.error_code,
            )
                .cmp(&(
                    b.severity,
                    b.source_file.as_ref(),
                    b.json_pointer.as_str(),
                    b.error_code,
                ))
        });
        (self.retained, self.admissions) = paired.into_iter().unzip();
    }
    fn finalize_marker(&mut self) {
        if let Some(stop) = self.stop {
            self.retained = vec![stop.finding()];
            self.admissions = vec![usize::MAX];
            self.had_error = true;
            return;
        }
        if !self.incomplete
            || self
                .retained
                .iter()
                .any(|f| f.error_code == "RP_W_FINDINGS_TRUNCATED")
        {
            return;
        }
        if self.retained.len() == self.limit {
            let position = self
                .admissions
                .iter()
                .enumerate()
                .max_by_key(|(_, n)| *n)
                .unwrap()
                .0;
            self.retained.remove(position);
            self.admissions.remove(position);
        }
        self.retained.push(Finding::new(
            "RP_W_FINDINGS_TRUNCATED",
            "finding_truncation",
            Severity::Warning,
            "findings production limit exceeded; validation is incomplete",
            None,
            "",
        ));
        self.admissions.push(usize::MAX);
    }
    /// Append a command-local error using the enclosing validation allowance.
    pub fn push_error(&mut self, make: impl FnOnce() -> Finding) {
        self.had_error = true;
        if self.incomplete || self.stop.is_some() {
            return;
        }
        if self.produced == self.limit {
            self.incomplete = true;
            return;
        }
        self.produced += 1;
        let finding = make();
        assert_eq!(finding.severity, Severity::Error);
        self.retained.push(finding);
        self.admissions.push(self.produced);
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stop.is_none() && !self.incomplete && !self.had_error && self.retained.is_empty()
    }
    #[must_use]
    pub fn into_findings(mut self) -> Vec<Finding> {
        self.finalize_marker();
        self.sort();
        self.retained
    }
    pub(crate) fn report_parts(mut self) -> (Vec<Finding>, Self, bool, bool) {
        self.finalize_marker();
        self.sort();
        let findings = std::mem::take(&mut self.retained);
        let error = self.had_error;
        let complete = !self.incomplete && self.stop.is_none();
        (findings, self, error, complete)
    }
    pub(crate) fn restore(mut self, findings: Vec<Finding>) -> Self {
        assert_eq!(findings.len(), self.admissions.len());
        self.retained = findings;
        self
    }
}

impl Default for Findings {
    fn default() -> Self {
        Self::new(DEFAULT_FINDINGS_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn finding(severity: Severity) -> Finding {
        Finding::new("test", "test", severity, "test", None, "")
    }
    #[test]
    fn exact_boundary_is_unchanged_and_overflow_is_lazy_and_sticky() {
        let mut exact = Findings::new(2);
        exact.emit(Severity::Warning, || finding(Severity::Warning));
        exact.push(|| finding(Severity::Error));
        let (retained, error, complete) = exact.finish();
        assert_eq!(
            retained,
            vec![finding(Severity::Warning), finding(Severity::Error)]
        );
        assert!(error && complete);
        let mut over = Findings::new(2);
        over.emit(Severity::Warning, || finding(Severity::Warning));
        over.push(|| finding(Severity::Error));
        over.emit(Severity::Warning, || panic!("overflow conversion"));
        over.push(|| panic!("cancelled conversion"));
        let (retained, error, complete) = over.finish();
        assert_eq!(retained.len(), 2);
        assert_eq!(retained[1].error_code, "RP_W_FINDINGS_TRUNCATED");
        assert!(error && !complete);
    }
    #[test]
    fn fork_presentation_sort_must_not_change_later_overflow_eviction() {
        let mut root = Findings::new(2);
        let mut child = root.fork();
        for pointer in ["/z", "/a"] {
            child.push(|| Finding::new("test", "test", Severity::Error, "test", None, pointer));
        }
        child.sort_and_dedup();
        root.absorb(child);
        root.push(|| panic!("overflow conversion"));
        let (retained, error, complete) = root.finish();
        assert_eq!(retained[0].json_pointer, "/z");
        assert_eq!(retained[1].error_code, "RP_W_FINDINGS_TRUNCATED");
        assert!(error && !complete);
    }

    #[test]
    fn command_append_preserves_admission_and_consumed_discarded_or_deduplicated_allowance() {
        let mut root = Findings::new(2);
        for pointer in ["/z", "/a"] {
            root.push(|| Finding::new("test", "test", Severity::Error, "test", None, pointer));
        }
        let (display, state, _, _) = root.into_output().report_parts();
        assert_eq!(display[0].json_pointer, "/a");
        let mut command = state.restore(display);
        command.push_error(|| panic!("command overflow conversion"));
        let output = command.into_findings();
        assert_eq!(output[0].json_pointer, "/z");
        assert_eq!(output[1].error_code, "RP_W_FINDINGS_TRUNCATED");
        for discard in [false, true] {
            let mut root = Findings::new(2);
            let mut child = root.fork();
            child.push(|| finding(Severity::Error));
            child.push(|| finding(Severity::Error));
            child.sort_and_dedup();
            if !discard {
                root.absorb(child);
            }
            let (display, state, error, complete) = root.into_output().report_parts();
            assert!(error && complete);
            assert_eq!(display.len(), usize::from(!discard));
            let mut command = state.restore(display);
            command.push_error(|| panic!("discard/dedup must not refund allowance"));
            assert!(!command.is_empty());
            let output = command.into_findings();
            assert_eq!(output.last().unwrap().error_code, "RP_W_FINDINGS_TRUNCATED");
            assert_eq!(output.len(), 1 + usize::from(!discard));
        }
    }

    #[test]
    fn iterator_and_conversion_cancel_before_unbounded_tail() {
        let visits = std::cell::Cell::new(0);
        let conversions = std::cell::Cell::new(0);
        let mut findings = Findings::new(3);
        findings.collect_errors(
            (0..100000).inspect(|_| visits.set(visits.get() + 1)),
            |_| {
                conversions.set(conversions.get() + 1);
                finding(Severity::Error)
            },
        );
        assert_eq!(visits.get(), 4);
        assert_eq!(conversions.get(), 3);
        findings.collect_errors(
            std::iter::from_fn(|| panic!("later iterator advanced")),
            |_: ()| panic!("later conversion"),
        );
    }

    #[test]
    fn tiny_limits_warning_only_and_nested_scopes_are_safe() {
        for limit in [0, 1] {
            let mut root = Findings::new(limit);
            let mut child = root.fork();
            child.emit(Severity::Warning, || finding(Severity::Warning));
            root.emit(Severity::Warning, || panic!("no fresh nested budget"));
            let (retained, error, complete) = root.finish();
            assert_eq!(retained.len(), 1);
            assert!(!error && !complete);
        }
        assert!(!Findings::default().cancelled());
    }
}
