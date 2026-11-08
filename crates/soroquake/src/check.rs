//! Invariant results.

/// The outcome of evaluating one invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The invariant held.
    Pass,
    /// The invariant was broken, with a human-readable reason.
    Fail(String),
}

impl Check {
    pub fn is_pass(&self) -> bool {
        matches!(self, Check::Pass)
    }
}

/// Build a [`Check`] from a condition and an optional formatted message.
///
/// ```
/// use soroquake::{check, Check};
/// let assets = 10;
/// let owed = 12;
/// let c = check!(assets >= owed, "insolvent: {assets} < {owed}");
/// assert_eq!(c, Check::Fail("insolvent: 10 < 12".into()));
/// ```
#[macro_export]
macro_rules! check {
    ($cond:expr) => {
        if $cond {
            $crate::Check::Pass
        } else {
            $crate::Check::Fail(stringify!($cond).to_string())
        }
    };
    ($cond:expr, $($msg:tt)+) => {
        if $cond {
            $crate::Check::Pass
        } else {
            $crate::Check::Fail(format!($($msg)+))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_and_fail() {
        assert!(check!(1 + 1 == 2).is_pass());
        assert_eq!(check!(1 > 2), Check::Fail("1 > 2".into()));
    }
}
