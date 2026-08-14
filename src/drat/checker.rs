//! The forward DRAT checker.
//!
//! Step 5 of the build order in `docs/TDD.md` part 3: the reader is wired up
//! and nothing is checked yet. Every addition is refused, so this file has no
//! route to `Verified` at all — which is the only property it has to have
//! while it stands. The store and the propagation engine land next.

use std::io::BufRead;

use crate::checker::Stats;
use crate::cnf::Cnf;
use crate::drat::{DratReader, DratStep};
use crate::limits::Limits;
use crate::parse::ParseErrorKind;
use crate::verdict::{Reason, Rejection, Unsupported, Verdict};

/// Checks a DRAT proof against a formula.
pub(crate) fn check_with_stats<R: BufRead>(
    _cnf: &Cnf,
    proof: DratReader<R>,
    _limits: &Limits,
) -> (Verdict, Stats) {
    let mut stats = Stats::default();
    for step in proof {
        let step = match step {
            Ok(step) => step,
            Err(err) => {
                if matches!(err.kind, ParseErrorKind::BinaryProof) {
                    return (
                        Verdict::Unsupported(Unsupported::BinaryProof { line: err.line }),
                        stats,
                    );
                }
                return (
                    Verdict::NotVerified(Rejection {
                        step: None,
                        line: 0,
                        resolvent: None,
                        reason: Reason::Parse(err),
                    }),
                    stats,
                );
            }
        };
        match step {
            DratStep::Delete { .. } => {
                stats.deletions = stats.deletions.saturating_add(1);
            }
            DratStep::Add { .. } => {
                stats.additions = stats.additions.saturating_add(1);
            }
        }
    }
    (
        Verdict::NotVerified(Rejection {
            step: None,
            line: 0,
            resolvent: None,
            reason: Reason::NoEmptyClause,
        }),
        stats,
    )
}
