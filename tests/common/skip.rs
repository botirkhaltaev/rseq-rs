//! Soft skip for host capabilities the test cannot create.

/// Print a skip reason, or panic when `RSEQ_REQUIRE` is set (and not `"0"`).
/// CI sets it so a missing capability fails the job instead of passing quietly.
pub(crate) fn skip(reason: &str) {
    let required = std::env::var_os("RSEQ_REQUIRE").is_some_and(|v| v != "0");
    assert!(!required, "required: {reason}");
    eprintln!("skip: {reason}");
}
