//! Allocation guards for untrusted input.
//!
//! Both input files are attacker-controlled from milestone 4 onward and should
//! be assumed so now. A single literal of `2000000000` in a clause would
//! otherwise size the assignment vector to two gigabytes, so the parser refuses
//! literals beyond `max_var` instead of resizing to meet them.

/// Default variable ceiling: 2^26, a 64 MB assignment vector at one byte each.
pub const DEFAULT_MAX_VAR: u32 = 1 << 26;

/// Default ceiling on the length of one clause, hint list or deletion list.
pub const DEFAULT_MAX_CLAUSE_LEN: usize = 1 << 24;

/// Ceilings applied while parsing. Exceeding one is a parse error, never a
/// resize and never a truncation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest permitted variable index.
    pub max_var: u32,
    /// Largest permitted number of entries in one clause or list.
    pub max_clause_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_var: DEFAULT_MAX_VAR,
            max_clause_len: DEFAULT_MAX_CLAUSE_LEN,
        }
    }
}
