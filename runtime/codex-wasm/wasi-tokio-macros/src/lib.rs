//! Proc macro `select!` for wasi-tokio.
//!
//! Matches real tokio::select! semantics:
//! 1. Guards evaluated ONCE, BEFORE futures are created (no borrow conflicts)
//! 2. Disabled branches tracked via bitmask
//! 3. Futures stored in tuple, pinned via unsafe Pin::new_unchecked
//! 4. Refutable patterns: if pattern doesn't match, branch is disabled
//! 5. Result returned via enum variant, matched outside poll_fn

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Pat, Result, Token};

struct SelectBranch {
    pat: Pat,
    future_expr: Expr,
    guard: Option<Expr>,
    body: Expr,
}

struct SelectInput {
    branches: Vec<SelectBranch>,
}

impl Parse for SelectInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Skip optional `biased;`
        if input.peek(syn::Ident) && input.peek2(Token![;]) {
            let fork = input.fork();
            let ident: syn::Ident = fork.parse()?;
            if ident == "biased" {
                input.parse::<syn::Ident>()?;
                input.parse::<Token![;]>()?;
            }
        }

        let mut branches = Vec::new();
        while !input.is_empty() {
            let pat = Pat::parse_single(input)?;
            input.parse::<Token![=]>()?;
            let future_expr: Expr = input.parse()?;

            let guard = if input.peek(Token![,]) && input.peek2(Token![if]) {
                input.parse::<Token![,]>()?;
                input.parse::<Token![if]>()?;
                Some(input.parse::<Expr>()?)
            } else {
                None
            };

            input.parse::<Token![=>]>()?;
            let body: Expr = input.parse()?;

            branches.push(SelectBranch {
                pat,
                future_expr,
                guard,
                body,
            });

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(SelectInput { branches })
    }
}

#[proc_macro]
pub fn select(input: TokenStream) -> TokenStream {
    let parsed = syn::parse_macro_input!(input as SelectInput);
    generate_select(parsed).into()
}

fn generate_select(input: SelectInput) -> TokenStream2 {
    let n = input.branches.len();

    if n == 0 {
        return quote! { std::future::pending::<()>().await };
    }

    if n == 1 {
        let b = &input.branches[0];
        let pat = &b.pat;
        let fut = &b.future_expr;
        let body = &b.body;
        return quote! {{
            let __val = { #fut }.await;
            match __val { #pat => { #body } }
        }};
    }

    // Generate enum for output variants (like tokio's __tokio_select_util::Out)
    let variant_names: Vec<_> = (0..n).map(|i| format_ident!("_V{}", i)).collect();
    let type_params: Vec<_> = (0..n).map(|i| format_ident!("_T{}", i)).collect();

    let enum_variants: Vec<_> = variant_names
        .iter()
        .zip(type_params.iter())
        .map(|(v, t)| quote! { #v(#t) })
        .collect();

    let enum_def = quote! {
        #[allow(dead_code)]
        enum __Out<#(#type_params),*> {
            #(#enum_variants,)*
            _Disabled,
        }
    };

    // Step 1: Evaluate preconditions BEFORE creating futures (matches tokio semantics)
    // This is the key insight — guards run while no futures exist, so no borrow conflicts.
    let guard_evals: Vec<_> = input
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let mask = 1u64 << i;
            match &b.guard {
                Some(guard) => quote! {
                    if !{ #guard } {
                        __disabled |= #mask;
                    }
                },
                None => quote! {},
            }
        })
        .collect();

    // Step 2: Create futures tuple (after guards, so borrows don't conflict)
    let fut_exprs: Vec<_> = input.branches.iter().map(|b| &b.future_expr).collect();

    // Step 3: Inside poll_fn, poll each non-disabled future
    let poll_branches: Vec<_> = (0..n)
        .map(|i| {
            let mask = 1u64 << i;
            let vn = &variant_names[i];
            let pat = &input.branches[i].pat;
            // Generate tuple destructuring: let (_, _, fut, ..) = &mut *__futures;
            let underscores: Vec<_> = (0..i).map(|_| quote! { _ }).collect();
            quote! {
                {
                    let __mask: u64 = #mask;
                    if __disabled & __mask == 0 {
                        let ( #(#underscores,)* ref mut __fut, .. ) = *__futures;
                        // SAFETY: futures are stored on the stack and never moved.
                        // Single-threaded WASM — no concurrent access.
                        let __fut = unsafe { std::pin::Pin::new_unchecked(__fut) };
                        match std::future::Future::poll(__fut, __cx) {
                            core::task::Poll::Ready(__out) => {
                                __disabled |= __mask;
                                // Check refutable pattern — if it doesn't match,
                                // disable this branch and continue polling others
                                #[allow(unused_variables, unused_mut)]
                                if let #pat = &__out {
                                    return core::task::Poll::Ready(__Out::#vn(__out));
                                }
                                // Pattern didn't match — branch stays disabled, continue polling
                            }
                            core::task::Poll::Pending => {
                                __is_pending = true;
                            }
                        }
                    }
                }
            }
        })
        .collect();

    // Step 4: Match arms outside poll_fn
    let match_arms: Vec<_> = input
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let vn = &variant_names[i];
            let pat = &b.pat;
            let body = &b.body;
            quote! { __Out::#vn(#pat) => { #body } }
        })
        .collect();

    // Generate branch indices as tokens
    let num_branches = n;
    let branch_indices: Vec<_> = (0..n).collect();

    quote! {{
        #enum_def

        let mut __disabled: u64 = 0;

        // Step 1: Evaluate guards BEFORE creating futures
        #(#guard_evals)*

        // Step 2+3: Create futures and poll — scoped so futures are dropped before match body
        let __output = {
            let mut __futures = ( #(#fut_exprs,)* );
            let __futures = &mut __futures;
            std::future::poll_fn(|__cx| {
                tokio::__poll_spawned_tasks(__cx);
                let mut __is_pending = false;

                // Round-robin: rotate starting branch each poll so no branch
                // is permanently starved (matches real tokio's random-start).
                let __start = tokio::__select_start(#num_branches);

                // Poll branches in round-robin order: start, start+1, ..., wrap around
                let __branch_order: [usize; #num_branches] = [
                    #( (#branch_indices + __start) % #num_branches ),*
                ];

                for &__branch_idx in &__branch_order {
                    #(
                        if __branch_idx == #branch_indices {
                            #poll_branches
                        }
                    )*
                }

                if __is_pending {
                    core::task::Poll::Pending
                } else {
                    // All branches disabled — shouldn't happen in normal use
                    panic!("all select! branches disabled")
                }
            }).await
        };

        // Step 4: Execute the matching branch body
        #[allow(unreachable_patterns)]
        match __output {
            #(#match_arms,)*
            _ => unreachable!("select! branch did not match"),
        }
    }}
}
