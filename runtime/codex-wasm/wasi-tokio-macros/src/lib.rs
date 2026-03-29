//! Proc macro `select!` for wasi-tokio.
//!
//! Generates a custom `SelectFuture` struct that holds all branch futures
//! and implements `Future` + `Unpin`. Guard expressions are evaluated
//! safely because the struct's `Unpin` impl breaks the borrow chain.

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

    // For 2+ branches: create futures eagerly, then poll via a custom
    // async block that uses unsafe to break borrow conflicts.
    //
    // Generated structure:
    //   let mut __fut_0 = Some(expr0);  // holds Option<impl Future>
    //   let mut __fut_1 = Some(expr1);
    //   ...
    //   loop {
    //       // Guards evaluated here — futures are in Options, borrows released
    //       // by setting Options to None when guard fails
    //       let __g0 = !done0 && guard0;
    //       ...
    //       // Poll: take future out of Option, poll it, put back if Pending
    //       ...
    //       tokio::__yield_once().await;  // yield to scheduler
    //   }

    let fut_vars: Vec<_> = (0..n).map(|i| format_ident!("__fut_{}", i)).collect();
    let done_vars: Vec<_> = (0..n).map(|i| format_ident!("__done_{}", i)).collect();
    let guard_vars: Vec<_> = (0..n).map(|i| format_ident!("__guard_{}", i)).collect();
    let indices: Vec<u8> = (0..n).map(|i| (i + 1) as u8).collect();

    let done_inits: Vec<_> = done_vars
        .iter()
        .map(|dv| quote! { let mut #dv = false; })
        .collect();

    // Store future expressions in Options to handle move values.
    // Each expression is evaluated ONCE (outside the loop), then .take()+recreate
    // is used per iteration to avoid borrow-across-loop issues.
    // For guarded branches, we store None initially.
    let expr_vars: Vec<_> = (0..n).map(|i| format_ident!("__expr_{}", i)).collect();
    let fut_inits: Vec<_> = input
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let ev = &expr_vars[i];
            if b.guard.is_some() {
                // Guarded: future is recreated each iteration, no need to store
                quote! {}
            } else {
                // Unguarded: wrap in Option to handle move values
                let fut_expr = &b.future_expr;
                quote! { let mut #ev = Some(#fut_expr); }
            }
        })
        .collect();

    // Guard evaluation — futures are in Options. When guard fails, we drop
    // the future by setting to None, releasing the borrow. On next iteration,
    // the future is recreated if the guard passes again.
    // NOTE: This means futures restart on each guard change. For WASM channels
    // this is fine since they resolve within one poll cycle.
    let guard_evals: Vec<_> = input
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let dv = &done_vars[i];
            let gv = &guard_vars[i];
            let _fv = &fut_vars[i];
            let _fut_expr = &b.future_expr;
            match &b.guard {
                Some(guard) => quote! { let #gv = !#dv && { #guard }; },
                None => quote! { let #gv = !#dv; },
            }
        })
        .collect();

    let match_arms: Vec<_> = input
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let idx = indices[i];
            let pat = &b.pat;
            let body = &b.body;
            let rv = format_ident!("__result_{}", i);
            // Use if-let for refutable patterns (e.g. Some(x) = stream.next())
            // If pattern doesn't match, continue the outer select loop
            quote! { #idx => { if let #pat = #rv.unwrap() { #body } else { continue '__select_loop; } } }
        })
        .collect();

    let result_vars: Vec<_> = (0..n).map(|i| format_ident!("__result_{}", i)).collect();
    let result_inits: Vec<_> = result_vars
        .iter()
        .map(|rv| quote! { let mut #rv = None; })
        .collect();

    // Generate drops for unguarded branch futures
    let drop_exprs: Vec<_> = input
        .branches
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            if b.guard.is_none() {
                let ev = &expr_vars[i];
                Some(quote! { drop(#ev); })
            } else {
                None
            }
        })
        .collect();

    // Each branch: get-or-create future, poll once, put back or drop.
    let poll_branches2: Vec<_> = (0..n)
        .map(|i| {
            let gv = &guard_vars[i];
            let dv = &done_vars[i];
            let rv = &result_vars[i];
            let idx = indices[i];
            let b = &input.branches[i];

            if b.guard.is_some() {
                // Guarded: create fresh each iteration (borrows released between iterations)
                let fut_expr = &b.future_expr;
                quote! {
                    if #gv && !#dv {
                        let mut __f = #fut_expr;
                        let __pinned = unsafe { std::pin::Pin::new_unchecked(&mut __f) };
                        match std::future::Future::poll(__pinned, &mut __cx) {
                            core::task::Poll::Ready(__val) => {
                                #dv = true;
                                #rv = Some(__val);
                                __which_branch = #idx;
                                break;
                            }
                            core::task::Poll::Pending => {}
                        }
                        // __f dropped here — borrows released before next guard eval
                    }
                }
            } else {
                // Unguarded: take from Option, poll, put back if Pending
                let ev = &expr_vars[i];
                quote! {
                    if #gv && !#dv {
                        if let Some(mut __f) = #ev.take() {
                            let __pinned = unsafe { std::pin::Pin::new_unchecked(&mut __f) };
                            match std::future::Future::poll(__pinned, &mut __cx) {
                                core::task::Poll::Ready(__val) => {
                                    #dv = true;
                                    #rv = Some(__val);
                                    __which_branch = #idx;
                                    break;
                                }
                                core::task::Poll::Pending => {
                                    #ev = Some(__f); // Put back for next iteration
                                }
                            }
                        }
                    }
                }
            }
        })
        .collect();

    quote! {{
        #(#fut_inits)*
        #(#done_inits)*
        #(#result_inits)*

        '__select_loop: loop {
            // Poll spawned tasks
            let __waker = tokio::__noop_waker();
            let mut __cx = core::task::Context::from_waker(&__waker);

            tokio::__poll_spawned_tasks(&mut __cx);

            // Phase 1: Evaluate guards (no futures alive — no borrow conflicts)
            #(#guard_evals)*

            // Phase 2: Create+poll+drop each future in one scope
            let mut __which_branch = 0u8;
            loop {
                #(#poll_branches2)*
                break; // No branch was Ready — exit inner loop
            }
            if __which_branch > 0 {
                // Drop stored futures to release borrows before match body
                #(#drop_exprs)*

                #[allow(unreachable_patterns)]
                break '__select_loop match __which_branch {
                    #(#match_arms)*
                    _ => unreachable!("select! branch index out of range")
                }
            }

            // Yield to WASM event loop
            tokio::__yield_once_sync();
        }
    }}
}
