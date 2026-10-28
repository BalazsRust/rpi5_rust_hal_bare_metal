//! SPI master on RP1's DesignWare SSI controllers (`SPI0`, `SPI1`).
//!
//! Registers: [`pi5_pac::dw_spi`] (`spi-dw.h`); procedure after
//! `spi-dw-core.c` in polling mode.
//!
//! ```no_run
//! use pi5_hal::spi::{Config, Mode, Spi};
//! use pi5_hal::time::Rate;
//! # fn demo(p: pi5_hal::Peripherals) -> Result<(), pi5_hal::spi::Error> {
//! let mut spi = Spi::new(p.SPI0, Config::default().with_frequency(Rate::from_mhz(1)).with_mode(Mode::_0))
//!     .unwrap()
//!     .with_sck(p.GPIO11)
//!     .with_mosi(p.GPIO10)
//!     .with_miso(p.GPIO9)
//!     .with_cs(p.GPIO8);          // hardware CE0
//! let mut buf = [0x9F, 0, 0, 0];  // e.g. JEDEC ID of a flash chip
//! spi.transfer_in_place(&mut buf)?;
//! # Ok(()) }
//! ```
//!
//! # Chip select
//!
//! `with_cs` uses the controller's own CS lines (`SER` register). The
//! DesignWare core asserts CS while the TX FIFO has data and **drops it as
//! soon as the FIFO runs empty**, so for transfers longer than the FIFO (64
//! frames on RP1, detected at init) CS may pulse between chunks. For devices
//! that care, drive CS from an [`crate::gpio::Output`] yourself (that is also
//! what `embedded-hal`'s `SpiDevice` wrappers expect).

use crate::asyncutil::{block_on, yield_now};
use crate::clocks;
use crate::delay::Timeout;
use crate::gpio::{AnyPin, Pin, PinError, Pull, claim_pin};
use crate::time::Rate;
use crate::{Async, Blocking, DriverMode};
use core::fmt;
use core::marker::PhantomData;
use core::time::Duration;
use embedded_hal::spi::{ErrorKind, ErrorType, SpiBus};
use pi5_pac::dw_spi as r;
use pi5_pac::mmio::{read32, write32};
use pi5_pac::rp1_pins::{Function, SpiRole, spi_role};

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

/// An SPI peripheral token.
pub trait Instance: sealed::Sealed {
    #[doc(hidden)]
    const BASE: usize;
    #[doc(hidden)]
    const INDEX: u8;
}

macro_rules! instance {
    ($name:ident, $base:expr, $index:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug)]
        pub struct $name {
            _private: (),
        }
        impl $name {
            pub(crate) const fn new() -> Self {
                Self { _private: () }
            }
        }
        impl sealed::Sealed for $name {}
        impl Instance for $name {
            const BASE: usize = $base;
            const INDEX: u8 = $index;
        }
    };
}
instance!(SPI0, pi5_pac::rp1::SPI0_BASE, 0, "SPI0: SCLK GPIO11, MOSI GPIO10, MISO GPIO9, CE0 GPIO8, CE1 GPIO7.");
instance!(SPI1, pi5_pac::rp1::SPI1_BASE, 1, "SPI1: SCLK GPIO21, MOSI GPIO20, MISO GPIO19, CE0 GPIO18, CE1 GPIO17, CE2 GPIO16.");

// ---------------------------------------------------------------------------
// Configuration and errors
// ---------------------------------------------------------------------------

/// SPI clock polarity/phase, numbered as everyone numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub enum Mode {
    /// CPOL = 0, CPHA = 0: idle low, sample on rising edge.
    #[default]
    _0,
    /// CPOL = 0, CPHA = 1.
    _1,
    /// CPOL = 1, CPHA = 0.
    _2,
    /// CPOL = 1, CPHA = 1.
    _3,
}

impl From<embedded_hal::spi::Mode> for Mode {
    fn from(m: embedded_hal::spi::Mode) -> Self {
        use embedded_hal::spi::{Phase, Polarity};
        match (m.polarity, m.phase) {
            (Polarity::IdleLow, Phase::CaptureOnFirstTransition) => Mode::_0,
            (Polarity::IdleLow, Phase::CaptureOnSecondTransition) => Mode::_1,
            (Polarity::IdleHigh, Phase::CaptureOnFirstTransition) => Mode::_2,
            (Polarity::IdleHigh, Phase::CaptureOnSecondTransition) => Mode::_3,
        }
    }
}

/// Bus settings.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// SCLK frequency (default 1 MHz). Achieved as `clk_sys / even divisor`.
    pub frequency: Rate,
    /// Clock mode.
    pub mode: Mode,
    /// Give up on a transfer after this long (default 100 ms).
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { frequency: Rate::from_mhz(1), mode: Mode::_0, timeout: Duration::from_millis(100) }
    }
}

impl Config {
    /// Set the clock frequency.
    pub const fn with_frequency(mut self, frequency: Rate) -> Self {
        self.frequency = frequency;
        self
    }
    /// Set the clock mode.
    pub const fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
    /// Set the per-transfer timeout.
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Why a [`Config`] could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Frequency is 0 or needs a divisor outside 2..=65534.
    FrequencyOutOfRange,
    /// No FIFO could be detected: wrong address, or RP1 not reachable.
    NoController,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::FrequencyOutOfRange => write!(f, "SPI frequency out of range for clk_sys / even divisor"),
            ConfigError::NoController => write!(f, "no DesignWare SSI responded at this address"),
        }
    }
}

/// Transfer errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The receive FIFO overflowed: data lost.
    Overrun,
    /// Another master drove the bus (`SR.DCOL`).
    DataCollision,
    /// The transfer did not finish within `Config::timeout`.
    Timeout,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Overrun => write!(f, "receive FIFO overrun"),
            Error::DataCollision => write!(f, "data collision (multi-master contention)"),
            Error::Timeout => write!(f, "timeout"),
        }
    }
}

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Error::Overrun => ErrorKind::Overrun,
            Error::DataCollision => ErrorKind::ModeFault,
            Error::Timeout => ErrorKind::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// An SPI master, 8-bit frames, MSB first.
pub struct Spi<Dm: DriverMode = Blocking> {
    base: usize,
    index: u8,
    fifo_len: u32,
    dfs32: bool,
    cs_mask: u32,
    timeout: Duration,
    _mode: PhantomData<Dm>,
}

impl<Dm: DriverMode> fmt::Debug for Spi<Dm> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Spi{}", self.index)
    }
}

/// Either two buffers or one shared buffer (in-place transfer).
enum Buffers<'a> {
    Separate { tx: &'a [u8], rx: &'a mut [u8] },
    InPlace(&'a mut [u8]),
}

impl Spi<Blocking> {
    /// Take a controller and configure it. Pins are attached with the
    /// `with_*` methods.
    pub fn new<S: Instance>(_spi: S, config: Config) -> Result<Self, ConfigError> {
        let mut spi = Self {
            base: S::BASE,
            index: S::INDEX,
            fifo_len: 0,
            dfs32: false,
            cs_mask: 0,
            timeout: config.timeout,
            _mode: PhantomData,
        };
        let b = spi.base;
        // SAFETY: we own the controller.
        unsafe {
            write32(b + r::SSIENR, 0);
            // Where is the frame-size field? (dw_spi_hw_init.) Write all ones
            // and look whether bits 3:0 are writable.
            write32(b + r::CTRLR0, 0xffff_ffff);
            spi.dfs32 = read32(b + r::CTRLR0) & r::CTRLR0_DFS_MASK == 0;
            write32(b + r::CTRLR0, 0);
            // FIFO depth: raise the TX threshold until it stops sticking.
            let mut depth = 0;
            for fifo in 1..256u32 {
                write32(b + r::TXFTLR, fifo);
                if read32(b + r::TXFTLR) != fifo {
                    break;
                }
                depth = fifo;
            }
            write32(b + r::TXFTLR, 0);
            if depth == 0 {
                return Err(ConfigError::NoController);
            }
            spi.fifo_len = depth + 1;
            write32(b + r::IMR, 0);
            write32(b + r::SER, 0);
            write32(b + r::RX_SAMPLE_DLY, 0);
        }
        spi.apply_config(&config)?;
        Ok(spi)
    }

    /// Switch to the async API.
    pub fn into_async(self) -> Spi<Async> {
        self.convert()
    }
}

impl Spi<Async> {
    /// Switch back to the blocking-only API.
    pub fn into_blocking(self) -> Spi<Blocking> {
        self.convert()
    }

    /// Send `words`, discarding what comes back.
    pub async fn write_async(&mut self, words: &[u8]) -> Result<(), Error> {
        self.do_transfer(words.len(), Buffers::Separate { tx: words, rx: &mut [] }).await
    }

    /// Receive `words.len()` bytes, sending zeros.
    pub async fn read_async(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let n = words.len();
        self.do_transfer(n, Buffers::Separate { tx: &[], rx: words }).await
    }

    /// Full duplex; the longer buffer sets the length, the shorter is padded
    /// with zeros / truncated.
    pub async fn transfer_async(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let n = read.len().max(write.len());
        self.do_transfer(n, Buffers::Separate { tx: write, rx: read }).await
    }

    /// Full duplex in one buffer: each byte is replaced by what was received
    /// while it was sent.
    pub async fn transfer_in_place_async(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let n = words.len();
        self.do_transfer(n, Buffers::InPlace(words)).await
    }
}

impl<Dm: DriverMode> Spi<Dm> {
    fn convert<To: DriverMode>(self) -> Spi<To> {
        Spi {
            base: self.base,
            index: self.index,
            fifo_len: self.fifo_len,
            dfs32: self.dfs32,
            cs_mask: self.cs_mask,
            timeout: self.timeout,
            _mode: PhantomData,
        }
    }

    /// Attach the clock pin.
    ///
    /// # Panics
    /// If the pin is not this controller's SCLK.
    pub fn with_sck(self, pin: impl Pin) -> Self {
        self.connect(pin, SpiRole::Sclk, Pull::None);
        self
    }

    /// Attach the MOSI pin.
    pub fn with_mosi(self, pin: impl Pin) -> Self {
        self.connect(pin, SpiRole::Mosi, Pull::None);
        self
    }

    /// Attach the MISO pin.
    pub fn with_miso(self, pin: impl Pin) -> Self {
        self.connect(pin, SpiRole::Miso, Pull::Up);
        self
    }

    /// Attach a hardware chip-select pin (CE0/CE1/...). Several may be
    /// attached; all of them are asserted together during a transfer.
    pub fn with_cs(mut self, pin: impl Pin) -> Self {
        let pin = pin.degrade();
        let gpio = pin.number();
        match spi_role(self.index, gpio) {
            Some(SpiRole::Cs(n)) => {
                if let Err(e) = claim_pin(&pin, Function::Spi(self.index), Pull::Up) {
                    panic!("SPI{}: GPIO{gpio} cannot be CS: {e}", self.index);
                }
                self.cs_mask |= 1 << n;
            }
            _ => panic!("SPI{}: GPIO{gpio} is not one of its chip selects", self.index),
        }
        self
    }

    fn connect(&self, pin: impl Pin, role: SpiRole, pull: Pull) {
        let pin: AnyPin = pin.degrade();
        let gpio = pin.number();
        let result = if spi_role(self.index, gpio) != Some(role) {
            Err(PinError::WrongRole { gpio })
        } else {
            claim_pin(&pin, Function::Spi(self.index), pull).map(|_| ())
        };
        if let Err(e) = result {
            panic!("SPI{}: GPIO{} cannot be {:?}: {}", self.index, gpio, role, e);
        }
    }

    /// Re-program frequency and mode.
    pub fn apply_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let hz = config.frequency.as_hz();
        if hz == 0 {
            return Err(ConfigError::FrequencyOutOfRange);
        }
        // BAUDR: even divisor of clk_sys, 2..=65534 (dw_spi_update_config).
        let clk = clocks::clk_sys_hz();
        let mut div = clk.div_ceil(hz);
        div = (div + 1) & !1;
        if !(2..=0xfffe).contains(&div) {
            return Err(ConfigError::FrequencyOutOfRange);
        }
        self.timeout = config.timeout;

        let mut cr0 = 0; // FRF = Motorola, TMOD = transmit & receive
        if self.dfs32 { cr0 |= 7 << r::CTRLR0_DFS32_SHIFT } else { cr0 |= 7 } // 8-bit frames
        if matches!(config.mode, Mode::_1 | Mode::_3) {
            cr0 |= r::CTRLR0_SCPH;
        }
        if matches!(config.mode, Mode::_2 | Mode::_3) {
            cr0 |= r::CTRLR0_SCPOL;
        }
        // SAFETY: we own the controller; CTRLR0/BAUDR may only change while disabled.
        unsafe {
            write32(self.base + r::SSIENR, 0);
            write32(self.base + r::CTRLR0, cr0);
            write32(self.base + r::BAUDR, div);
        }
        Ok(())
    }

    /// The SCLK frequency actually configured.
    pub fn frequency(&self) -> Rate {
        // SAFETY: BAUDR read.
        let div = unsafe { read32(self.base + r::BAUDR) }.max(2);
        Rate::from_hz(clocks::clk_sys_hz() / div)
    }

    /// Detected FIFO depth in frames.
    pub fn fifo_depth(&self) -> u32 {
        self.fifo_len
    }

    // --- blocking API ---

    /// Send `words`, discarding what comes back.
    pub fn write(&mut self, words: &[u8]) -> Result<(), Error> {
        block_on(self.do_transfer(words.len(), Buffers::Separate { tx: words, rx: &mut [] }))
    }

    /// Receive `words.len()` bytes, sending zeros.
    pub fn read(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let n = words.len();
        block_on(self.do_transfer(n, Buffers::Separate { tx: &[], rx: words }))
    }

    /// Full duplex; the longer buffer sets the length.
    pub fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let n = read.len().max(write.len());
        block_on(self.do_transfer(n, Buffers::Separate { tx: write, rx: read }))
    }

    /// Full duplex in one buffer.
    pub fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let n = words.len();
        block_on(self.do_transfer(n, Buffers::InPlace(words)))
    }

    /// Wait until the bus is idle (transfers here are synchronous, so this
    /// returns immediately; kept for the `SpiBus` contract).
    pub fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    // --- implementation ---

    /// Clock `len` frames out and in, keeping the TX FIFO ahead of the RX
    /// FIFO by at most its depth (`dw_spi_poll_transfer`).
    async fn do_transfer(&mut self, len: usize, mut bufs: Buffers<'_>) -> Result<(), Error> {
        if len == 0 {
            return Ok(());
        }
        let b = self.base;
        let fifo = self.fifo_len as usize;
        // SAFETY: we own the controller.
        unsafe {
            let _ = read32(b + r::ICR); // clear stale error flags
            write32(b + r::SSIENR, 1);
            write32(b + r::SER, self.cs_mask);
        }
        let timeout = Timeout::new(self.timeout);
        let (mut sent, mut received) = (0usize, 0usize);
        let result = loop {
            let mut progressed = false;
            // Fill: while there is room and we are not too far ahead of RX.
            while sent < len && sent - received < fifo && sr(b) & r::SR_TF_NOT_FULL != 0 {
                let byte = match &bufs {
                    Buffers::Separate { tx, .. } => tx.get(sent).copied().unwrap_or(0),
                    Buffers::InPlace(buf) => buf[sent],
                };
                // SAFETY: TFNF was just checked.
                unsafe { write32(b + r::DR, u32::from(byte)) };
                sent += 1;
                progressed = true;
            }
            // Drain everything that has come back.
            while received < sent && sr(b) & r::SR_RF_NOT_EMPTY != 0 {
                // SAFETY: RFNE was just checked.
                let v = unsafe { read32(b + r::DR) } as u8;
                match &mut bufs {
                    Buffers::Separate { rx, .. } => {
                        if received < rx.len() {
                            rx[received] = v;
                        }
                    }
                    Buffers::InPlace(buf) => buf[received] = v,
                }
                received += 1;
                progressed = true;
            }
            // SAFETY: RISR/SR are status registers.
            let risr = unsafe { read32(b + r::RISR) };
            if risr & r::INT_RXOI != 0 {
                break Err(Error::Overrun);
            }
            if sr(b) & r::SR_DCOL != 0 {
                break Err(Error::DataCollision);
            }
            if received == len {
                break Ok(());
            }
            if timeout.expired() {
                break Err(Error::Timeout);
            }
            if !progressed {
                yield_now().await;
            }
        };
        // Let the last frame finish, then deassert CS and flush the FIFOs.
        let t = Timeout::new(Duration::from_millis(10));
        while sr(b) & r::SR_BUSY != 0 && !t.expired() {}
        // SAFETY: we own the controller.
        unsafe {
            write32(b + r::SER, 0);
            write32(b + r::SSIENR, 0);
        }
        result
    }
}

#[inline]
fn sr(base: usize) -> u32 {
    // SAFETY: SR is read-only.
    unsafe { read32(base + r::SR) }
}

impl<Dm: DriverMode> ErrorType for Spi<Dm> {
    type Error = Error;
}

impl<Dm: DriverMode> SpiBus<u8> for Spi<Dm> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Error> {
        Spi::read(self, words)
    }
    fn write(&mut self, words: &[u8]) -> Result<(), Error> {
        Spi::write(self, words)
    }
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        Spi::transfer(self, read, write)
    }
    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Error> {
        Spi::transfer_in_place(self, words)
    }
    fn flush(&mut self) -> Result<(), Error> {
        Spi::flush(self)
    }
}
