//! The `#[pi5_executor::task]` and `#[pi5_executor::main]` attributes.
//!
//! A procedural macro is a function that runs *at compile time*, receives the
//! tokens of the item it is attached to, and returns the tokens that replace
//! it. What these two generate is small and worth knowing:
//!
//! ```ignore
//! #[task(pool_size = 2)]
//! async fn blink(mut led: Output) { ... }
//!
//! // becomes
//!
//! fn blink(led: Output) -> Result<SpawnToken<impl Sized>, SpawnError> {
//!     const POOL_SIZE: usize = 2;
//!     static POOL: TaskPoolRef = TaskPoolRef::new();           // storage for 2 instances
//!     unsafe { POOL.get::<_, POOL_SIZE>()._spawn_async_fn(move || __blink_task(led)) }
//! }
//! async fn __blink_task(mut led: Output) { ... }               // the original body
//! ```
//!
//! So calling `blink(led)` does not run anything: it stores the future in a
//! static pool and returns a token; `spawner.spawn(token)` puts it on the run
//! queue. `#[main]` wraps `async fn main(spawner: Spawner)` the same way and
//! adds the `__pi5_rt_user_main` entry point that pi5-hal's startup code calls.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Expr, ExprLit, FnArg, ItemFn, Lit, Meta, Pat, ReturnType, Type, parse_macro_input, spanned::Spanned};

fn error(span: proc_macro2::Span, msg: &str) -> TokenStream {
    syn::Error::new(span, msg).to_compile_error().into()
}

/// Declare an async task. Optional argument: `pool_size = N` (how many
/// instances may run at once; default 1).
#[proc_macro_attribute]
pub fn task(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut pool_size: usize = 1;
    if !args.is_empty() {
        let meta = parse_macro_input!(args as Meta);
        match &meta {
            Meta::NameValue(nv) if nv.path.is_ident("pool_size") => match &nv.value {
                Expr::Lit(ExprLit { lit: Lit::Int(n), .. }) => match n.base10_parse::<usize>() {
                    Ok(v) if v >= 1 => pool_size = v,
                    _ => return error(n.span(), "pool_size must be an integer >= 1"),
                },
                other => return error(other.span(), "expected an integer: `pool_size = N`"),
            },
            other => return error(other.span(), "the only supported argument is `pool_size = N`"),
        }
    }

    let f = parse_macro_input!(item as ItemFn);
    match task_impl(f, pool_size) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn task_impl(f: ItemFn, pool_size: usize) -> syn::Result<proc_macro2::TokenStream> {
    if f.sig.asyncness.is_none() {
        return Err(syn::Error::new(f.sig.span(), "task functions must be `async fn`"));
    }
    if !f.sig.generics.params.is_empty() {
        return Err(syn::Error::new(f.sig.generics.span(), "task functions must not be generic"));
    }
    match &f.sig.output {
        ReturnType::Default => {}
        ReturnType::Type(_, ty) => {
            if !matches!(**ty, Type::Tuple(ref t) if t.elems.is_empty()) {
                return Err(syn::Error::new(ty.span(), "task functions must not return a value"));
            }
        }
    }

    // Outer function: same names and types, but without `mut`/`ref` patterns
    // (those belong to the body, which stays in the inner function).
    let mut outer_args = Vec::new();
    let mut arg_names = Vec::new();
    for arg in &f.sig.inputs {
        match arg {
            FnArg::Receiver(r) => return Err(syn::Error::new(r.span(), "task functions cannot take `self`")),
            FnArg::Typed(pt) => match &*pt.pat {
                Pat::Ident(pi) => {
                    let ident = &pi.ident;
                    let ty = &pt.ty;
                    outer_args.push(quote!(#ident: #ty));
                    arg_names.push(quote!(#ident));
                }
                other => return Err(syn::Error::new(other.span(), "task arguments must be simple identifiers")),
            },
        }
    }

    let vis = &f.vis;
    let attrs = &f.attrs;
    let name = &f.sig.ident;
    let inner = format_ident!("__{}_task", name);
    let inputs = &f.sig.inputs;
    let body = &f.block;

    Ok(quote! {
        #(#attrs)*
        #vis fn #name(#(#outer_args),*) -> ::core::result::Result<
            ::pi5_executor::SpawnToken<impl ::core::marker::Sized>,
            ::pi5_executor::SpawnError,
        > {
            const POOL_SIZE: usize = #pool_size;
            static POOL: ::pi5_executor::raw::TaskPoolRef = ::pi5_executor::raw::TaskPoolRef::new();
            unsafe { POOL.get::<_, POOL_SIZE>()._spawn_async_fn(move || #inner(#(#arg_names),*)) }
        }

        #[doc(hidden)]
        #[allow(non_snake_case, clippy::too_many_arguments)]
        async fn #inner(#inputs) #body
    })
}

/// Declare the async entry point: `async fn main(spawner: Spawner)`.
///
/// Generates the task for it plus `__pi5_rt_user_main`, which creates the
/// core-0 executor and runs it with `main` as the first task.
#[proc_macro_attribute]
pub fn main(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return error(proc_macro2::Span::call_site(), "#[pi5_executor::main] takes no arguments");
    }
    let f = parse_macro_input!(item as ItemFn);
    if f.sig.asyncness.is_none() {
        return error(f.sig.span(), "main must be `async fn main(spawner: Spawner)`");
    }
    if f.sig.inputs.len() != 1 {
        return error(f.sig.inputs.span(), "main must take exactly one argument: `spawner: Spawner`");
    }
    let inputs = &f.sig.inputs;
    let body = &f.block;
    let attrs = &f.attrs;

    quote! {
        #(#attrs)*
        #[::pi5_executor::task]
        async fn __pi5_main(#inputs) #body

        #[unsafe(no_mangle)]
        pub extern "C" fn __pi5_rt_user_main() -> ! {
            let mut executor = ::pi5_executor::Executor::new();
            // `run` never returns, so the executor really does live forever.
            let executor = unsafe { ::pi5_executor::_export::make_static(&mut executor) };
            executor.run(|spawner| {
                spawner.spawn(__pi5_main(spawner).unwrap());
            })
        }
    }
    .into()
}
