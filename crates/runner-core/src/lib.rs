//! Typed plan validation and execution primitives.

/// Returns the plan format version implemented by this crate.
#[must_use]
pub const fn plan_format_version() -> &'static str {
    "v1"
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_plan_format_version() {
        assert_eq!(super::plan_format_version(), "v1");
    }
}
