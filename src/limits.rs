//! Allocation guards for untrusted input.
//!
//! Both input files are attacker-controlled from milestone 4 onward and should
//! be assumed so now. A single literal of `2000000000` in a clause would
//! otherwise size the assignment vector to two gigabytes, so the parser refuses
//! literals beyond `max_var` instead of resizing to meet them.
//!
//! One field here is not a guard. `max_dead_arena_lits` decides when the DRAT
//! store reclaims what it is holding, so no input can cross it and nothing is
//! ever rejected for it. It lives here because it is the other knob that
//! decides how much memory a run costs, and a reader looking for that will
//! look in this file.

/// Default variable ceiling: 2^26.
///
/// On the LRAT path a variable costs about a byte, so this is the 64 MB
/// assignment vector it has always been described as. On the DRAT path it is
/// **not**: that store also carries `watches` and `occ`, two `Vec<Vec<u32>>`
/// with a slot per literal, and an empty slot is 24 bytes whether anything is
/// ever pushed into it or not. Measured peak working set, same 21-byte formula
/// and a proof line naming one large variable: 20.5 MB on the LRAT path
/// against 1,556.5 MB on the DRAT path at 2^24, scaling linearly at about 96
/// bytes per variable. At 2^26 that is roughly 6 GB bought with a fourteen-byte
/// line, into Rust's abort-on-allocation-failure path. Hence the second
/// ceiling below rather than a correction to the comment alone.
pub const DEFAULT_MAX_VAR: u32 = 1 << 26;

/// Default variable ceiling on the **DRAT** path: 2^22, about 400 MB.
///
/// Derived from the 96 bytes per variable measured above. Applied together
/// with [`DEFAULT_MAX_VAR`] and not instead of it, so lowering `max_var` still
/// tightens both paths. It sits far above anything a real proof reaches: the
/// largest instance in this project's corpus, the A217058 a(4) rung, declares
/// 1,209 variables, and the pigeonhole 8x7 refutation declares 56.
pub const DEFAULT_MAX_DRAT_VAR: u32 = 1 << 22;

/// Default ceiling on the length of one clause, hint list or deletion list.
pub const DEFAULT_MAX_CLAUSE_LEN: usize = 1 << 24;

/// Default ceiling on the bytes of one proof line: 2^24, 16 MB.
///
/// The longest line in any proof measured for this project is 234 bytes, on
/// the pigeonhole 8x7 refutation; the longest in the committed corpus is 204.
/// The default is some seventy thousand times that, so it is a guard against a
/// file with no line breaks in it rather than a limit a producer can meet.
pub const DEFAULT_MAX_LINE_BYTES: usize = 1 << 24;

/// Default floor under the arena compaction trigger: 1,024 literals.
///
/// Compaction runs when the literals of dead clauses exceed both the literals
/// of live ones and this floor, so the floor is what stops a proof that
/// deletes its second clause from copying a four-literal arena. It is a floor
/// and not a ratio because the ratio is already the other half of the test.
///
/// 1,024 literals is 4 KB, which is below anything the budget in
/// `docs/TDD.md` part 4 is stated in and above every fixture in the corpus
/// except the real certificate. Setting it to zero compacts on every deletion
/// that leaves any dead literal at all, which is what the fuzz harness does:
/// random proofs are small and delete little, so without it almost none of the
/// ten thousand cases would enter the code the milestone added.
pub const DEFAULT_MAX_DEAD_ARENA_LITS: usize = 1024;

/// Ceilings applied while parsing. Exceeding one is a parse error, never a
/// resize and never a truncation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest permitted variable index.
    pub max_var: u32,
    /// Largest permitted variable index on the DRAT path, where a variable
    /// costs about ninety-six bytes rather than one. The effective ceiling
    /// there is the smaller of this and [`Limits::max_var`]; the LRAT path
    /// ignores it.
    pub max_drat_var: u32,
    /// Largest permitted number of entries in one clause or list.
    pub max_clause_len: usize,
    /// Largest permitted number of bytes in one line of the **proof**, not
    /// counting the newline that ends it.
    ///
    /// The proof reader is the one that promises to stream: it yields a step
    /// at a time and the checker keeps only the live clause database, so the
    /// line it is decoding is the whole of its per-file memory. Without this
    /// ceiling that promise is untrue, and measurably — a 200 MB proof with no
    /// line breaks peaked at 268.6 MB of working set before `max_clause_len`
    /// could apply, because the list is bounded as it is scanned and the line
    /// is buffered before any of it is scanned.
    ///
    /// It is deliberately not applied to the formula, whose parser holds every
    /// clause in memory by design: a line bound there caps a fraction of an
    /// allocation the formula's own size already decides.
    pub max_line_bytes: usize,
    /// Compact the DRAT clause arena once the literals of dead clauses exceed
    /// both the literals of live ones and this floor.
    ///
    /// Not a ceiling like the four above: nothing is rejected for crossing it,
    /// and no input can be made to fail by choosing it badly. It decides when
    /// the store reclaims rather than what it will accept, which is why it is
    /// the one field here that a proof cannot reach. The LRAT path ignores it;
    /// its database is a map whose deletions really delete.
    ///
    /// Zero compacts on every deletion that leaves a dead literal behind. That
    /// is the setting the fuzz harness uses and it is not a sensible default:
    /// it makes the arena copy O(live) per deletion.
    pub max_dead_arena_lits: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_var: DEFAULT_MAX_VAR,
            max_drat_var: DEFAULT_MAX_DRAT_VAR,
            max_clause_len: DEFAULT_MAX_CLAUSE_LEN,
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            max_dead_arena_lits: DEFAULT_MAX_DEAD_ARENA_LITS,
        }
    }
}
