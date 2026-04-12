//! rust-analyzer-backed workspace loading for semantic codemod rules.

use anyhow::{Context, Result};
use ra_ap_hir::EditionedFileId;
use ra_ap_ide::{FileId, RootDatabase};
use ra_ap_ide_db::base_db::SourceDatabase;
use ra_ap_load_cargo::{load_workspace_at, LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_paths::AbsPathBuf;
use ra_ap_project_model::{CargoConfig, RustLibSource};
use ra_ap_syntax::ast;
use ra_ap_vfs::{Vfs, VfsPath};
use std::path::{Path, PathBuf};

/// A loaded upstream workspace ready for semantic analysis.
pub struct SemanticWorkspace {
    pub root: PathBuf,
    pub db: RootDatabase,
    pub vfs: Vfs,
}

impl SemanticWorkspace {
    pub fn load(codex_rs: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(codex_rs)
            .with_context(|| format!("canonicalizing {}", codex_rs.display()))?;

        let mut cargo_config = CargoConfig::default();
        cargo_config.no_deps = false;
        cargo_config.sysroot = Some(RustLibSource::Discover);

        let load_config = LoadCargoConfig {
            load_out_dirs_from_check: false,
            with_proc_macro_server: ProcMacroServerChoice::None,
            prefill_caches: false,
            num_worker_threads: 1,
            proc_macro_processes: 0,
        };

        let root_abs = AbsPathBuf::assert_utf8(root.clone());
        let (db, vfs, _proc_macro_server) =
            load_workspace_at(root_abs.as_ref(), &cargo_config, &load_config, &|_| {})
                .with_context(|| format!("loading semantic workspace at {}", root.display()))?;

        Ok(Self { root, db, vfs })
    }

    pub fn local_rust_files(&self) -> Vec<(FileId, PathBuf)> {
        let mut files = self
            .vfs
            .iter()
            .filter_map(|(file_id, path)| {
                let abs = path.as_path()?;
                let std_path = std::path::PathBuf::from(abs.to_path_buf());
                if !std_path.starts_with(&self.root) {
                    return None;
                }
                if std_path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                    return None;
                }
                Some((file_id, std_path))
            })
            .collect::<Vec<_>>();
        files.sort_by(|a, b| a.1.cmp(&b.1));
        files
    }

    pub fn file_id_for_path(&self, path: &Path) -> Option<FileId> {
        let vfs_path = VfsPath::from(AbsPathBuf::assert_utf8(path.to_path_buf()));
        self.vfs.file_id(&vfs_path).map(|(file_id, _)| file_id)
    }

    pub fn path_for_file_id(&self, file_id: FileId) -> Option<PathBuf> {
        self.vfs
            .file_path(file_id)
            .as_path()
            .map(|abs| std::path::PathBuf::from(abs.to_path_buf()))
    }

    pub fn file_text(&self, file_id: FileId) -> String {
        self.db.file_text(file_id).text(&self.db).to_string()
    }

    pub fn parse_file(&self, file_id: FileId) -> ast::SourceFile {
        EditionedFileId::current_edition(&self.db, file_id)
            .parse(&self.db)
            .tree()
    }
}
