//! I2C master on RP1's DesignWare controllers (`I2C0`-`I2C3`).
//!
//! Registers: [`pi5_pac::dw_i2c`] (`i2c-designware-core.h`); the transfer
//! procedure follows `i2c-designware-master.c` without interrupts.
//!
//! ```no_run
//! use pi5_hal::i2c::{Config, I2c};
//! use pi5_hal::time::Rate;
//! # fn demo(p: pi5_hal::Peripherals) -> Result<(), pi5_hal::i2c::Error> {
//! let mut i2c = I2c::new(p.I2C1, Config::default().with_frequency(Rate::from_khz(400)))
//!     .unwrap()
//!     .with_sda(p.GPIO2)
//!     .with_scl(p.GPIO3);
//! let mut id = [0u8; 1];
//! i2c.write_read(0x68, &[0x75], &mut id)?;   // read register 0x75 of an MPU-6050
//! # Ok(()) }
//! ```
//!
//! # How a DesignWare I2C transfer works
//!
//! The controller is a command FIFO. For every byte you push one entry into
//! `DATA_CMD`: either a byte to write, or a "read one byte" request (bit 8).
//! Two more bits say "put a RESTART before this byte" (bit 10) and "put a
//! STOP after it" (bit 9). Received bytes pop out of the same register.
//! If the slave does not acknowledge, the controller aborts, flushes the
//! FIFO, and records why in `TX_ABRT_SOURCE`.
//!
//! # Timing
//!
//! SCL high/low times are counts of the controller clock (`clk_sys`, measured
//! by [`crate::clocks::clk_sys_hz`]). The counts come from the same formulas
//! Linux uses ([`pi5_pac::dw_i2c::timing`]), with the rise/fall times the
//! device tree states for RP1's pads.

use crate::asyncutil::{block_on, yield_now};
use crate::clocks;
use crate::delay::Timeout;
use crate::gpio::{AnyPin, Pin, PinError, Pull, claim_pin};
use crate::time::Rate;
use crate::{Async, Blocking, DriverMode};
use core::fmt;
use core::marker::PhantomData;
use core::time::Duration;
use embedded_hal::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation, SevenBitAddress};
use pi5_pac::dw_i2c as r;
use pi5_pac::mmio::{read32, write32};
use pi5_pac::rp1_pins::{Function, I2cRole, i2c_role};

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

/// An I2C peripheral token.
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
instance!(I2C0, pi5_pac::rp1::I2C0_BASE, 0, "I2C0: SDA/SCL on GPIO0/1 (ID EEPROM pins) or GPIO8/9.");
instance!(I2C1, pi5_pac::rp1::I2C1_BASE, 1, "I2C1: SDA/SCL on GPIO2/3 (header 3/5, the usual bus) or GPIO10/11.");
instance!(I2C2, pi5_pac::rp1::I2C2_BASE, 2, "I2C2: SDA/SCL on GPIO4/5 or GPIO12/13.");
instance!(I2C3, pi5_pac::rp1::I2C3_BASE, 3, "I2C3: SDA/SCL on GPIO6/7, GPIO14/15 or GPIO22/23.");

// ---------------------------------------------------------------------------
// Configuration and errors
// ---------------------------------------------------------------------------

/// Bus settings.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// SCL frequency, up to 1 MHz. 100 kHz default.
    pub frequency: Rate,
    /// Give up on a transaction after this long (default 100 ms).
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { frequency: Rate::from_khz(100), timeout: Duration::from_millis(100) }
    }
}

impl Config {
    /// Set the SCL frequency.
    pub const fn with_frequency(mut self, frequency: Rate) -> Self {
        self.frequency = frequency;
        self
    }
    /// Set the per-transaction timeout.
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Why a [`Config`] could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Frequency is 0 or above 1 MHz.
    FrequencyOutOfRange,
    /// The register block does not identify as a DesignWare I2C (wrong
    /// address, or RP1 not reachable).
    NotDesignWare {
        /// What `COMP_TYPE` read instead of `0x44570140`.
        comp_type: u32,
    },
    /// The controller refused to disable within the timeout.
    NotIdle,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::FrequencyOutOfRange => write!(f, "I2C frequency must be 1 Hz..=1 MHz"),
            ConfigError::NotDesignWare { comp_type } => {
                write!(f, "no DesignWare I2C at this address (COMP_TYPE = {comp_type:#010x}, expected 0x44570140)")
            }
            ConfigError::NotIdle => write!(f, "I2C controller would not disable"),
        }
    }
}

/// Transfer errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No device acknowledged the address.
    AddressNack,
    /// The device acknowledged its address but not a data byte.
    DataNack,
    /// Another master won the bus.
    ArbitrationLost,
    /// The receive FIFO overflowed (bytes lost).
    Overrun,
    /// The transaction did not finish within `Config::timeout`. Usually a
    /// stuck bus (SDA or SCL held low) or missing pull-ups.
    Timeout,
    /// The controller aborted for another reason; raw `TX_ABRT_SOURCE`.
    Abort(u32),
    /// A zero-length operation; not supported by this driver.
    ZeroLength,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AddressNack => write!(f, "address not acknowledged (no device at this address?)"),
            Error::DataNack => write!(f, "data byte not acknowledged"),
            Error::ArbitrationLost => write!(f, "arbitration lost"),
            Error::Overrun => write!(f, "receive overrun"),
            Error::Timeout => write!(f, "timeout (bus stuck or missing pull-ups?)"),
            Error::Abort(src) => write!(f, "transfer aborted, TX_ABRT_SOURCE = {src:#x}"),
            Error::ZeroLength => write!(f, "zero-length operations are not supported"),
        }
    }
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Error::AddressNack => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
            Error::DataNack => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data),
            Error::ArbitrationLost => ErrorKind::ArbitrationLoss,
            Error::Overrun => ErrorKind::Overrun,
            _ => ErrorKind::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// An I2C master.
pub struct I2c<Dm: DriverMode = Blocking> {
    base: usize,
    index: u8,
    timeout: Duration,
    rx_fifo_depth: u32,
    tx_fifo_depth: u32,
    _mode: PhantomData<Dm>,
}

impl<Dm: DriverMode> fmt::Debug for I2c<Dm> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I2c{}", self.index)
    }
}

impl I2c<Blocking> {
    /// Take a controller and program its timing. Pins are attached with
    /// [`I2c::with_sda`] / [`I2c::with_scl`].
    pub fn new<I: Instance>(_i2c: I, config: Config) -> Result<Self, ConfigError> {
        let mut i2c = Self {
            base: I::BASE,
            index: I::INDEX,
            timeout: config.timeout,
            rx_fifo_depth: 0,
            tx_fifo_depth: 0,
            _mode: PhantomData,
        };
        // Sanity check the address: a DesignWare I2C announces itself.
        // SAFETY: read-only ID register.
        let comp_type = unsafe { read32(i2c.base + r::COMP_TYPE) };
        if comp_type != r::COMP_TYPE_VALUE {
            return Err(ConfigError::NotDesignWare { comp_type });
        }
        // SAFETY: read-only parameter register.
        let params = unsafe { read32(i2c.base + r::COMP_PARAM_1) };
        i2c.rx_fifo_depth = r::rx_fifo_depth(params);
        i2c.tx_fifo_depth = r::tx_fifo_depth(params);
        i2c.apply_config(&config)?;
        Ok(i2c)
    }

    /// Switch to the async API.
    pub fn into_async(self) -> I2c<Async> {
        self.convert()
    }
}

impl I2c<Async> {
    /// Switch back to the blocking-only API.
    pub fn into_blocking(self) -> I2c<Blocking> {
        self.convert()
    }

    /// Write `bytes` to `address`.
    pub async fn write_async(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        self.do_transaction(address, &mut [Operation::Write(bytes)]).await
    }

    /// Read `buffer.len()` bytes from `address`.
    pub async fn read_async(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Error> {
        self.do_transaction(address, &mut [Operation::Read(buffer)]).await
    }

    /// Write then read with a RESTART in between (the register-read idiom).
    pub async fn write_read_async(&mut self, address: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<(), Error> {
        self.do_transaction(address, &mut [Operation::Write(bytes), Operation::Read(buffer)]).await
    }

    /// Arbitrary sequence of operations in one START..STOP.
    pub async fn transaction_async(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        self.do_transaction(address, operations).await
    }
}

impl<Dm: DriverMode> I2c<Dm> {
    fn convert<To: DriverMode>(self) -> I2c<To> {
        I2c {
            base: self.base,
            index: self.index,
            timeout: self.timeout,
            rx_fifo_depth: self.rx_fifo_depth,
            tx_fifo_depth: self.tx_fifo_depth,
            _mode: PhantomData,
        }
    }

    /// Attach the SDA pin.
    ///
    /// # Panics
    /// If the pin cannot carry this controller's SDA.
    pub fn with_sda(self, pin: impl Pin) -> Self {
        self.connect(pin, I2cRole::Sda);
        self
    }

    /// Attach the SCL pin.
    pub fn with_scl(self, pin: impl Pin) -> Self {
        self.connect(pin, I2cRole::Scl);
        self
    }

    fn connect(&self, pin: impl Pin, role: I2cRole) {
        let pin: AnyPin = pin.degrade();
        let gpio = pin.number();
        let result = if i2c_role(gpio) != role {
            Err(PinError::WrongRole { gpio })
        } else {
            // Pull-up as a safety net; real buses have their own resistors.
            claim_pin(&pin, Function::I2c(self.index), Pull::Up).map(|_| ())
        };
        if let Err(e) = result {
            panic!("I2C{}: GPIO{} cannot be {:?}: {}", self.index, gpio, role, e);
        }
    }

    /// Re-program speed and timeout.
    pub fn apply_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        let hz = config.frequency.as_hz();
        if hz == 0 || hz > 1_000_000 {
            return Err(ConfigError::FrequencyOutOfRange);
        }
        self.timeout = config.timeout;

        // Minimum high/low times of the I2C spec for the mode we fall in,
        // scaled up proportionally to hit the requested frequency.
        let (h_min, l_min, speed_bits) = if hz <= 100_000 {
            (r::timing::SS_HIGH_NS, r::timing::SS_LOW_NS, r::CON_SPEED_STD)
        } else if hz <= 400_000 {
            (r::timing::FS_HIGH_NS, r::timing::FS_LOW_NS, r::CON_SPEED_FAST)
        } else {
            (260, 500, r::CON_SPEED_FAST)
        };
        let period_ns = 1_000_000_000u64 / u64::from(hz);
        let t_high = (period_ns * u64::from(h_min) / u64::from(h_min + l_min)) as u32;
        let t_low = (period_ns as u32).saturating_sub(t_high);

        let ic_clk_khz = clocks::clk_sys_hz() / 1000;
        let sda_fall = r::timing::DEFAULT_FALL_NS;
        let scl_fall = pi5_pac::rp1::I2C_SCL_FALL_NS;
        let hcnt = r::timing::scl_hcnt(ic_clk_khz, t_high.max(h_min), sda_fall).max(6);
        let lcnt = r::timing::scl_lcnt(ic_clk_khz, t_low.max(l_min), scl_fall).max(8);

        self.disable().map_err(|_| ConfigError::NotIdle)?;
        // SAFETY: we own the controller; it is disabled.
        unsafe {
            let b = self.base;
            // Both register pairs are written; CON.SPEED picks which is used.
            write32(b + r::SS_SCL_HCNT, if speed_bits == r::CON_SPEED_STD { hcnt } else { 4000 });
            write32(b + r::SS_SCL_LCNT, if speed_bits == r::CON_SPEED_STD { lcnt } else { 4700 });
            write32(b + r::FS_SCL_HCNT, if speed_bits == r::CON_SPEED_FAST { hcnt } else { 600 });
            write32(b + r::FS_SCL_LCNT, if speed_bits == r::CON_SPEED_FAST { lcnt } else { 1300 });
            // SDA hold: half the low time (Linux's default when the DT gives none).
            write32(b + r::SDA_HOLD, lcnt / 2);
            write32(b + r::CON, r::CON_MASTER | r::CON_SLAVE_DISABLE | r::CON_RESTART_EN | speed_bits);
            // Thresholds: not used (we poll), but keep them sane.
            write32(b + r::TX_TL, 0);
            write32(b + r::RX_TL, 0);
            write32(b + r::INTR_MASK, 0);
        }
        Ok(())
    }

    // --- blocking API ---

    /// Write `bytes` to `address`.
    pub fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        block_on(self.do_transaction(address, &mut [Operation::Write(bytes)]))
    }

    /// Read `buffer.len()` bytes from `address`.
    pub fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Error> {
        block_on(self.do_transaction(address, &mut [Operation::Read(buffer)]))
    }

    /// Write then read with a RESTART in between.
    pub fn write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<(), Error> {
        block_on(self.do_transaction(address, &mut [Operation::Write(bytes), Operation::Read(buffer)]))
    }

    /// Arbitrary sequence of operations in one START..STOP.
    pub fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        block_on(self.do_transaction(address, operations))
    }

    // --- implementation ---

    /// `ENABLE = 0`, then wait for `ENABLE_STATUS.EN` to clear
    /// (`__i2c_dw_disable`, up to 100 x 25 us).
    fn disable(&mut self) -> Result<(), Error> {
        // SAFETY: we own the controller.
        unsafe {
            write32(self.base + r::ENABLE, 0);
            let t = Timeout::new(Duration::from_millis(5));
            while read32(self.base + r::ENABLE_STATUS) & r::ENABLE_ENABLE != 0 {
                if t.expired() {
                    return Err(Error::Timeout);
                }
            }
        }
        Ok(())
    }

    /// Check for an abort; if there was one, clear it and say why.
    fn check_abort(&self) -> Result<(), Error> {
        // SAFETY: status registers; reading CLR_TX_ABRT clears the flag.
        unsafe {
            let raw = read32(self.base + r::RAW_INTR_STAT);
            if raw & r::INTR_TX_ABRT != 0 {
                let src = read32(self.base + r::TX_ABRT_SOURCE);
                let _ = read32(self.base + r::CLR_TX_ABRT);
                return Err(if src & (r::ABRT_7B_ADDR_NOACK | r::ABRT_10ADDR1_NOACK | r::ABRT_10ADDR2_NOACK) != 0 {
                    Error::AddressNack
                } else if src & r::ABRT_TXDATA_NOACK != 0 {
                    Error::DataNack
                } else if src & r::ABRT_ARB_LOST != 0 {
                    Error::ArbitrationLost
                } else {
                    Error::Abort(src)
                });
            }
            if raw & r::INTR_RX_OVER != 0 {
                let _ = read32(self.base + r::CLR_INTR);
                return Err(Error::Overrun);
            }
        }
        Ok(())
    }

    async fn do_transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        if operations.iter().any(|op| match op {
            Operation::Read(b) => b.is_empty(),
            Operation::Write(b) => b.is_empty(),
        }) {
            return Err(Error::ZeroLength);
        }
        let result = self.run_ops(address, operations).await;
        // Always leave the controller disabled and clean (Linux does the same).
        let _ = self.disable();
        // SAFETY: reading CLR_INTR clears latched status.
        unsafe {
            let _ = read32(self.base + r::CLR_INTR);
        }
        result
    }

    async fn run_ops(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        let b = self.base;
        self.disable()?;
        // SAFETY: we own the controller; it is disabled while TAR is written.
        unsafe {
            write32(b + r::TAR, u32::from(address & 0x7f));
            write32(b + r::ENABLE, r::ENABLE_ENABLE);
            let _ = read32(b + r::CLR_INTR);
        }
        let timeout = Timeout::new(self.timeout);
        let n_ops = operations.len();

        for (i, op) in operations.iter_mut().enumerate() {
            let last_op = i + 1 == n_ops;
            let restart = i > 0; // RESTART before the first byte of every later op
            match op {
                Operation::Write(bytes) => {
                    for (j, &byte) in bytes.iter().enumerate() {
                        self.wait_tx_room(&timeout).await?;
                        let mut cmd = u32::from(byte);
                        if j == 0 && restart {
                            cmd |= r::DATA_CMD_RESTART;
                        }
                        if last_op && j + 1 == bytes.len() {
                            cmd |= r::DATA_CMD_STOP;
                        }
                        // SAFETY: TFNF was just checked.
                        unsafe { write32(b + r::DATA_CMD, cmd) };
                    }
                }
                Operation::Read(buffer) => {
                    let len = buffer.len();
                    let (mut issued, mut received) = (0usize, 0usize);
                    while received < len {
                        // Issue read requests, never more outstanding than the RX FIFO holds.
                        while issued < len && (issued - received) < self.rx_fifo_depth as usize && status(b) & r::STATUS_TFNF != 0 {
                            let mut cmd = r::DATA_CMD_READ;
                            if issued == 0 && restart {
                                cmd |= r::DATA_CMD_RESTART;
                            }
                            if last_op && issued + 1 == len {
                                cmd |= r::DATA_CMD_STOP;
                            }
                            // SAFETY: TFNF was just checked.
                            unsafe { write32(b + r::DATA_CMD, cmd) };
                            issued += 1;
                        }
                        // Drain what has arrived.
                        let mut progressed = false;
                        // SAFETY: RXFLR is a count; DATA_CMD pops one byte per read.
                        unsafe {
                            while received < len && read32(b + r::RXFLR) > 0 {
                                buffer[received] = (read32(b + r::DATA_CMD) & r::DATA_CMD_DAT_MASK) as u8;
                                received += 1;
                                progressed = true;
                            }
                        }
                        self.check_abort()?;
                        if timeout.expired() {
                            return Err(Error::Timeout);
                        }
                        if !progressed {
                            yield_now().await;
                        }
                    }
                }
            }
        }

        // Wait for the STOP to go out (or the master to go idle).
        loop {
            self.check_abort()?;
            // SAFETY: status reads.
            let (raw, st) = unsafe { (read32(b + r::RAW_INTR_STAT), status(b)) };
            if raw & r::INTR_STOP_DET != 0 || (st & r::STATUS_TFE != 0 && st & r::STATUS_MASTER_ACTIVITY == 0) {
                break;
            }
            if timeout.expired() {
                return Err(Error::Timeout);
            }
            yield_now().await;
        }
        Ok(())
    }

    /// Wait for room in the command FIFO, watching for aborts and the timeout.
    async fn wait_tx_room(&mut self, timeout: &Timeout) -> Result<(), Error> {
        loop {
            if status(self.base) & r::STATUS_TFNF != 0 {
                return Ok(());
            }
            self.check_abort()?;
            if timeout.expired() {
                return Err(Error::Timeout);
            }
            yield_now().await;
        }
    }
}

#[inline]
fn status(base: usize) -> u32 {
    // SAFETY: STATUS is read-only.
    unsafe { read32(base + r::STATUS) }
}

impl<Dm: DriverMode> ErrorType for I2c<Dm> {
    type Error = Error;
}

impl<Dm: DriverMode> embedded_hal::i2c::I2c<SevenBitAddress> for I2c<Dm> {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        I2c::transaction(self, address, operations)
    }
}
