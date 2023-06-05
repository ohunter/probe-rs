use crate::{error, CoreType, Error, InstructionSet, MemoryInterface};
use anyhow::{anyhow, Result};
pub use probe_rs_target::{Architecture, CoreAccessOptions};
use std::time::Duration;

pub mod core_state;
pub mod core_status;
pub mod memory_mapped_registers;
pub mod registers;

pub use core_state::*;
pub use core_status::*;
pub use memory_mapped_registers::MemoryMappedRegister;
pub use registers::*;

/// An struct for storing the current state of a core.
#[derive(Debug, Clone)]
pub struct CoreInformation {
    /// The current Program Counter.
    pub pc: u64,
}

/// A generic interface to control a MCU core.
pub trait CoreInterface: MemoryInterface {
    /// Wait until the core is halted. If the core does not halt on its own,
    /// a [`DebugProbeError::Timeout`](crate::DebugProbeError::Timeout) error will be returned.
    fn wait_for_core_halted(&mut self, timeout: Duration) -> Result<(), error::Error>;

    /// Check if the core is halted. If the core does not halt on its own,
    /// a [`DebugProbeError::Timeout`](crate::DebugProbeError::Timeout) error will be returned.
    fn core_halted(&mut self) -> Result<bool, error::Error>;

    /// Returns the current status of the core.
    fn status(&mut self) -> Result<CoreStatus, error::Error>;

    /// Try to halt the core. This function ensures the core is actually halted, and
    /// returns a [`DebugProbeError::Timeout`](crate::DebugProbeError::Timeout) otherwise.
    fn halt(&mut self, timeout: Duration) -> Result<CoreInformation, error::Error>;

    /// Continue to execute instructions.
    fn run(&mut self) -> Result<(), error::Error>;

    /// Reset the core, and then continue to execute instructions. If the core
    /// should be halted after reset, use the [`reset_and_halt`] function.
    ///
    /// [`reset_and_halt`]: Core::reset_and_halt
    fn reset(&mut self) -> Result<(), error::Error>;

    /// Reset the core, and then immediately halt. To continue execution after
    /// reset, use the [`reset`] function.
    ///
    /// [`reset`]: Core::reset
    fn reset_and_halt(&mut self, timeout: Duration) -> Result<CoreInformation, error::Error>;

    /// Steps one instruction and then enters halted state again.
    fn step(&mut self) -> Result<CoreInformation, error::Error>;

    /// Read the value of a core register.
    fn read_core_reg(
        &mut self,
        address: registers::RegisterId,
    ) -> Result<registers::RegisterValue, error::Error>;

    /// Write the value of a core register.
    fn write_core_reg(
        &mut self,
        address: registers::RegisterId,
        value: registers::RegisterValue,
    ) -> Result<(), error::Error>;

    /// Returns all the available breakpoint units of the core.
    fn available_breakpoint_units(&mut self) -> Result<u32, error::Error>;

    /// Read the hardware breakpoints from FpComp registers, and adds them to the Result Vector.
    /// A value of None in any position of the Vector indicates that the position is unset/available.
    /// We intentionally return all breakpoints, irrespective of whether they are enabled or not.
    fn hw_breakpoints(&mut self) -> Result<Vec<Option<u64>>, error::Error>;

    /// Enables breakpoints on this core. If a breakpoint is set, it will halt as soon as it is hit.
    fn enable_breakpoints(&mut self, state: bool) -> Result<(), error::Error>;

    /// Sets a breakpoint at `addr`. It does so by using unit `bp_unit_index`.
    fn set_hw_breakpoint(&mut self, unit_index: usize, addr: u64) -> Result<(), error::Error>;

    /// Clears the breakpoint configured in unit `unit_index`.
    fn clear_hw_breakpoint(&mut self, unit_index: usize) -> Result<(), error::Error>;

    /// Returns a list of all the registers of this core.
    fn registers(&self) -> &'static registers::RegisterFile;

    /// Returns `true` if hwardware breakpoints are enabled, `false` otherwise.
    fn hw_breakpoints_enabled(&self) -> bool;

    /// Configure the target to ensure software breakpoints will enter Debug Mode.
    fn debug_on_sw_breakpoint(&mut self, _enabled: bool) -> Result<(), error::Error> {
        // This default will have override methods for architectures that require special behavior, e.g. RISV-V.
        Ok(())
    }

    /// Get the `Architecture` of the Core.
    fn architecture(&self) -> Architecture;

    /// Get the `CoreType` of the Core
    fn core_type(&self) -> CoreType;

    /// Determine the instruction set the core is operating in
    /// This must be queried while halted as this is a runtime
    /// decision for some core types
    fn instruction_set(&mut self) -> Result<InstructionSet, error::Error>;

    /// Determine if an FPU is present.
    /// This must be queried while halted as this is a runtime
    /// decision for some core types.
    fn fpu_support(&mut self) -> Result<bool, error::Error>;

    /// Called during session stop to do any pending cleanup
    fn on_session_stop(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl<'probe> MemoryInterface for Core<'probe> {
    fn supports_native_64bit_access(&mut self) -> bool {
        self.inner.supports_native_64bit_access()
    }

    fn read_word_64(&mut self, address: u64) -> Result<u64, Error> {
        self.inner.read_word_64(address)
    }

    fn read_word_32(&mut self, address: u64) -> Result<u32, Error> {
        self.inner.read_word_32(address)
    }

    fn read_word_8(&mut self, address: u64) -> Result<u8, Error> {
        self.inner.read_word_8(address)
    }

    fn read_64(&mut self, address: u64, data: &mut [u64]) -> Result<(), Error> {
        self.inner.read_64(address, data)
    }

    fn read_32(&mut self, address: u64, data: &mut [u32]) -> Result<(), Error> {
        self.inner.read_32(address, data)
    }

    fn read_8(&mut self, address: u64, data: &mut [u8]) -> Result<(), Error> {
        self.inner.read_8(address, data)
    }

    fn read(&mut self, address: u64, data: &mut [u8]) -> Result<(), Error> {
        self.inner.read(address, data)
    }

    fn write_word_64(&mut self, addr: u64, data: u64) -> Result<(), Error> {
        self.inner.write_word_64(addr, data)
    }

    fn write_word_32(&mut self, addr: u64, data: u32) -> Result<(), Error> {
        self.inner.write_word_32(addr, data)
    }

    fn write_word_8(&mut self, addr: u64, data: u8) -> Result<(), Error> {
        self.inner.write_word_8(addr, data)
    }

    fn write_64(&mut self, addr: u64, data: &[u64]) -> Result<(), Error> {
        self.inner.write_64(addr, data)
    }

    fn write_32(&mut self, addr: u64, data: &[u32]) -> Result<(), Error> {
        self.inner.write_32(addr, data)
    }

    fn write_8(&mut self, addr: u64, data: &[u8]) -> Result<(), Error> {
        self.inner.write_8(addr, data)
    }

    fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), Error> {
        self.inner.write(addr, data)
    }

    fn supports_8bit_transfers(&self) -> Result<bool, error::Error> {
        self.inner.supports_8bit_transfers()
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.inner.flush()
    }
}

/// Generic core handle representing a physical core on an MCU.
///
/// This should be considere as a temporary view of the core which locks the debug probe driver to as single consumer by borrowing it.
///
/// As soon as you did your atomic task (e.g. halt the core, read the core state and all other debug relevant info) you should drop this object,
/// to allow potential other shareholders of the session struct to grab a core handle too.
pub struct Core<'probe> {
    inner: Box<dyn CoreInterface + 'probe>,
    state: &'probe mut CoreState,
}

impl<'probe> Core<'probe> {
    /// Create a new [`Core`].
    pub fn new(core: impl CoreInterface + 'probe, state: &'probe mut CoreState) -> Core<'probe> {
        Self {
            inner: Box::new(core),
            state,
        }
    }

    /// Creates a new [`CoreState`]
    pub fn create_state(id: usize, options: CoreAccessOptions) -> CoreState {
        CoreState::new(id, options)
    }

    /// Returns the ID of this core.
    pub fn id(&self) -> usize {
        self.state.id()
    }

    /// Wait until the core is halted. If the core does not halt on its own,
    /// a [`DebugProbeError::Timeout`](crate::DebugProbeError::Timeout) error will be returned.
    #[tracing::instrument(skip(self))]
    pub fn wait_for_core_halted(&mut self, timeout: Duration) -> Result<(), error::Error> {
        self.inner.wait_for_core_halted(timeout)
    }

    /// Check if the core is halted. If the core does not halt on its own,
    /// a [`DebugProbeError::Timeout`](crate::DebugProbeError::Timeout) error will be returned.
    pub fn core_halted(&mut self) -> Result<bool, error::Error> {
        self.inner.core_halted()
    }

    /// Try to halt the core. This function ensures the core is actually halted, and
    /// returns a [`DebugProbeError::Timeout`](crate::DebugProbeError::Timeout) otherwise.
    #[tracing::instrument(skip(self))]
    pub fn halt(&mut self, timeout: Duration) -> Result<CoreInformation, error::Error> {
        self.inner.halt(timeout)
    }

    /// Continue to execute instructions.
    #[tracing::instrument(skip(self))]
    pub fn run(&mut self) -> Result<(), error::Error> {
        self.inner.run()
    }

    /// Reset the core, and then continue to execute instructions. If the core
    /// should be halted after reset, use the [`reset_and_halt`] function.
    ///
    /// [`reset_and_halt`]: Core::reset_and_halt
    #[tracing::instrument(skip(self))]
    pub fn reset(&mut self) -> Result<(), error::Error> {
        self.inner.reset()
    }

    /// Reset the core, and then immediately halt. To continue execution after
    /// reset, use the [`reset`] function.
    ///
    /// [`reset`]: Core::reset
    #[tracing::instrument(skip(self))]
    pub fn reset_and_halt(&mut self, timeout: Duration) -> Result<CoreInformation, error::Error> {
        self.inner.reset_and_halt(timeout)
    }

    /// Steps one instruction and then enters halted state again.
    #[tracing::instrument(skip(self))]
    pub fn step(&mut self) -> Result<CoreInformation, error::Error> {
        self.inner.step()
    }

    /// Returns the current status of the core.
    #[tracing::instrument(skip(self))]
    pub fn status(&mut self) -> Result<CoreStatus, error::Error> {
        self.inner.status()
    }

    /// Read the value of a core register.
    ///
    /// # Remarks
    ///
    /// `T` can be an unsigned interger type, such as [u32] or [u64], or
    /// it can be [RegisterValue] to allow the caller to support arbitrary
    /// length registers.
    ///
    /// To add support to convert to a custom type implement [`TryInto<CustomType>`]
    /// for [RegisterValue]].
    ///
    /// # Errors
    ///
    /// If `T` isn't large enough to hold the register value an error will be raised.
    #[tracing::instrument(skip(self, address), fields(address))]
    pub fn read_core_reg<T>(
        &mut self,
        address: impl Into<registers::RegisterId>,
    ) -> Result<T, error::Error>
    where
        registers::RegisterValue: TryInto<T>,
        Result<T, <registers::RegisterValue as TryInto<T>>::Error>: RegisterValueResultExt<T>,
    {
        let address = address.into();

        tracing::Span::current().record("address", format!("{address:?}"));

        let value = self.inner.read_core_reg(address)?;

        value.try_into().into_crate_error()
    }

    /// Write the value of a core register.
    ///
    /// # Errors
    ///
    /// If T is too large to write to the target register an error will be raised.
    #[tracing::instrument(skip(self, value))]
    pub fn write_core_reg<T>(
        &mut self,
        address: registers::RegisterId,
        value: T,
    ) -> Result<(), error::Error>
    where
        T: Into<registers::RegisterValue>,
    {
        self.inner.write_core_reg(address, value.into())
    }

    /// Returns all the available breakpoint units of the core.
    pub fn available_breakpoint_units(&mut self) -> Result<u32, error::Error> {
        self.inner.available_breakpoint_units()
    }

    /// Enables breakpoints on this core. If a breakpoint is set, it will halt as soon as it is hit.
    fn enable_breakpoints(&mut self, state: bool) -> Result<(), error::Error> {
        self.inner.enable_breakpoints(state)
    }

    /// Configure the debug module to ensure software breakpoints will enter Debug Mode.
    #[tracing::instrument(skip(self))]
    pub fn debug_on_sw_breakpoint(&mut self, enabled: bool) -> Result<(), error::Error> {
        self.inner.debug_on_sw_breakpoint(enabled)
    }

    /// Returns a list of all the registers of this core.
    pub fn registers(&self) -> &'static registers::RegisterFile {
        self.inner.registers()
    }

    /// Find the index of the next available HW breakpoint comparator.
    fn find_free_breakpoint_comparator_index(&mut self) -> Result<usize, error::Error> {
        let mut next_available_hw_breakpoint = 0;
        for breakpoint in self.inner.hw_breakpoints()? {
            if breakpoint.is_none() {
                return Ok(next_available_hw_breakpoint);
            } else {
                next_available_hw_breakpoint += 1;
            }
        }
        Err(error::Error::Other(anyhow!(
            "No available hardware breakpoints"
        )))
    }

    /// Set a hardware breakpoint
    ///
    /// This function will try to set a hardware breakpoint att `address`.
    ///
    /// The amount of hardware breakpoints which are supported is chip specific,
    /// and can be queried using the `get_available_breakpoint_units` function.
    #[tracing::instrument(skip(self))]
    pub fn set_hw_breakpoint(&mut self, address: u64) -> Result<(), error::Error> {
        if !self.inner.hw_breakpoints_enabled() {
            self.enable_breakpoints(true)?;
        }

        // If there is a breakpoint set already, return its bp_unit_index, else find the next free index.
        let breakpoint_comparator_index = match self
            .inner
            .hw_breakpoints()?
            .iter()
            .position(|&bp| bp == Some(address))
        {
            Some(breakpoint_comparator_index) => breakpoint_comparator_index,
            None => self.find_free_breakpoint_comparator_index()?,
        };

        tracing::debug!(
            "Trying to set HW breakpoint #{} with comparator address  {:#08x}",
            breakpoint_comparator_index,
            address
        );

        // Actually set the breakpoint. Even if it has been set, set it again so it will be active.
        self.inner
            .set_hw_breakpoint(breakpoint_comparator_index, address)?;
        Ok(())
    }

    /// Set a hardware breakpoint
    ///
    /// This function will try to clear a hardware breakpoint at `address` if there exists a breakpoint at that address.
    #[tracing::instrument(skip(self))]
    pub fn clear_hw_breakpoint(&mut self, address: u64) -> Result<(), error::Error> {
        let bp_position = self
            .inner
            .hw_breakpoints()?
            .iter()
            .position(|bp| bp.is_some() && bp.unwrap() == address);

        tracing::debug!(
            "Will clear HW breakpoint    #{} with comparator address    {:#08x}",
            bp_position.unwrap_or(usize::MAX),
            address
        );

        match bp_position {
            Some(bp_position) => {
                self.inner.clear_hw_breakpoint(bp_position)?;
                Ok(())
            }
            None => Err(error::Error::Other(anyhow!(
                "No breakpoint found at address {:#010x}",
                address
            ))),
        }
    }

    /// Clear all hardware breakpoints
    ///
    /// This function will clear all HW breakpoints which are configured on the target,
    /// regardless if they are set by probe-rs, AND regardless if they are enabled or not.
    /// Also used as a helper function in [`Session::drop`](crate::session::Session).
    #[tracing::instrument(skip(self))]
    pub fn clear_all_hw_breakpoints(&mut self) -> Result<(), error::Error> {
        for breakpoint in (self.inner.hw_breakpoints()?).into_iter().flatten() {
            self.clear_hw_breakpoint(breakpoint)?
        }
        Ok(())
    }

    /// Returns the architecture of the core.
    pub fn architecture(&self) -> Architecture {
        self.inner.architecture()
    }

    /// Returns the core type of the core
    pub fn core_type(&self) -> CoreType {
        self.inner.core_type()
    }

    /// Determine the instruction set the core is operating in
    /// This must be queried while halted as this is a runtime
    /// decision for some core types
    pub fn instruction_set(&mut self) -> Result<InstructionSet, error::Error> {
        self.inner.instruction_set()
    }

    /// Determine if an FPU is present.
    /// This must be queried while halted as this is a runtime
    /// decision for some core types.
    pub fn fpu_support(&mut self) -> Result<bool, error::Error> {
        self.inner.fpu_support()
    }

    /// Called during session tear down to do any pending cleanup
    #[tracing::instrument(skip(self))]
    pub(crate) fn on_session_stop(&mut self) -> Result<(), Error> {
        self.inner.on_session_stop()
    }
}
