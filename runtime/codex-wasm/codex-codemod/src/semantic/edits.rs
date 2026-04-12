//! Source edit primitives for semantic rules.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ra_ap_ide::TextRange;
use ra_ap_syntax::{AstNode, SyntaxNode, TextSize};

#[derive(Debug, Clone)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug)]
pub struct FileEdits {
    pub path: PathBuf,
    pub source: String,
    pub edits: Vec<Edit>,
}

impl FileEdits {
    pub fn new(path: PathBuf, source: String) -> Self {
        Self {
            path,
            source,
            edits: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn add_range(&mut self, range: TextRange, replacement: impl Into<String>) {
        self.edits.push(Edit {
            start: text_size_to_usize(range.start()),
            end: text_size_to_usize(range.end()),
            replacement: replacement.into(),
        });
    }

    pub fn add_raw(&mut self, start: usize, end: usize, replacement: impl Into<String>) {
        self.edits.push(Edit {
            start,
            end,
            replacement: replacement.into(),
        });
    }

    pub fn add_node<N: AstNode>(&mut self, node: &N, replacement: impl Into<String>) {
        self.add_syntax(node.syntax(), replacement);
    }

    pub fn add_syntax(&mut self, node: &SyntaxNode, replacement: impl Into<String>) {
        self.add_range(node.text_range(), replacement);
    }

    pub fn replace_first_exact(&mut self, find: &str, replace: &str) -> ReplaceExactResult {
        if self.source.contains(replace) && !self.source.contains(find) {
            return ReplaceExactResult::AlreadyApplied;
        }
        let Some(start) = self.source.find(find) else {
            return ReplaceExactResult::NotMatched;
        };
        self.edits.push(Edit {
            start,
            end: start + find.len(),
            replacement: replace.to_string(),
        });
        ReplaceExactResult::Applied
    }

    pub fn apply(self) -> Result<Option<String>> {
        if self.edits.is_empty() {
            return Ok(None);
        }

        let mut edits = self.edits;
        edits.sort_by(|a, b| {
            let size_a = a.end.saturating_sub(a.start);
            let size_b = b.end.saturating_sub(b.start);
            size_b.cmp(&size_a).then(b.start.cmp(&a.start))
        });

        let mut accepted: Vec<Edit> = Vec::new();
        for edit in edits {
            let overlaps = accepted
                .iter()
                .any(|other| edit.start < other.end && edit.end > other.start);
            if !overlaps {
                accepted.push(edit);
            }
        }
        accepted.sort_by(|a, b| b.start.cmp(&a.start));

        let mut out = self.source;
        for edit in &accepted {
            let start = edit.start.min(out.len());
            let end = edit.end.min(out.len());
            out.replace_range(start..end, &edit.replacement);
        }
        Ok(Some(out))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceExactResult {
    Applied,
    AlreadyApplied,
    NotMatched,
}

pub fn write_if_changed(path: &Path, original: &str, updated: Option<String>) -> Result<bool> {
    let Some(updated) = updated else {
        return Ok(false);
    };
    if updated == original {
        return Ok(false);
    }
    std::fs::write(path, updated).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

pub fn extend_to_line_start(source: &str, start: usize) -> usize {
    if start == 0 {
        return 0;
    }
    let prefix = &source[..start];
    match prefix.rfind('\n') {
        Some(idx) => idx + 1,
        None => 0,
    }
}

pub fn extend_to_line_end(source: &str, end: usize) -> usize {
    match source[end..].find('\n') {
        Some(idx) => end + idx + 1,
        None => source.len(),
    }
}

pub fn text_size_to_usize(size: TextSize) -> usize {
    u32::from(size) as usize
}
