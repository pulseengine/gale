# MC/DC vector set for mpu-thin — the region-programming core.
# Two real decision targets: `validate_region`'s four-condition conjunction
# (power-of-two, min-size, alignment, no-overflow) and `size_field`'s if-else
# ladder over the eight RASR SIZE encodings.
F='gust:mpu/iso@0.1.0#'
A=()
v(){ A+=(--invoke-with-args "${F}$1=$2"); }

# size-field: every arm of the ladder, then the default
for s in 32 64 128 256 512 1024 2048 4096; do v size-field "$s"; done
v size-field '48'               # not a power of two -> default arm
v size-field '0'                # zero              -> default arm
v size-field '16'               # below MIN_REGION_SIZE -> default arm

# rasr-for: both arms of the writable -> AP selection
v rasr-for '32,1'            # ap = 3
v rasr-for '32,0'           # ap = 6
v rasr-for '1024,1'
v rasr-for '1024,0'

# try-add-region drives validate_region's four conditions. Base row is valid;
# each following row falsifies exactly one condition.
v try-add-region '0,536870912,1024,1'    # 0x2000_0000, 1024  -> T T T T -> true
v try-add-region '0,536870912,0,1'       # size == 0          -> early false
v try-add-region '0,536870912,48,1'      # not a power of two -> F
v try-add-region '0,536870912,16,1'      # below min size 32  -> F
v try-add-region '0,536870928,1024,1'    # 0x2000_0010: misaligned for 1024 -> F
v try-add-region '0,-32,32,1'            # 0xFFFF_FFE0 as i32; +32 overflows -> F
v try-add-region '99,536870912,1024,1'   # partition out of range -> F (defensive)
v try-add-region '0,536871936,32,0'     # a second valid region, read-only

# fill partition 1's eight slots, then a ninth: the no-free-slot rejection
v try-add-region '1,268435456,32,1'
v try-add-region '1,268435488,32,1'
v try-add-region '1,268435520,32,1'
v try-add-region '1,268435552,32,1'
v try-add-region '1,268435584,32,1'
v try-add-region '1,268435616,32,1'
v try-add-region '1,268435648,32,1'
v try-add-region '1,268435680,32,1'
v try-add-region '1,268435712,32,1'      # ninth -> no free slot -> false

# covers-addr: inside a region, outside every region, and a partition with none
v covers-addr '0,536870912'     # exactly the base            -> true
v covers-addr '0,536871000'     # inside the 1024-byte region -> true
v covers-addr '0,536872960'     # past the end                -> false
v covers-addr '0,0'             # nowhere near                -> false
v covers-addr '2,536870912'     # partition with no regions   -> false

# switch-to-partition: drives the full program sequence through the seam,
# for a populated partition and an empty one.
v switch-to-partition '0'
v switch-to-partition '1'
v switch-to-partition '2'
