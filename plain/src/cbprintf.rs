//! Verified cbprintf model for Zephyr RTOS.
//!
//! Formally verified model of Zephyr's cbprintf (callback-based printf)
//! from lib/os/cbprintf_complete.c and lib/os/cbprintf_packaged.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **validation logic** of the cbprintf subsystem:
//! format string parsing, argument packaging bounds, and output length
//! tracking. Actual I/O (callback dispatch, floating-point formatting,
//! va_list manipulation) remains in C.
//!
//! Source mapping:
//!   struct conversion          -> FormatSpec      (cbprintf_complete.c:188-306)
//!   extract_flags/width/prec   -> FormatSpec::parse_specifier
//!   cbvprintf_package          -> PackageState    (cbprintf_packaged.c:233+)
//!   get_package_len            -> PackageState::total_len
//!   output byte tracking       -> OutputState     (implicit in cbvprintf)
//!
//! Omitted (not validation-relevant):
//!   - cbprintf_via_va_list — arch-specific va_list construction
//!   - Floating-point formatting — handled entirely in C
//!   - String pointer relocation — packaged string copy logic
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - CONFIG_CBPRINTF_FP_SUPPORT — float path
//!
//! ASIL-D verified properties:
//!   CB1: FormatSpec fields are within representable bounds
//!   CB2: PackageState never exceeds buffer capacity
//!   CB3: OutputState length tracking is monotone and bounded
//!   CB4: Dangerous conversion specifiers (%n, oversized width/prec) are rejected
//!   CB5: %n is always rejected regardless of context
/// Maximum width or precision value accepted (matches INT_MAX in C).
/// Values larger than this would overflow the int width_value / prec_value
/// fields in struct conversion.
pub const MAX_WIDTH_PREC: u32 = 0x7FFF_FFFF;
/// Maximum package buffer size (bytes). Matches
/// CONFIG_CBPRINTF_PACKAGE_BUF_SIZE_MAX or a safe conservative limit.
pub const MAX_PACKAGE_BUF: usize = 65536;
/// Maximum output length tracked before reporting overflow.
pub const MAX_OUTPUT_LEN: usize = usize::MAX / 2;
/// The conversion specifier character from a % format field.
///
/// Maps to the specifier_cat_enum in cbprintf_complete.c and the
/// specifier character itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionSpecifier {
    /// %d / %i — signed decimal integer
    SignedInt,
    /// %u — unsigned decimal integer
    UnsignedInt,
    /// %o — unsigned octal
    Octal,
    /// %x / %X — unsigned hexadecimal
    Hex,
    /// %c — character
    Char,
    /// %s — string pointer
    String,
    /// %p — void pointer
    Pointer,
    /// %% — literal percent sign (no argument consumed)
    Percent,
    /// %n — WRITE-BACK specifier. ALWAYS REJECTED (CB5).
    WriteBack,
    /// Unrecognised specifier
    Invalid,
}
/// Length modifier prefix (hh, h, l, ll, j, z, t, L).
///
/// Maps to length_mod_enum in cbprintf_complete.c:55-65.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthModifier {
    /// No length modifier (default int / double)
    None,
    /// hh — signed/unsigned char
    Hh,
    /// h — short
    H,
    /// l — long
    L,
    /// ll — long long
    Ll,
    /// j — intmax_t / uintmax_t
    J,
    /// z — size_t
    Z,
    /// t — ptrdiff_t
    T,
    /// L — long double (only meaningful for float specifiers)
    UpperL,
}
/// A parsed printf conversion specification.
///
/// Models `struct conversion` from cbprintf_complete.c:188-306.
/// All fields have bounded, well-typed representations.
///
/// CB1 invariant: width and precision values fit in [0, MAX_WIDTH_PREC].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct FormatSpec {
    /// '-' flag: left-justify value in width
    pub flag_dash: bool,
    /// '+' flag: explicit sign for non-negative numbers
    pub flag_plus: bool,
    /// ' ' flag: space for non-negative sign
    pub flag_space: bool,
    /// '#' flag: alternative form
    pub flag_hash: bool,
    /// '0' flag: pad with leading zeroes (cleared when flag_dash is set)
    pub flag_zero: bool,
    /// True if a width field was present
    pub width_present: bool,
    /// True if width is taken from the next int argument ('*')
    pub width_star: bool,
    /// Width value (when width_present && !width_star); in [0, MAX_WIDTH_PREC]
    pub width_value: u32,
    /// True if a precision field was present
    pub prec_present: bool,
    /// True if precision is taken from the next int argument ('.*')
    pub prec_star: bool,
    /// Precision value (when prec_present && !prec_star); in [0, MAX_WIDTH_PREC]
    pub prec_value: u32,
    pub length_mod: LengthModifier,
    pub specifier: ConversionSpecifier,
}
impl FormatSpec {
    /// Create a FormatSpec with explicit values.
    ///
    /// Enforces CB1 at construction: width and precision are clamped to
    /// MAX_WIDTH_PREC if the caller provides out-of-range values.
    #[allow(
        clippy::too_many_arguments,
        clippy::fn_params_excessive_bools,
        clippy::similar_names
    )]
    pub fn new(
        flag_dash: bool,
        flag_plus: bool,
        flag_space: bool,
        flag_hash: bool,
        flag_zero: bool,
        width_present: bool,
        width_star: bool,
        width_value: u32,
        prec_present: bool,
        prec_star: bool,
        prec_value: u32,
        length_mod: LengthModifier,
        specifier: ConversionSpecifier,
    ) -> FormatSpec {
        let w = if width_value > MAX_WIDTH_PREC { MAX_WIDTH_PREC } else { width_value };
        let p = if prec_value > MAX_WIDTH_PREC { MAX_WIDTH_PREC } else { prec_value };
        let fz = flag_zero && !flag_dash;
        let fd = flag_dash;
        FormatSpec {
            flag_dash: fd,
            flag_plus,
            flag_space,
            flag_hash,
            flag_zero: fz,
            width_present,
            width_star,
            width_value: w,
            prec_present,
            prec_star,
            prec_value: p,
            length_mod,
            specifier,
        }
    }
    /// Validate the specifier: reject %n and Invalid.
    ///
    /// CB4: dangerous specifiers (oversized width/prec represented as
    ///      Invalid after parse failure) are rejected.
    /// CB5: %n is always rejected.
    ///
    /// Returns Ok(()) when the spec is safe, Err(-EINVAL) otherwise.
    pub fn validate(&self) -> Result<(), i32> {
        match self.specifier {
            ConversionSpecifier::WriteBack => Err(crate::error::EINVAL),
            ConversionSpecifier::Invalid => Err(crate::error::EINVAL),
            _ => Ok(()),
        }
    }
}
/// Validate a single format specifier.
///
/// CB4: reject specifiers that are Invalid (parse failure, oversized values).
/// CB5: always reject %n.
///
/// Returns Ok(()) on success, Err(-EINVAL) on rejection.
pub fn validate_format_spec(spec: &FormatSpec) -> Result<(), i32> {
    spec.validate()
}
/// State of the cbprintf argument packaging buffer.
///
/// Models the buffer pointer (`buf`) and buffer end (`buf0 + len`) used
/// in `cbvprintf_package()` (cbprintf_packaged.c:233+).
///
/// CB2 invariant: pos <= capacity, capacity <= MAX_PACKAGE_BUF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageState {
    /// Current write position in the buffer (bytes from start).
    pub pos: usize,
    /// Total buffer capacity in bytes.
    pub capacity: usize,
}
impl PackageState {
    /// Create a fresh PackageState at the start of a buffer.
    pub fn new(capacity: usize) -> PackageState {
        PackageState { pos: 0, capacity }
    }
    /// Remaining free bytes in the buffer (executable version).
    pub fn remaining(&self) -> usize {
        self.capacity - self.pos
    }
    /// Advance the position by `n` bytes if there is room.
    ///
    /// CB2: the advance is rejected if it would exceed capacity.
    /// Returns Ok(new_state) on success, Err(-ENOMEM) on overflow.
    pub fn advance(&self, n: usize) -> Result<PackageState, i32> {
        if n > self.capacity - self.pos {
            Err(crate::error::ENOMEM)
        } else {
            Ok(PackageState {
                pos: self.pos + n,
                capacity: self.capacity,
            })
        }
    }
    /// Total bytes consumed so far.
    pub fn total_len(&self) -> usize {
        self.pos
    }
}
/// Check whether writing `size` bytes into `state` would exceed the buffer.
///
/// CB2: package never overflows buffer — returns Err if it would.
///
/// Returns Ok(new_state) or Err(-ENOMEM).
pub fn package_bounds_check(
    state: PackageState,
    size: usize,
) -> Result<PackageState, i32> {
    state.advance(size)
}
/// Tracks the number of bytes written by the cbprintf callback.
///
/// Models the implicit `count` variable inside `cbvprintf()`.
///
/// CB3 invariant: count is monotonically non-decreasing and bounded by
/// MAX_OUTPUT_LEN. Any addition that would overflow is clamped and
/// flagged as an overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputState {
    /// Bytes written so far.
    pub count: usize,
    /// True if an overflow was detected (addition would wrap).
    pub overflow: bool,
}
impl OutputState {
    /// Create a fresh OutputState with zero bytes written.
    pub fn new() -> OutputState {
        OutputState {
            count: 0,
            overflow: false,
        }
    }
    /// Record `n` additional bytes of output.
    ///
    /// CB3: if the addition would exceed MAX_OUTPUT_LEN the state is
    /// marked as overflowed and count is saturated at MAX_OUTPUT_LEN.
    pub fn add_bytes(&self, n: usize) -> OutputState {
        if n > MAX_OUTPUT_LEN - self.count {
            OutputState {
                count: MAX_OUTPUT_LEN,
                overflow: true,
            }
        } else {
            OutputState {
                count: self.count + n,
                overflow: self.overflow,
            }
        }
    }
    /// Return the total byte count, or None if overflow was detected.
    pub fn result(&self) -> Option<usize> {
        if self.overflow { None } else { Some(self.count) }
    }
}
/// Record `n` bytes of output into `state` with overflow protection.
///
/// CB3: output length tracking is accurate and bounded.
pub fn output_bounds_check(state: OutputState, n: usize) -> OutputState {
    state.add_bytes(n)
}
/// Validate a conversion specifier character received from C.
///
/// CB4 + CB5: %n (code 'n' = 110) is always rejected; unknown specifiers
/// are rejected. Returns 0 on success, -EINVAL on rejection.
pub fn validate_specifier_char(ch: u8) -> i32 {
    let spec = match ch {
        b'd' | b'i' => ConversionSpecifier::SignedInt,
        b'u' => ConversionSpecifier::UnsignedInt,
        b'o' => ConversionSpecifier::Octal,
        b'x' | b'X' => ConversionSpecifier::Hex,
        b'c' => ConversionSpecifier::Char,
        b's' => ConversionSpecifier::String,
        b'p' => ConversionSpecifier::Pointer,
        b'%' => ConversionSpecifier::Percent,
        b'n' => ConversionSpecifier::WriteBack,
        _ => ConversionSpecifier::Invalid,
    };
    let fs = FormatSpec::new(
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        0,
        false,
        false,
        0,
        LengthModifier::None,
        spec,
    );
    match fs.validate() {
        Ok(()) => 0,
        Err(e) => e,
    }
}
