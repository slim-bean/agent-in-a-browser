#![allow(unused)]
//! Stub replacement for `apply-patch/src/invocation.rs` that removes the
//! tree-sitter dependency. All parsing functions report "not an apply_patch
//! invocation" so the WASM build compiles without a C toolchain.

use std::path::Path;

use crate::ApplyPatchArgs;
use crate::MaybeApplyPatchVerified;
use crate::parser::ParseError;

/// Outcome of a *lenient* attempt to recognise an `apply_patch` invocation.
#[derive(Debug, PartialEq)]
pub enum MaybeApplyPatch {
    Body(ApplyPatchArgs),
    ShellParseError(ExtractHeredocError),
    PatchParseError(ParseError),
    NotApplyPatch,
}

/// Errors that can occur when extracting the heredoc body from a shell script.
#[derive(Debug, PartialEq)]
pub enum ExtractHeredocError {
    CommandDidNotStartWithApplyPatch,
    FailedToLoadBashGrammar,
    HeredocNotUtf8,
    FailedToParsePatchIntoAst,
    FailedToFindHeredocBody,
}

/// Stub: always returns [`MaybeApplyPatch::NotApplyPatch`].
pub fn maybe_parse_apply_patch(argv: &[String]) -> MaybeApplyPatch {
    MaybeApplyPatch::NotApplyPatch
}

/// Stub: always returns [`MaybeApplyPatchVerified::NotApplyPatch`].
pub fn maybe_parse_apply_patch_verified(argv: &[String], cwd: &Path) -> MaybeApplyPatchVerified {
    MaybeApplyPatchVerified::NotApplyPatch
}
