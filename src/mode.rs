//! Driver modes, as in esp-hal: `Uart<Blocking>` vs `Uart<Async>`.
//!
//! The mode is a zero-sized type parameter. Blocking methods exist in both
//! modes; the `*_async` methods only on `Async`. `into_async()` /
//! `into_blocking()` convert, costing nothing at run time. The point is
//! purely documentary and type-checked: a function that takes
//! `Uart<Async>` cannot be handed a driver someone forgot to convert.

mod sealed {
    pub trait Sealed {}
}

/// Marker trait for [`Blocking`] and [`Async`].
pub trait DriverMode: sealed::Sealed + 'static {
    /// `true` for [`Async`].
    const IS_ASYNC: bool;
}

/// The driver only offers blocking methods.
#[derive(Debug)]
pub struct Blocking;

/// The driver additionally offers `async` methods.
#[derive(Debug)]
pub struct Async;

impl sealed::Sealed for Blocking {}
impl sealed::Sealed for Async {}
impl DriverMode for Blocking {
    const IS_ASYNC: bool = false;
}
impl DriverMode for Async {
    const IS_ASYNC: bool = true;
}
