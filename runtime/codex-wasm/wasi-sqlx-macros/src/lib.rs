//! Proc macros for wasi-sqlx: `#[derive(FromRow)]` and `migrate!`.
//!
//! - `FromRow` generates row-to-struct mapping via `row.try_get("field")`
//! - `migrate!` embeds SQL migration files at compile time (like real sqlx)

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

/// Derive `sqlx::FromRow` for a struct with named fields.
///
/// Each field must implement `sqlx::FromSqliteValue`. The generated impl
/// calls `row.try_get("field_name")` for each field.
///
/// # Example
/// ```ignore
/// #[derive(sqlx::FromRow)]
/// struct User {
///     id: i64,
///     name: String,
/// }
/// ```
#[proc_macro_derive(FromRow)]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return syn::Error::new_spanned(
                    &input.ident,
                    "FromRow can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(
                &input.ident,
                "FromRow can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    let field_inits = fields.iter().map(|f| {
        let field_name = f.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        quote! {
            #field_name: row.try_get(#field_name_str)?
        }
    });

    let expanded = quote! {
        impl #impl_generics sqlx::FromRow for #name #ty_generics #where_clause {
            fn from_row(row: &sqlx::SqliteRow) -> Result<Self, sqlx::Error> {
                use sqlx::Row;
                Ok(Self {
                    #(#field_inits,)*
                })
            }
        }
    };

    expanded.into()
}

/// Embed SQL migration files at compile time.
///
/// Reads all `*.sql` files from the given directory (relative to the calling
/// crate's `CARGO_MANIFEST_DIR`), sorts by version number extracted from
/// the filename prefix (e.g., `0001_threads.sql` → version 1), and produces
/// a `sqlx::migrate::Migrator` with all migrations embedded as static data.
///
/// # Usage
/// ```ignore
/// static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
/// ```
#[proc_macro]
pub fn migrate(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as syn::LitStr);
    let dir_path = lit.value();

    // Resolve relative to the calling crate's manifest directory
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let full_path = std::path::Path::new(&manifest_dir).join(&dir_path);

    if !full_path.is_dir() {
        return syn::Error::new(
            lit.span(),
            format!("migration directory not found: {}", full_path.display()),
        )
        .to_compile_error()
        .into();
    }

    // Read and sort migration files
    let mut migrations: Vec<(i64, String, String, String)> = Vec::new(); // (version, description, sql, path)

    let mut entries: Vec<_> = std::fs::read_dir(&full_path)
        .expect("failed to read migration directory")
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.extension().map(|e| e == "sql").unwrap_or(false) {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let version: i64 = name
                .split('_')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            if version > 0 {
                let sql = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
                let path_str = path.to_string_lossy().to_string();
                migrations.push((version, name, sql, path_str));
            }
        }
    }

    migrations.sort_by_key(|(v, _, _, _)| *v);

    let versions: Vec<_> = migrations.iter().map(|(v, _, _, _)| *v).collect();
    let descriptions: Vec<_> = migrations.iter().map(|(_, d, _, _)| d.as_str()).collect();
    let sqls: Vec<_> = migrations.iter().map(|(_, _, s, _)| s.as_str()).collect();
    // Emit cargo:rerun-if-changed for each migration file
    // Note: proc macros can't emit cargo:rerun-if-changed directives.
    // The macro will re-run when the crate is recompiled.

    let expanded = quote! {
        sqlx::migrate::Migrator::from_embedded(&[
            #( sqlx::migrate::EmbeddedMigration {
                version: #versions,
                description: #descriptions,
                sql: #sqls,
            }, )*
        ])
    };

    expanded.into()
}
