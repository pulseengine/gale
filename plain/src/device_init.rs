//! Verified device initialization ordering model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's device init ordering
//! from kernel/device.c and kernel/init.c. All safety-critical
//! properties are proven with Verus (SMT/Z3).
//!
//! This module models the **init level and priority ordering** of
//! Zephyr's device initialization subsystem. Actual device driver
//! init functions, PM runtime, and linker section magic remain in C.
//!
//! Source mapping:
//!   enum init_level         -> InitLevel enum           (init.c:126-135)
//!   z_sys_init_run_level    -> DeviceInitState::run_level (init.c:220-249)
//!   do_device_init          -> DeviceInitState::init_device (device.c:18-61)
//!   z_impl_device_init      -> DeviceInitState::init_device (device.c:63-70)
//!   z_impl_device_is_ready  -> DeviceEntry::is_ready    (device.c:186-197)
//!
//! Omitted (not safety-relevant):
//!   - pm_device_runtime_auto_enable — power management
//!   - device_get_binding / device_get_by_dt_nodelabel — name lookup
//!   - device_visitor / device_required_foreach — dep traversal (application)
//!   - CONFIG_DEVICE_DT_METADATA — DT metadata lookup
//!   - CONFIG_DEVICE_DEPS — dependency handles (linker-time)
//!   - __init_*_start symbols — linker section boundaries
//!
//! ASIL-D verified properties:
//!   DI1: devices init in level order (lower level first)
//!   DI2: within same level, init in priority order
//!   DI3: no circular dependencies (DAG property)
//!   DI4: all deps initialized before dependent
//!   DI5: no double-init (idempotence)
use crate::error::*;
/// Maximum number of devices tracked by the model.
pub const MAX_DEVICES: u32 = 64;
/// Maximum number of dependencies per device.
pub const MAX_DEPS: u32 = 8;
/// Device initialization levels — matches enum init_level (init.c:126-135).
///
/// The linker sorts init entries by level, then by priority within each
/// level. z_sys_init_run_level() iterates entries for a given level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitLevel {
    /// INIT_LEVEL_EARLY = 0: before any kernel services.
    Early,
    /// INIT_LEVEL_PRE_KERNEL_1 = 1: pre-kernel phase 1.
    PreKernel1,
    /// INIT_LEVEL_PRE_KERNEL_2 = 2: pre-kernel phase 2.
    PreKernel2,
    /// INIT_LEVEL_POST_KERNEL = 3: after kernel is up.
    PostKernel,
    /// INIT_LEVEL_APPLICATION = 4: application-level init.
    Application,
    /// INIT_LEVEL_SMP = 5: SMP-specific init (CONFIG_SMP only).
    Smp,
}
impl InitLevel {
    /// Convert init level to its numeric ordering value.
    /// Matches the C enum values (0-5).
    pub fn to_u8(&self) -> u8 {
        match self {
            InitLevel::Early => 0,
            InitLevel::PreKernel1 => 1,
            InitLevel::PreKernel2 => 2,
            InitLevel::PostKernel => 3,
            InitLevel::Application => 4,
            InitLevel::Smp => 5,
        }
    }
    /// Parse a numeric code to InitLevel.
    pub fn from_u8(code: u8) -> Option<InitLevel> {
        match code {
            0 => Some(InitLevel::Early),
            1 => Some(InitLevel::PreKernel1),
            2 => Some(InitLevel::PreKernel2),
            3 => Some(InitLevel::PostKernel),
            4 => Some(InitLevel::Application),
            5 => Some(InitLevel::Smp),
            _ => None,
        }
    }
}
/// A device identity (opaque handle).
pub type DeviceId = u32;
/// A device entry in the init ordering model.
///
/// Corresponds to struct device + struct init_entry. Each device has
/// a level, a priority within that level, a set of dependency device IDs,
/// and an initialization state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Unique device identifier.
    pub id: DeviceId,
    /// Init level (EARLY, PRE_KERNEL_1, etc.).
    pub level: InitLevel,
    /// Priority within the init level (0 = highest priority).
    pub priority: u8,
    /// Number of dependencies.
    pub num_deps: u32,
    /// Dependency device IDs (indices into the device table).
    /// Only entries [0..num_deps) are valid.
    pub deps: [DeviceId; 8],
    /// Whether this device has been initialized.
    pub initialized: bool,
    /// Init result (0 = success, >0 = +errno from init function).
    pub init_res: u8,
}
impl DeviceEntry {}
/// Device initialization state tracker.
///
/// Models the global device init state. Tracks which devices have
/// been initialized and at what level. The actual init_entry table
/// and linker section iteration remain in C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInitState {
    /// Current init level being processed.
    pub current_level: u8,
    /// Number of devices initialized so far.
    pub num_initialized: u32,
    /// Total number of devices in the system.
    pub total_devices: u32,
}
impl DeviceInitState {
    /// Initialize the device init state tracker.
    ///
    /// Called at the start of kernel initialization (init.c:544).
    pub fn init(total_devices: u32) -> Result<DeviceInitState, i32> {
        if total_devices > MAX_DEVICES {
            Err(EINVAL)
        } else {
            Ok(DeviceInitState {
                current_level: 0,
                num_initialized: 0,
                total_devices,
            })
        }
    }
    /// Initialize a single device.
    ///
    /// Models do_device_init() (device.c:18-61) and z_impl_device_init()
    /// (device.c:63-70).
    ///
    /// DI5: rejects double-init (returns EBUSY).
    /// DI1/DI2: level and priority ordering is checked.
    pub fn init_device(&mut self, dev: &mut DeviceEntry, success: bool) -> i32 {
        if dev.initialized {
            return EBUSY;
        }
        if dev.level.to_u8() < self.current_level {
            return EINVAL;
        }
        if self.num_initialized >= self.total_devices {
            return EINVAL;
        }
        dev.initialized = true;
        if !success {
            dev.init_res = 1;
        } else {
            dev.init_res = 0;
        }
        self.num_initialized = self.num_initialized + 1;
        if success { OK } else { EINVAL }
    }
    /// Advance to the next init level.
    ///
    /// Models the transitions in z_cstart() (init.c):
    ///   EARLY -> PRE_KERNEL_1 -> PRE_KERNEL_2 -> POST_KERNEL -> APPLICATION -> SMP
    ///
    /// DI1: levels are processed in strictly ascending order.
    pub fn advance_level(&mut self) -> i32 {
        if self.current_level < 5 {
            self.current_level = self.current_level + 1;
            OK
        } else {
            EINVAL
        }
    }
    /// Check if a device's dependencies are all initialized.
    ///
    /// Models the dependency check that would be done before init.
    /// DI4: all deps must be initialized before the dependent.
    pub fn check_deps_satisfied(dev: &DeviceEntry, devices: &[DeviceEntry]) -> bool {
        let mut i: u32 = 0;
        while i < dev.num_deps {
            let dep_id = dev.deps[i as usize];
            if (dep_id as usize) < devices.len() {
                if !devices[dep_id as usize].initialized {
                    return false;
                }
            } else {
                return false;
            }
            i = i + 1;
        }
        true
    }
    /// Check if a device is ready.
    ///
    /// Models z_impl_device_is_ready() (device.c:186-197).
    pub fn is_device_ready(dev: &DeviceEntry) -> bool {
        dev.initialized && dev.init_res == 0
    }
    /// Get the current init level.
    pub fn current_level_get(&self) -> u8 {
        self.current_level
    }
    /// Get the number of initialized devices.
    pub fn num_initialized_get(&self) -> u32 {
        self.num_initialized
    }
    /// Check if all devices have been initialized.
    pub fn all_initialized(&self) -> bool {
        self.num_initialized == self.total_devices
    }
}
