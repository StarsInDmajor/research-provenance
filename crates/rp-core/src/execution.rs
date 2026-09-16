//! Command-scoped cooperative execution; not a hard wall-clock deadline.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionStop {
    Deadline,
    Interrupted,
}

impl ExecutionStop {
    pub fn finding(self) -> crate::Finding {
        let (code, family, message) = match self {
            Self::Deadline => (
                "RP_E_RESOURCE_DEADLINE_EXCEEDED",
                "resource_limit",
                "cooperative command deadline exceeded",
            ),
            Self::Interrupted => (
                "RP_E_INTERRUPTED",
                "interrupted",
                "command cancelled by caller token",
            ),
        };
        crate::Finding::new(code, family, crate::Severity::Error, message, None, "")
    }
    pub fn status(self) -> crate::Status {
        match self {
            Self::Deadline => crate::Status::Invalid,
            Self::Interrupted => crate::Status::Interrupted,
        }
    }
}
impl std::fmt::Display for ExecutionStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Deadline => "cooperative command deadline exceeded",
            Self::Interrupted => "command cancelled by caller token",
        })
    }
}
impl std::error::Error for ExecutionStop {}

/// One non-refundable command lifetime. Clones share clock, caller token and stop
/// latch. No process globals, background worker, signal handler or runtime is used.
/// Cancellation is observed only at checkpoints; library calls/syscalls already in
/// progress can run beyond the elapsed-time allowance.
#[derive(Clone)]
pub struct ExecutionBudget {
    clock: Arc<dyn Fn() -> Duration + Send + Sync>,
    token: Arc<AtomicBool>,
    limit: Duration,
    stopped: Arc<AtomicU8>,
}
impl std::fmt::Debug for ExecutionBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionBudget").finish_non_exhaustive()
    }
}
impl Default for ExecutionBudget {
    fn default() -> Self {
        Self::new(Duration::from_secs(60), Arc::new(AtomicBool::new(false)))
    }
}
impl ExecutionBudget {
    /// Production monotonic clock, starting now. Duration is clamped to 1ms..=600s.
    /// The caller requests cancellation by storing true into its token.
    pub fn new(limit: Duration, token: Arc<AtomicBool>) -> Self {
        let start = Instant::now();
        Self::with_clock(limit, token, move || start.elapsed())
    }
    /// Inject elapsed time since command start (not wall time). Intended for
    /// deterministic tests/embedders; the caller must provide a monotonic clock.
    pub fn with_clock(
        limit: Duration,
        token: Arc<AtomicBool>,
        clock: impl Fn() -> Duration + Send + Sync + 'static,
    ) -> Self {
        Self {
            clock: Arc::new(clock),
            token,
            limit: limit.clamp(Duration::from_millis(1), Duration::from_secs(600)),
            stopped: Arc::new(AtomicU8::new(0)),
        }
    }
    pub(crate) fn read_chunk(
        &self,
        reader: &mut impl std::io::Read,
        bytes: &mut [u8],
    ) -> Result<usize, ExecutionReadError> {
        self.checkpoint().map_err(ExecutionReadError::Stopped)?;
        let result = reader.read(bytes);
        self.checkpoint().map_err(ExecutionReadError::Stopped)?;
        result.map_err(ExecutionReadError::Io)
    }
    pub fn checkpoint(&self) -> Result<(), ExecutionStop> {
        let latched = self.stopped.load(Ordering::Acquire);
        if latched != 0 {
            return Err(if latched == 1 {
                ExecutionStop::Interrupted
            } else {
                ExecutionStop::Deadline
            });
        }
        let expired = (self.clock)() >= self.limit;
        let stop = if self.token.load(Ordering::Acquire) {
            1
        } else if expired {
            2
        } else {
            0
        };
        if stop == 0 {
            return Ok(());
        }
        let latched = self
            .stopped
            .compare_exchange(0, stop, Ordering::AcqRel, Ordering::Acquire)
            .unwrap_or_else(|prior| prior);
        Err(if latched == 1 || (latched == 0 && stop == 1) {
            ExecutionStop::Interrupted
        } else {
            ExecutionStop::Deadline
        })
    }
}

#[derive(Debug)]
pub(crate) enum ExecutionReadError {
    Stopped(ExecutionStop),
    Io(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    #[test]
    fn exact_expiry_sticky_and_shared_nested_state() {
        let now = Arc::new(AtomicU64::new(0));
        let token = Arc::new(AtomicBool::new(false));
        let budget = ExecutionBudget::with_clock(Duration::from_secs(2), token.clone(), {
            let now = now.clone();
            move || Duration::from_millis(now.load(Ordering::Relaxed))
        });
        let nested = budget.clone();
        now.store(1999, Ordering::Relaxed);
        assert_eq!(nested.checkpoint(), Ok(()));
        now.store(2000, Ordering::Relaxed);
        assert_eq!(nested.checkpoint(), Err(ExecutionStop::Deadline));
        now.store(0, Ordering::Relaxed);
        token.store(true, Ordering::Release);
        assert_eq!(budget.checkpoint(), Err(ExecutionStop::Deadline));
    }
    #[test]
    fn simultaneous_cancellation_wins_and_is_sticky() {
        let token = Arc::new(AtomicBool::new(true));
        let budget = ExecutionBudget::with_clock(Duration::from_secs(1), token.clone(), || {
            Duration::from_secs(1)
        });
        assert_eq!(budget.checkpoint(), Err(ExecutionStop::Interrupted));
        token.store(false, Ordering::Release);
        assert_eq!(budget.checkpoint(), Err(ExecutionStop::Interrupted));
        assert_eq!(ExecutionBudget::default().checkpoint(), Ok(()));
    }
    #[test]
    fn read_chunk_stops_after_inflight_read_and_before_next_read() {
        struct Reader {
            token: Arc<AtomicBool>,
            calls: usize,
        }
        impl std::io::Read for Reader {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                self.calls += 1;
                self.token.store(true, Ordering::Release);
                b[0] = 1;
                Ok(1)
            }
        }
        let token = Arc::new(AtomicBool::new(false));
        let budget = ExecutionBudget::new(Duration::from_secs(60), token.clone());
        let mut reader = Reader { token, calls: 0 };
        for _ in 0..2 {
            assert!(matches!(
                budget.read_chunk(&mut reader, &mut [0; 10]),
                Err(ExecutionReadError::Stopped(ExecutionStop::Interrupted))
            ));
        }
        assert_eq!(reader.calls, 1);
    }
    #[test]
    fn schema_boolean_boundary_discards_success_after_expiry() {
        let bundle = crate::SchemaBundle::new().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let budget = ExecutionBudget::with_clock(
            Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
            {
                let calls = calls.clone();
                move || {
                    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        Duration::ZERO
                    } else {
                        Duration::from_secs(1)
                    }
                }
            },
        );
        let value = serde_json::to_value(crate::CommandResult::new(
            "validate",
            crate::Status::Ok,
            None,
            vec![],
            None,
        ))
        .unwrap();
        assert!(!bundle.is_valid_with_budget(&value, &budget));
        assert_eq!(budget.checkpoint(), Err(ExecutionStop::Deadline));
    }
    #[test]
    fn concurrent_commands_do_not_share_latches() {
        let workers: Vec<_> = (0..8)
            .map(|i| {
                std::thread::spawn(move || {
                    let budget = ExecutionBudget::new(
                        Duration::from_secs(60),
                        Arc::new(AtomicBool::new(i % 2 == 0)),
                    );
                    let clone = budget.clone();
                    assert_eq!(
                        clone.checkpoint(),
                        if i % 2 == 0 {
                            Err(ExecutionStop::Interrupted)
                        } else {
                            Ok(())
                        }
                    );
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }
    #[test]
    fn caller_duration_is_positive_and_bounded() {
        let budget =
            ExecutionBudget::with_clock(Duration::MAX, Arc::new(AtomicBool::new(false)), || {
                Duration::from_secs(600)
            });
        assert_eq!(budget.checkpoint(), Err(ExecutionStop::Deadline));
        let budget =
            ExecutionBudget::with_clock(Duration::ZERO, Arc::new(AtomicBool::new(false)), || {
                Duration::ZERO
            });
        assert_eq!(budget.checkpoint(), Ok(()));
    }
}
