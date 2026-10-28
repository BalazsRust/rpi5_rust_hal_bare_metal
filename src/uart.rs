//! PL011 UARTs: the BCM2712 debug port (`UART10`) and RP1's `UART0` on
//! header pins GPIO14/15.
//!
//! Registers: [`pi5_pac::pl011`]. Addresses/clocks: [`pi5_pac::bcm2712`]
//! (`UART10_BASE`, 9.216 MHz) and [`pi5_pac::rp1`] (`UART0_BASE`; its clock
//! is programmed to 50 MHz by [`crate::clocks::enable_uart_clock`]).
//!
//! ```no_run
//! use pi5_hal::uart::{Config, Uart};
//! # fn demo(p: pi5_hal::Peripherals) {
//! // Debug UART on the 3-pin JST connector: pins are fixed.
//! let mut dbg = Uart::new(p.UART10, Config::default()).unwrap();
//! // Header UART: pick the pins.
//! let mut hdr = Uart::new(p.UART0, Config::default().with_baudrate(9600))
//!     .unwrap()
//!     .with_tx(p.GPIO14)
//!     .with_rx(p.GPIO15);
//! dbg.write(b"hello\r\n").unwrap();
//! let mut buf = [0u8; 8];
//! let n = hdr.read(&mut buf).unwrap();
//! # }
//! ```
//!
//! # Init sequence (PL011 TRM 3.3.8)
//!
//! 1. `CR = 0` (disable; changing the baud rate while running corrupts data)
//! 2. wait for `FR.BUSY` to clear (let the current character finish)
//! 3. `LCR_H = 0` (flush FIFOs)
//! 4. `IBRD`/`FBRD` = clock / (16 x baud), integer and 1/64 fraction
//! 5. `LCR_H` = word length, parity, stop bits, FIFO enable. This write also
//!    latches the divisors.
//! 6. mask and clear interrupts, `CR = UARTEN | TXE | RXE`

use crate::asyncutil::{block_on, wait_until};
use crate::delay::Timeout;
use crate::gpio::{AnyPin, Pin, PinError, Pull, claim_pin};
use crate::{Async, Blocking, DriverMode};
use core::fmt;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use pi5_pac::mmio::{read32, write32};
use pi5_pac::pl011 as r;
use pi5_pac::rp1_pins::{Function, UartRole};

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

/// A UART peripheral token.
pub trait Instance: sealed::Sealed {
    #[doc(hidden)]
    const BASE: usize;
    #[doc(hidden)]
    const NAME: &'static str;
    #[doc(hidden)]
    fn clock_hz() -> u32;
    #[doc(hidden)]
    fn connect(pin: &AnyPin, role: UartRole) -> Result<(), PinError>;
}

/// The BCM2712 debug UART on the 3-pin JST connector (`/dev/ttyAMA10` under
/// Linux). Pins are fixed; `with_tx`/`with_rx` panic.
#[derive(Debug)]
pub struct UART10 {
    _private: (),
}
impl UART10 {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}
impl sealed::Sealed for UART10 {}
impl Instance for UART10 {
    const BASE: usize = pi5_pac::bcm2712::UART10_BASE;
    const NAME: &'static str = "UART10";
    fn clock_hz() -> u32 {
        pi5_pac::bcm2712::UART10_CLOCK_HZ
    }
    fn connect(_pin: &AnyPin, _role: UartRole) -> Result<(), PinError> {
        Err(PinError::DedicatedPins)
    }
}

/// RP1's UART0: TX on GPIO14 (header 8), RX on GPIO15 (header 10), optional
/// CTS/RTS on GPIO16/17 (`/dev/ttyAMA0` under Linux).
#[derive(Debug)]
pub struct UART0 {
    _private: (),
}
impl UART0 {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}
impl sealed::Sealed for UART0 {}
impl Instance for UART0 {
    const BASE: usize = pi5_pac::rp1::UART0_BASE;
    const NAME: &'static str = "UART0";
    fn clock_hz() -> u32 {
        crate::clocks::enable_uart_clock()
    }
    fn connect(pin: &AnyPin, role: UartRole) -> Result<(), PinError> {
        let gpio = pin.number();
        if pi5_pac::rp1_pins::uart0_role(gpio) != Some(role) {
            return Err(PinError::WrongRole { gpio });
        }
        let pull = if matches!(role, UartRole::Rx | UartRole::Cts) { Pull::Up } else { Pull::None };
        claim_pin(pin, Function::Uart(0), pull).map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// Configuration and errors
// ---------------------------------------------------------------------------

/// Number of data bits per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub enum DataBits {
    /// 5
    _5 = 0,
    /// 6
    _6 = 1,
    /// 7
    _7 = 2,
    /// 8
    #[default]
    _8 = 3,
}

/// Parity bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parity {
    /// None.
    #[default]
    None,
    /// Even.
    Even,
    /// Odd.
    Odd,
}

/// Stop bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub enum StopBits {
    /// 1
    #[default]
    _1,
    /// 2
    _2,
}

/// Serial port settings (esp-hal style builder).
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Bits per second. 115200 is what the firmware and Linux use.
    pub baudrate: u32,
    /// Data bits.
    pub data_bits: DataBits,
    /// Parity.
    pub parity: Parity,
    /// Stop bits.
    pub stop_bits: StopBits,
    /// Override the clock feeding the baud generator (normally derived from
    /// the instance). Useful to experiment if the assumed clock is wrong.
    pub clock_hz: Option<u32>,
    /// Send `\r\n` for every `\n` written through `core::fmt::Write`.
    pub lf_to_crlf: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            baudrate: 115_200,
            data_bits: DataBits::_8,
            parity: Parity::None,
            stop_bits: StopBits::_1,
            clock_hz: None,
            lf_to_crlf: true,
        }
    }
}

impl Config {
    /// Set the baud rate.
    pub const fn with_baudrate(mut self, baudrate: u32) -> Self {
        self.baudrate = baudrate;
        self
    }
    /// Set the number of data bits.
    pub const fn with_data_bits(mut self, data_bits: DataBits) -> Self {
        self.data_bits = data_bits;
        self
    }
    /// Set the parity.
    pub const fn with_parity(mut self, parity: Parity) -> Self {
        self.parity = parity;
        self
    }
    /// Set the stop bits.
    pub const fn with_stop_bits(mut self, stop_bits: StopBits) -> Self {
        self.stop_bits = stop_bits;
        self
    }
    /// Override the baud generator clock.
    pub const fn with_clock_hz(mut self, clock_hz: u32) -> Self {
        self.clock_hz = Some(clock_hz);
        self
    }
    /// Enable/disable `\n` -> `\r\n` translation for formatted output.
    pub const fn with_lf_to_crlf(mut self, on: bool) -> Self {
        self.lf_to_crlf = on;
        self
    }
}

/// Why a [`Config`] could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// The baud rate needs a divisor outside the 16-bit `IBRD` range.
    UnachievableBaudrate,
    /// The UART did not go idle (clock not running?).
    NotIdle,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::UnachievableBaudrate => write!(f, "baud rate not achievable with this clock"),
            ConfigError::NotIdle => write!(f, "UART stayed busy; is its clock running?"),
        }
    }
}

/// Transmit errors. There are none on a PL011 (a write cannot fail), but the
/// `Result` keeps the API identical to esp-hal's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxError {}

/// A receive error, reported for the byte it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxError {
    /// Stop bit missing: usually a baud-rate mismatch.
    Framing,
    /// Parity mismatch.
    Parity,
    /// Break condition.
    Break,
    /// Receive FIFO overflowed; at least one byte was lost before this one.
    Overrun,
}

impl fmt::Display for RxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RxError::Framing => write!(f, "framing error (baud rate mismatch?)"),
            RxError::Parity => write!(f, "parity error"),
            RxError::Break => write!(f, "break condition"),
            RxError::Overrun => write!(f, "receive overrun, data lost"),
        }
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Set once UART10 has been programmed, so the panic/exception handlers know
/// whether to initialise it before printing.
static UART10_READY: AtomicBool = AtomicBool::new(false);
/// Set once a program has configured RP1 UART0 (GPIO14/15). The panic and
/// exception handlers then print there too, where the user's serial
/// adapter actually is, instead of only on the debug connector.
static UART0_READY: AtomicBool = AtomicBool::new(false);

/// A PL011 UART driver.
pub struct Uart<Dm: DriverMode = Blocking> {
    base: usize,
    name: &'static str,
    clock_hz: u32,
    lf_to_crlf: bool,
    connect: fn(&AnyPin, UartRole) -> Result<(), PinError>,
    _mode: PhantomData<Dm>,
}

impl<Dm: DriverMode> fmt::Debug for Uart<Dm> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uart({})", self.name)
    }
}

impl Uart<Blocking> {
    /// Take a UART and program it.
    pub fn new<U: Instance>(_uart: U, config: Config) -> Result<Self, ConfigError> {
        let mut uart = Self {
            base: U::BASE,
            name: U::NAME,
            clock_hz: U::clock_hz(),
            lf_to_crlf: config.lf_to_crlf,
            connect: U::connect,
            _mode: PhantomData,
        };
        uart.apply_config(&config)?;
        if U::BASE == pi5_pac::bcm2712::UART10_BASE {
            UART10_READY.store(true, Ordering::Release);
        } else if U::BASE == UART0::BASE {
            UART0_READY.store(true, Ordering::Release);
        }
        Ok(uart)
    }

    /// Use the debug UART exactly as the firmware left it (115200 8N1 when
    /// `uart_2ndstage=1`), only making sure it is enabled. If this prints
    /// readable text while [`Uart::new`] does not, the assumed clock is wrong.
    pub fn new_firmware_configured(_uart: UART10) -> Self {
        let uart = Self {
            base: UART10::BASE,
            name: UART10::NAME,
            clock_hz: UART10::clock_hz(),
            lf_to_crlf: true,
            connect: UART10::connect,
            _mode: PhantomData,
        };
        // SAFETY: we own the UART.
        unsafe { write32(uart.base + r::CR, read32(uart.base + r::CR) | r::CR_UARTEN | r::CR_TXE | r::CR_RXE) };
        UART10_READY.store(true, Ordering::Release);
        uart
    }

    /// Switch to the async API.
    pub fn into_async(self) -> Uart<Async> {
        self.convert()
    }
}

impl Uart<Async> {
    /// Switch back to the blocking-only API.
    pub fn into_blocking(self) -> Uart<Blocking> {
        self.convert()
    }

    /// Write all of `data`, yielding to other tasks whenever the FIFO is full.
    pub async fn write_async(&mut self, data: &[u8]) -> Result<usize, TxError> {
        self.do_write(data).await
    }

    /// Wait until the last byte has left the shift register.
    pub async fn flush_async(&mut self) {
        let base = self.base;
        wait_until(move || flag(base, r::FR_BUSY) == 0).await
    }

    /// Wait for at least one byte, then read as many as are available (up to
    /// `buf.len()`). Returns how many.
    pub async fn read_async(&mut self, buf: &mut [u8]) -> Result<usize, RxError> {
        self.do_read(buf).await
    }

    /// Read exactly `buf.len()` bytes.
    pub async fn read_exact_async(&mut self, buf: &mut [u8]) -> Result<(), RxError> {
        let mut done = 0;
        while done < buf.len() {
            done += self.do_read(&mut buf[done..]).await?;
        }
        Ok(())
    }

    /// Read one byte.
    pub async fn read_byte_async(&mut self) -> Result<u8, RxError> {
        let mut b = [0u8];
        self.do_read(&mut b).await?;
        Ok(b[0])
    }
}

impl<Dm: DriverMode> Uart<Dm> {
    fn convert<To: DriverMode>(self) -> Uart<To> {
        Uart {
            base: self.base,
            name: self.name,
            clock_hz: self.clock_hz,
            lf_to_crlf: self.lf_to_crlf,
            connect: self.connect,
            _mode: PhantomData,
        }
    }

    /// Route the TX signal to `pin` (RP1 UARTs only).
    ///
    /// # Panics
    /// If the pin cannot carry this UART's TX, with a message saying why.
    pub fn with_tx(self, pin: impl Pin) -> Self {
        self.connect_pin(pin, UartRole::Tx);
        self
    }

    /// Route the RX signal to `pin` (RP1 UARTs only).
    pub fn with_rx(self, pin: impl Pin) -> Self {
        self.connect_pin(pin, UartRole::Rx);
        self
    }

    /// Route CTS to `pin` and enable hardware flow control on it.
    pub fn with_cts(self, pin: impl Pin) -> Self {
        self.connect_pin(pin, UartRole::Cts);
        // SAFETY: we own the UART.
        unsafe { write32(self.base + r::CR, read32(self.base + r::CR) | r::CR_CTSEN) };
        self
    }

    /// Route RTS to `pin` and enable hardware flow control on it.
    pub fn with_rts(self, pin: impl Pin) -> Self {
        self.connect_pin(pin, UartRole::Rts);
        // SAFETY: we own the UART.
        unsafe { write32(self.base + r::CR, read32(self.base + r::CR) | r::CR_RTSEN) };
        self
    }

    fn connect_pin(&self, pin: impl Pin, role: UartRole) {
        let pin = pin.degrade();
        if let Err(e) = (self.connect)(&pin, role) {
            panic!("{}: GPIO{} cannot be {:?}: {}", self.name, pin.number(), role, e);
        }
    }

    /// Re-program baud rate / frame format.
    pub fn apply_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let clock = config.clock_hz.unwrap_or(self.clock_hz);
        // Divisor in 1/64ths: 64 * clock / (16 * baud) = 4 * clock / baud, rounded.
        let div64 = (4 * u64::from(clock) + u64::from(config.baudrate) / 2) / u64::from(config.baudrate);
        let ibrd = div64 >> 6;
        let fbrd = (div64 & 0x3f) as u32;
        if ibrd == 0 || ibrd > 0xffff {
            return Err(ConfigError::UnachievableBaudrate);
        }
        self.lf_to_crlf = config.lf_to_crlf;

        let mut lcr = ((config.data_bits as u32) << r::LCR_H_WLEN_SHIFT) | r::LCR_H_FEN;
        match config.parity {
            Parity::None => {}
            Parity::Even => lcr |= r::LCR_H_PEN | r::LCR_H_EPS,
            Parity::Odd => lcr |= r::LCR_H_PEN,
        }
        if config.stop_bits == StopBits::_2 {
            lcr |= r::LCR_H_STP2;
        }

        // SAFETY: we own the UART; all offsets are PL011 registers.
        unsafe {
            let b = self.base;
            write32(b + r::CR, 0);
            let t = Timeout::new(Duration::from_millis(20));
            while read32(b + r::FR) & r::FR_BUSY != 0 {
                if t.expired() {
                    return Err(ConfigError::NotIdle);
                }
            }
            write32(b + r::LCR_H, 0);
            write32(b + r::IBRD, ibrd as u32);
            write32(b + r::FBRD, fbrd);
            write32(b + r::LCR_H, lcr);
            write32(b + r::IMSC, 0);
            write32(b + r::ICR, r::ICR_ALL);
            write32(b + r::CR, r::CR_UARTEN | r::CR_TXE | r::CR_RXE);
        }
        Ok(())
    }

    // --- blocking API (available in both modes) ---

    /// Write all of `data`, blocking while the FIFO is full.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, TxError> {
        block_on(self.do_write(data))
    }

    /// Write one byte.
    pub fn write_byte(&mut self, byte: u8) {
        let _ = self.write(core::slice::from_ref(&byte));
    }

    /// Wait until the last byte has left the shift register.
    pub fn flush(&mut self) {
        let base = self.base;
        block_on(wait_until(move || flag(base, r::FR_BUSY) == 0))
    }

    /// Block until at least one byte is available, then read as many as are
    /// available (up to `buf.len()`).
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, RxError> {
        block_on(self.do_read(buf))
    }

    /// Read exactly `buf.len()` bytes.
    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), RxError> {
        let mut done = 0;
        while done < buf.len() {
            done += self.read(&mut buf[done..])?;
        }
        Ok(())
    }

    /// Block until one byte arrives.
    pub fn read_byte(&mut self) -> Result<u8, RxError> {
        let mut b = [0u8];
        self.read(&mut b)?;
        Ok(b[0])
    }

    /// Non-blocking: one byte if available.
    pub fn try_read_byte(&mut self) -> Option<Result<u8, RxError>> {
        if flag(self.base, r::FR_RXFE) != 0 {
            return None;
        }
        Some(pop_byte(self.base))
    }

    /// The PrimeCell peripheral ID, assembled from the four ID registers. For
    /// UART10 the device tree says `0x00341011` (`bcm2712.dtsi:247`); reading
    /// that back proves the address and bus mapping are right.
    pub fn peripheral_id(&self) -> u32 {
        let mut id = 0;
        for i in 0..4 {
            // SAFETY: ID registers are read-only.
            id |= (unsafe { read32(self.base + r::PERIPH_ID0 + i * 4) } & 0xff) << (8 * i);
        }
        id
    }

    /// Clock the baud generator is assumed to run from.
    pub fn clock_hz(&self) -> u32 {
        self.clock_hz
    }

    // --- shared implementation ---

    async fn do_write(&mut self, data: &[u8]) -> Result<usize, TxError> {
        let base = self.base;
        for &b in data {
            // Wait while the transmit FIFO is full.
            wait_until(move || flag(base, r::FR_TXFF) == 0).await;
            // SAFETY: we own the UART.
            unsafe { write32(base + r::DR, u32::from(b)) };
        }
        Ok(data.len())
    }

    async fn do_read(&mut self, buf: &mut [u8]) -> Result<usize, RxError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let base = self.base;
        wait_until(move || flag(base, r::FR_RXFE) == 0).await;
        let mut n = 0;
        while n < buf.len() && flag(base, r::FR_RXFE) == 0 {
            buf[n] = pop_byte(base)?;
            n += 1;
        }
        Ok(n)
    }

    /// Emergency handle for the panic/exception handlers: RP1 UART0 if the
    /// program set it up (it is then wired to something), otherwise the
    /// debug UART, initialised with defaults if nobody did so yet.
    ///
    /// # Safety
    /// Bypasses ownership; only for use once normal execution is over.
    pub(crate) unsafe fn emergency() -> Uart<Blocking> {
        if UART0_READY.load(Ordering::Acquire) {
            // Already configured (baud rate, pins): use it as it is.
            return Uart::<Blocking> {
                base: UART0::BASE,
                name: UART0::NAME,
                clock_hz: 0,
                lf_to_crlf: true,
                connect: UART0::connect,
                _mode: PhantomData,
            };
        }
        let mut uart = Uart::<Blocking> {
            base: UART10::BASE,
            name: UART10::NAME,
            clock_hz: UART10::clock_hz(),
            lf_to_crlf: true,
            connect: UART10::connect,
            _mode: PhantomData,
        };
        if !UART10_READY.load(Ordering::Acquire) {
            let _ = uart.apply_config(&Config::default());
            UART10_READY.store(true, Ordering::Release);
        }
        uart
    }
}

#[inline]
fn flag(base: usize, bit: u32) -> u32 {
    // SAFETY: FR is a read-only status register.
    unsafe { read32(base + r::FR) & bit }
}

/// Pop one entry from the receive FIFO and decode its error bits.
fn pop_byte(base: usize) -> Result<u8, RxError> {
    // SAFETY: reading DR pops the FIFO; the caller checked it is not empty.
    let word = unsafe { read32(base + r::DR) };
    if word & r::DR_OE != 0 {
        Err(RxError::Overrun)
    } else if word & r::DR_BE != 0 {
        Err(RxError::Break)
    } else if word & r::DR_PE != 0 {
        Err(RxError::Parity)
    } else if word & r::DR_FE != 0 {
        Err(RxError::Framing)
    } else {
        Ok(word as u8)
    }
}

/// `write!(uart, "x = {}", x)` support.
impl<Dm: DriverMode> fmt::Write for Uart<Dm> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if b == b'\n' && self.lf_to_crlf {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
        Ok(())
    }
}
