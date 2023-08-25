//! Information taken from https://uefi.org/specs/ACPI/6.5/05_ACPI_Software_Programming_Model.html
#![allow(dead_code)]

use core::{
    fmt,
    marker::PhantomData,
    mem::{self, MaybeUninit},
};

use acpi::{sdt::SdtHeader, AcpiTable};

#[repr(C, packed)]
pub struct Nfit {
    pub header: SdtHeader,
    pub reserved: u32,
}

impl AcpiTable for Nfit {
    fn header(&self) -> &SdtHeader {
        &self.header
    }
}

impl Nfit {
    pub fn entries(&self) -> NfitEntryIter {
        NfitEntryIter {
            pointer: unsafe { (self as *const Nfit as *const u8).add(mem::size_of::<Nfit>()) },
            remaining_length: self.header.length - mem::size_of::<Nfit>() as u32,
            _phantom: PhantomData,
        }
    }
}

pub struct NfitEntryIter<'a> {
    pointer: *const u8,
    remaining_length: u32,
    _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy)]
pub enum NfitEntry<'a> {
    /// System Physical Address (SPA) Range Structure
    SpaRange(&'a SpaRangeEntry),
    /// NVDIMM Region Mapping Structure
    NvdimmRegionMapping(&'a NvdimmRegionMappingEntry),
    /// Interleave Structure
    Interleave(&'a InterleaveEntry),
    /// SMBIOS Management Information Structure
    SmbiosManagementInfo(&'a SmbiosManagementInfoEntry),
    /// NVDIMM Control Region Structure Mark
    NvdimmControlRegion(&'a NvdimmControlRegionEntry),
    /// NVDIMM Block Data Windows Region Structure
    NvdimmBlockDataWindowRegion(&'a NvdimmBlockDataWindowRegionEntry),
    /// Flush Hint Address Structure
    FlushHintAddress(&'a FlushHintAddressEntry),
    /// Platform Capabilities Structure
    PlatformCapabilities(&'a PlatformCapabilitiesEntry),
}

impl<'a> Iterator for NfitEntryIter<'a> {
    type Item = NfitEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining_length > 0 {
            let entry_pointer = self.pointer;
            let header = unsafe { *(self.pointer as *const EntryHeader) };

            self.pointer = unsafe { self.pointer.offset(header.length as isize) };
            self.remaining_length -= header.length as u32;

            macro_rules! construct_entry {
                ($entry_type:expr,
                 $entry_pointer:expr,
                 $($value:expr => $variant:path as $type:ty),* $(,)?
                ) => {
                    match $entry_type {
                        $(
                            $value => {
                                return Some($variant(unsafe {
                                    &*($entry_pointer as *const $type)
                                }))
                            }
                        )*
                        // Reserved
                        8..=0xffff => {}
                    }
                }
            }

            construct_entry!(
                header.entry_type,
                entry_pointer,
                // https://uefi.org/specs/ACPI/6.5/05_ACPI_Software_Programming_Model.html#nfit-structure-types
                0 => NfitEntry::SpaRange as SpaRangeEntry,
                1 => NfitEntry::NvdimmRegionMapping as NvdimmRegionMappingEntry,
                2 => NfitEntry::Interleave as InterleaveEntry,
                3 => NfitEntry::SmbiosManagementInfo as SmbiosManagementInfoEntry,
                4 => NfitEntry::NvdimmControlRegion as NvdimmControlRegionEntry,
                5 => NfitEntry::NvdimmBlockDataWindowRegion as NvdimmBlockDataWindowRegionEntry,
                6 => NfitEntry::FlushHintAddress as FlushHintAddressEntry,
                7 => NfitEntry::PlatformCapabilities as PlatformCapabilitiesEntry,
            );
        }

        None
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct EntryHeader {
    entry_type: u16,
    length: u16,
}

macro_rules! print_flags {
    ($f:expr, $flags:expr, [$($value:expr),* $(,)?] $(,)?) => {
        $(
            if $flags & $value == $value {
                write!($f, " {}", stringify!($value))?;
            }
        )*
    }
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
/// This structure describes the system physical address ranges occupied by
/// NVDIMMs, and their corresponding Region Types.
///
/// System physical address ranges described as Virtual CD or Virtual Disk shall
/// be described as AddressRangeReserved in E820, and EFI Reserved Memory Type
/// in the UEFI GetMemoryMap.
///
/// Platform is allowed to implement this structure just to describe system
/// physical address ranges that describe Virtual CD and Virtual Disk. For
/// Virtual CD Region and Virtual Disk Region (both volatile and persistent),
/// the following fields - Proximity Domain, SPA Range Structure Index, Flags,
/// and Address Range Memory Mapping Attribute, are not relevant and shall be
/// set to 0.
///
/// The default mapping of the NVDIMM Control Region shall be UC memory
/// attributes with AddressRangeReserved type in E820 and EfiMemoryMappedIO
/// type in UEFI GetMemoryMap. The default mapping of the NVDIMM Block Data
/// Window Region shall be WB memory attributes with AddressRangeReserved type
/// in E820 and EfiMemoryMappedIO type in UEFI GetMemoryMap.
pub struct SpaRangeEntry {
    pub header: EntryHeader,
    /// Used by NVDIMM Region Mapping Structure to uniquely refer to this structure. Value of 0 is Reserved and shall not be used as an index.
    pub index: u16,
    /// Bits[15:3] are reserved. See SPA_RANGE_*.
    pub flags: u16,
    /// Reserved.
    pub reserved: u32,
    /// Integer that represents the proximity domain to which the memory belongs. This number must match with corresponding entry in the SRAT table.
    pub proximity_domain: u32,
    /// GUID that defines the type of the Address Range Type. The GUID can be any of the values defined in this section, or a vendor defined GUID.
    pub address_range_type_guid: NfitGuid,
    /// Start Address of the System Physical Address Range.
    pub system_physical_address_range_base: u64,
    /// Range Length of the region in bytes.
    pub system_physical_address_range_length: u64,
    /// Memory mapping attributes for this address range. See EFI_MEMORY_*.
    pub address_range_memory_mapping_attributes: u64,
    /// Opaque cookie value set by platform firmware for OSPM use, to detect changes that may impact the readability of the data.
    pub spa_location_cookie: MaybeUninit<u64>,
}

/// Indicates that Control region is strictly for management during hot add/online operation.
pub const SPA_RANGE_ADD_ONLINE_ONLY: u16 = 1;
/// Indicates that data in the Proximity Domain field is valid.
pub const SPA_RANGE_PROXIMITY_VALID: u16 = 2;
/// Indicates that data in the SPALocationCookie field is valid.
pub const SPA_RANGE_LOCATION_COOKIE_VALID: u16 = 4;

/// Memory cacheability attribute: The memory region supports being configured
/// as not cacheable.
pub const EFI_MEMORY_UC: u64 = 0x00000001;
/// Memory cacheability attribute: The memory region supports being configured
/// as write combining.
pub const EFI_MEMORY_WC: u64 = 0x00000002;
/// Memory cacheability attribute: The memory region supports being configured
/// as cacheable with a "write through" policy.
/// Writes that hit in the cache will also be written to main memory.
pub const EFI_MEMORY_WT: u64 = 0x00000004;
/// Memory cacheability attribute: The memory region supports being configured
/// as cacheable with a "write back" policy.
/// Reads and writes that hit in the cache do not propagate to main memory.
/// Dirty data is written back to main memory when a new cache line is allocated.
pub const EFI_MEMORY_WB: u64 = 0x00000008;
/// Memory cacheability attribute: The memory region supports being configured
/// as not cacheable, exported, and supports the "fetch and add" semaphore mechanism.
pub const EFI_MEMORY_UCE: u64 = 0x00000010;
/// Physical memory protection attribute: The memory region supports being
/// configured as write-protected by system hardware.
/// This is typically used as a cacheability attribute today. The memory region
/// supports being configured as cacheable with a "write protected" policy.
/// Reads come from cache lines when possible, and read misses cause cache fills.
/// Writes are propagated to the system bus and cause corresponding cache lines
/// on all processors on the bus to be invalidated.
pub const EFI_MEMORY_WP: u64 = 0x00001000;
/// Physical memory protection attribute: The memory region supports being
/// configured as read-protected by system hardware.
pub const EFI_MEMORY_RP: u64 = 0x00002000;
/// Physical memory protection attribute: The memory region supports being
/// configured so it is protected by system hardware from executing code.
pub const EFI_MEMORY_XP: u64 = 0x00004000;
/// Runtime memory attribute: The memory region refers to persistent memory.
pub const EFI_MEMORY_NV: u64 = 0x00008000;
/// The memory region provides higher reliability relative to other memory in
/// the system. If all memory has the same reliability, then this bit is not used.
pub const EFI_MEMORY_MORE_RELIABLE: u64 = 0x00010000;
/// Physical memory protection attribute: The memory region supports making this
/// memory range read-only by system hardware.
pub const EFI_MEMORY_RO: u64 = 0x00020000;
/// Runtime memory attribute: The memory region needs to be given a virtual
/// mapping by the operating system when SetVirtualAddressMap() is called.
pub const EFI_MEMORY_SP: u64 = 0x00040000;

/// Persistent Memory (PM) Region.
pub const PERSISTENT_MEMORY_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x66f0d379,
    0xb4f3,
    0x4074,
    [0xac, 0x43, 0x0d, 0x33, 0x18, 0xb7, 0x8c, 0xdb],
);
/// NVDIMM Control Region.
pub const NVDIMM_CONTROL_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x92f701f6,
    0x13b4,
    0x405d,
    [0x91, 0x0b, 0x29, 0x93, 0x67, 0xe8, 0x23, 0x4c],
);
/// NVDIMM Block Data Window Region.
pub const NVDIMM_BLOCK_DATA_WINDOW_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x91af0530,
    0x5d86,
    0x470e,
    [0xa6, 0xb0, 0x0a, 0x2d, 0xb9, 0x40, 0x82, 0x49],
);
/// RAM Disk supporting a Virtual Disk Region - Volatile (a volatile memory region that contains a raw disk format).
pub const DISK_RAW_VOLATILE_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x3d5abd30,
    0x4175,
    0x87ce,
    [0x6d, 0x64, 0xd2, 0xad, 0xe5, 0x23, 0xc4, 0xbb],
);
/// RAM Disk supporting a Virtual CD Region - Volatile (a volatile memory region that contains an ISO image).
pub const DISK_ISO_VOLATILE_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x77ab535a,
    0x45fc,
    0x624b,
    [0x55, 0x60, 0xf7, 0xb2, 0x81, 0xd1, 0xf9, 0x6e],
);
/// RAM Disk supporting a Virtual Disk Region - Persistent (a persistent memory region that contains a raw disk format).
pub const DISK_RAW_PERSISTENT_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x5cea02c9,
    0x4d07,
    0x69d3,
    [0x26, 0x9f, 0x44, 0x96, 0xfb, 0xe0, 0x96, 0xf9],
);
/// RAM Disk supporting a Virtual CD Region - Persistent (a persistent memory region that contains an ISO image).
pub const DISK_ISO_PERSISTENT_REGION_TYPE_GUID: NfitGuid = NfitGuid(
    0x08018188,
    0x42cd,
    0xbb48,
    [0x10, 0x0f, 0x53, 0x87, 0xd5, 0x3d, 0xed, 0x3d],
);

impl fmt::Debug for SpaRangeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let length = self.header.length;
        let index = self.index;
        let flags = self.flags;
        let proximity_domain = self.proximity_domain;
        let address_range_type_guid = self.address_range_type_guid;
        let system_physical_address_range_base = self.system_physical_address_range_base;
        let system_physical_address_range_length = self.system_physical_address_range_length;
        let address_range_memory_mapping_attributes = self.address_range_memory_mapping_attributes;
        let (nl, tb) = if f.alternate() {
            ("\n", "    ")
        } else {
            ("", " ")
        };

        write!(f, "SpaRangeEntry {{{}", nl)?;
        write!(f, "{}length: {},{}", tb, length, nl)?;
        write!(f, "{}index: {},{}", tb, index, nl)?;

        write!(f, "{}flags:", tb)?;
        if flags != 0 {
            print_flags!(
                f,
                flags,
                [
                    SPA_RANGE_ADD_ONLINE_ONLY,
                    SPA_RANGE_PROXIMITY_VALID,
                    SPA_RANGE_LOCATION_COOKIE_VALID,
                ],
            );
        } else {
            write!(f, " none")?;
        }
        write!(f, ",{}", nl)?;

        if flags & SPA_RANGE_PROXIMITY_VALID == SPA_RANGE_PROXIMITY_VALID {
            write!(f, "{}proximity_domain: {:08x},{}", tb, proximity_domain, nl)?;
        } else {
            write!(f, "{}proximity_domain: invalid,{}", tb, nl)?;
        }

        write!(
            f,
            "{}address_range_type_guid: {:?},{}",
            tb, address_range_type_guid, nl
        )?;
        write!(
            f,
            "{}system_physical_address_range_base: 0x{:016x},{}",
            tb, system_physical_address_range_base, nl
        )?;
        write!(
            f,
            "{}system_physical_address_range_length: 0x{:016x},{}",
            tb, system_physical_address_range_length, nl
        )?;

        write!(f, "{}address_range_memory_mapping_attributes:", tb)?;
        if address_range_memory_mapping_attributes != 0 {
            print_flags!(
                f,
                address_range_memory_mapping_attributes,
                [
                    EFI_MEMORY_UC,
                    EFI_MEMORY_WC,
                    EFI_MEMORY_WT,
                    EFI_MEMORY_WB,
                    EFI_MEMORY_UCE,
                    EFI_MEMORY_WP,
                    EFI_MEMORY_RP,
                    EFI_MEMORY_XP,
                    EFI_MEMORY_NV,
                    EFI_MEMORY_MORE_RELIABLE,
                    EFI_MEMORY_RO,
                    EFI_MEMORY_SP,
                ],
            );
        } else {
            write!(f, " none")?;
        }
        write!(f, ",{}", nl)?;

        if flags & SPA_RANGE_LOCATION_COOKIE_VALID == SPA_RANGE_LOCATION_COOKIE_VALID {
            let spa_location_cookie = unsafe { self.spa_location_cookie.assume_init() };
            write!(
                f,
                "{}spa_location_cookie: {:016x}{}",
                tb, spa_location_cookie, nl
            )?;
        } else {
            write!(f, "{}spa_location_cookie: not present{}", tb, nl)?;
        }

        write!(f, "}}")
    }
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
/// The NVDIMM Region Mapping structure describes an NVDIMM region and its
/// mapping, if any, to an SPA range.
pub struct NvdimmRegionMappingEntry {
    pub header: EntryHeader,
    /// The _ADR of the NVDIMM device containing the NVDIMM region
    pub nfit_device_handle: u32,
    /// Handle (i.e., instance number) for the SMBIOS Memory Device (Type 17)
    /// structure describing the NVDIMM containing the NVDIMM region.
    pub nvdimm_physical_id: u16,
    /// Unique identifier for the NVDIMM region. This identifier shall be unique
    /// across all the NVDIMM regions in the NVDIMM. There could be multiple
    /// regions within the device corresponding to different address types.
    /// Also, for a given address type, there could be multiple regions due to
    /// interleave discontinuity.
    pub nvdimm_region_id: u16,
    /// The SPA range, if any, associated with the NVDIMM region:
    /// 0x0000: The NVDIMM region does not map to a SPA range.
    /// The following fields are not valid and should be ignored:
    /// - NVDIMM Region Size;
    /// - Region Offset;
    /// - NVDIMM Physical Address Region Base;
    /// - Interleave Structure Index; and
    /// - Interleave Ways.
    /// Fields other than the above (e.g. NFIT Device Handle, NVDIMM Physical ID,
    /// NVDIMM Region ID, and NVDIMM State Flags) are valid.
    /// 0x0001-0xFFFF: The index of the SPA Range Structure for the NVDIMM region.
    pub spa_range_index: u16,
    /// The index of the NVDIMM Control Region Structure for the NVDIMM region.
    pub nvdimm_control_region_index: u16,
    /// The size of the NVDIMM region, in bytes. If SPA Range Structure Index
    /// and Interleave Ways are both non-zero, this field shall match System
    /// Physical Address Range Length divided by Interleave Ways.
    /// NOTE: the size in SPA Range occupied by the NVDIMM for this region will
    /// not be the same as the NVDIMM Region Size when Interleave Ways is
    /// greater than 1.
    pub nvdimm_region_size: u64,
    /// In bytes: The Starting Offset for the NVDIMM region in the Interleave
    /// Set. This offset is with respect to System Physical Address Range Base
    /// the SPA Range Structure. NOTE: The starting SPA of the NVDIMM region in
    /// the NVDIMM is provided by System Physical Address Range Base + Region Offset
    pub region_offset: u64,
    /// In bytes. The base physical address within the NVDIMM of the NVDIMM region.
    pub nvdimm_physical_address_region_base: u64,
    /// The Interleave Structure, if any, for the NVDIMM region.
    /// |Interleave Structure Index|Interleave Ways|Interpretation|
    /// |--------------------------|---------------|--------------|
    /// |0|0|Interleaving, if any, of the NVDIMM region is not reported|
    /// |0|1|The NVDIMM region is not interleaved with other NVDIMMs (i.e., it is one-way interleaved)|
    /// |0|>1|The NVDIMM region is part of an interleave set with the number of NVDIMMs indicated in the Interleave Ways field, including the NVDIMM containing the NVDIMM region, but the Interleave Structure is not described.|
    /// |>0|>1|The NVDIMM region is part of an interleave set with: a) the number of NVDIMMs indicated in the Interleave Ways field, including the NVDIMM containing the NVDIMM region; and b) the Interleave Structure indicated by the Interleave Structure Index field.|
    /// |All other combinations||Invalid case|
    pub interleave_index: u16,
    /// Number of NVDIMMs in the interleave set, including the NVDIMM containing
    /// the NVDIMM region.
    pub interleave_ways: u16,
    /// Bits[15:7] are reserved.
    /// Implementation Note: Platform firmware might report several set bits.
    pub nvdimm_state_flags: u16,
    /// Reserved.
    pub reserved: u16,
}

/// Indicates that the previous SAVE operation to the NVDIMM containing the
/// NVDIMM region failed.
pub const MEM_SAVE_FAILED: u16 = 0x0001;
/// Indicates that the last RESTORE operation from the NVDIMM containing the
/// NVDIMM region failed.
pub const MEM_RESTORE_FAILED: u16 = 0x0002;
/// Indicates that the platform flush of data to the NVDIMM containing the
/// NVDIMM region before the previous SAVE failed. As a result, the restored
/// data content may be inconsistent even if SAVE_FAILED and RESTORE_FAILED do
/// not indicate failure.
pub const MEM_FLUSH_FAILED: u16 = 0x0004;
/// Indicates that the NVDIMM containing the NVDIMM region is not able to accept
/// persistent writes. For an energy-source backed NVDIMM device, this is set if
/// it is not armed or the previous ERASE operation did not complete.
/// If not set the NVDIMM containing the NVDIMM region is armed.
pub const MEM_NOT_ARMED: u16 = 0x0008;
/// Indicates that the NVDIMM containing the NVDIMM region observed SMART and
/// health events prior to OSPM handoff.
pub const MEM_HEALTH_OBSERVED: u16 = 0x0010;
/// Indicates that platform firmware is enabled to notify OSPM of SMART and
/// health events related to the NVDIMM containing the NVDIMM region using
/// Notify codes as specified in NVDIMM Device Notification Values.
pub const MEM_HEALTH_ENABLED: u16 = 0x0020;
/// Indicates that the platform firmware did not map the NVDIMM containing the
/// NVDIMM region into an SPA range. This could be due to various issues such as
/// a device initialization error, device error, insufficient hardware resources
/// to map the device, or a disabled device.
/// Implementation Note: In case of device error, MEM_HEALTH_OBSERVED might be
/// set along with MEM_MAP_FAILED.
pub const MEM_MAP_FAILED: u16 = 0x0040;

impl fmt::Debug for NvdimmRegionMappingEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let length = self.header.length;
        let nfit_device_handle = self.nfit_device_handle;
        let nvdimm_physical_id = self.nvdimm_physical_id;
        let nvdimm_region_id = self.nvdimm_region_id;
        let spa_range_index = self.spa_range_index;
        let nvdimm_control_region_index = self.nvdimm_control_region_index;
        let nvdimm_region_size = self.nvdimm_region_size;
        let region_offset = self.region_offset;
        let nvdimm_physical_address_region_base = self.nvdimm_physical_address_region_base;
        let interleave_index = self.interleave_index;
        let nvdimm_state_flags = self.nvdimm_state_flags;
        let (nl, tb) = if f.alternate() {
            ("\n", "    ")
        } else {
            ("", " ")
        };

        write!(f, "NvdimmRegionMappingEntry {{{}", nl)?;
        write!(f, "{}length: {},{}", tb, length, nl)?;
        write!(
            f,
            "{}nfit_device_handle: {:x},{}",
            tb, nfit_device_handle, nl
        )?;
        write!(
            f,
            "{}nvdimm_physical_id: 0x{:04x},{}",
            tb, nvdimm_physical_id, nl
        )?;
        write!(
            f,
            "{}nvdimm_region_id: 0x{:04x},{}",
            tb, nvdimm_region_id, nl
        )?;
        write!(f, "{}spa_range_index: {},{}", tb, spa_range_index, nl)?;
        write!(
            f,
            "{}nvdimm_control_region_index: {},{}",
            tb, nvdimm_control_region_index, nl
        )?;
        write!(
            f,
            "{}nvdimm_region_size: 0x{:016x},{}",
            tb, nvdimm_region_size, nl
        )?;
        write!(f, "{}region_offset: 0x{:016x},{}", tb, region_offset, nl)?;
        write!(
            f,
            "{}nvdimm_physical_address_region_base: 0x{:016x},{}",
            tb, nvdimm_physical_address_region_base, nl
        )?;
        write!(f, "{}interleave_index: {}{}", tb, interleave_index, nl)?;

        write!(f, "{}nvdimm_state_flags:", tb)?;
        if nvdimm_state_flags != 0 {
            print_flags!(
                f,
                nvdimm_state_flags,
                [
                    MEM_SAVE_FAILED,
                    MEM_RESTORE_FAILED,
                    MEM_FLUSH_FAILED,
                    MEM_NOT_ARMED,
                    MEM_HEALTH_OBSERVED,
                    MEM_HEALTH_ENABLED,
                    MEM_MAP_FAILED,
                ],
            );
        } else {
            write!(f, " none")?;
        }
        write!(f, ",{}", nl)?;

        write!(f, "}}")
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct InterleaveEntry {
    header: EntryHeader,
    /// Index Number uniquely identifies the interleave description - this
    /// allows reuse of interleave description across multiple NVDIMMs.
    /// Index must be non-zero.
    pub index: u16,
    /// Reserved.
    pub reserved: u16,
    /// Only need to describe the number of lines needed before the interleave
    /// pattern repeats
    pub num_of_lines_described: u32,
    /// e.g. 64, 128, 256, 4096
    pub line_size: u32,
    /// Line Offset refers to the offset of the line, in multiples of Line Size,
    /// from the corresponding SPA Range Base for the NVDIMM region.
    /// Line 1 SPA = SPA Range Base + Region Offset + (Line 1 Offset*Line Size).
    /// Line SPA is naturally aligned to the Line size.
    pub line_offset: [u32; 0],
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SmbiosManagementInfoEntry {
    pub header: EntryHeader,
    /// Reserved.
    pub reserved: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvdimmControlRegionEntry {
    pub header: EntryHeader,
    pub index: u16,
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision_id: u16,
    pub subsystem_vendor_id: u16,
    pub subsystem_device_id: u16,
    pub subsystem_revision_id: u16,
    pub valid_fields: u8,
    pub manufacturing_location: u8,
    pub manufacturing_date: u16,
    pub reserved1: u16,
    pub serial_number: [u8; 4],
    pub region_format_interface_code: u16,
    pub num_of_block_control_windows: u16,
    pub block_control_window_size: u64,
    pub command_register_offset: u64,
    pub command_register_size: u64,
    pub status_register_offset: u64,
    pub status_register_size: u64,
    pub nvdimm_control_region_flag: u16,
    pub reserved2: [u8; 6],
}

impl fmt::Display for NvdimmControlRegionEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.valid_fields & 1 == 1 {
            write!(
                f,
                "{:02x}{:02x}-{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}",
                self.vendor_id & 0xff,
                self.vendor_id >> 8 & 0xff,
                self.manufacturing_location,
                self.manufacturing_date & 0xff,
                self.manufacturing_date >> 8 & 0xff,
                self.serial_number[0],
                self.serial_number[1],
                self.serial_number[2],
                self.serial_number[3],
            )
        } else {
            write!(
                f,
                "{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}",
                self.vendor_id & 0xff,
                self.vendor_id >> 8 & 0xff,
                self.serial_number[0],
                self.serial_number[1],
                self.serial_number[2],
                self.serial_number[3],
            )
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvdimmBlockDataWindowRegionEntry {
    pub header: EntryHeader,
    pub nvdimm_control_region_index: u16,
    pub num_of_block_data_windows: u16,
    pub block_data_window_start_offset: u64,
    pub block_data_window_size: u64,
    pub block_accessible_memory_capacity: u64,
    pub block_accessible_memory_start_addr: u64,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct FlushHintAddressEntry {
    pub header: EntryHeader,
    /// Indicates the NVDIMM supported by the Flush Hint Addresses in this
    /// structure.
    pub nfit_device_handle: u32,
    /// Number of Flush Hint Addresses in this structure.
    pub num_of_flush_hint_addresses: u16,
    /// Reserved.
    pub reserved: [u16; 3],
    /// 64-bit system physical address that needs to be written to cause
    /// durability flush. Software is allowed to write up to a cache line of
    /// data. The content of the data is not relevant to the functioning of the
    /// flush hint mechanism.
    pub flush_hint_addresses: [u64; 0],
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct PlatformCapabilitiesEntry {
    pub header: EntryHeader,
    /// The bit index of the highest valid capability implemented by the
    /// platform. The subsequent bits shall not be considered to determine the
    /// capabilities supported by the platform.
    pub highest_valid_cap_bit: u8,
    /// Reserved.
    pub reserved1: [u8; 3],
    /// Bits[31:3] reserved. See CAPABILITY_*.
    pub capabilities: u32,
    /// Reserved.
    pub reserved2: u32,
}

/// CPU Cache Flush to NVDIMM Durability on Power Loss Capable. If set to 1,
/// indicates that platform ensures the entire CPU store data path is flushed to
/// persistent memory on system power loss.
pub const CAPABILITY_CACHE_FLUSH: u32 = 1;
/// Memory Controller Flush to NVDIMM Durability on Power Loss Capable. If set
/// to 1, indicates that platform provides mechanisms to automatically flush
/// outstanding write data from the memory controller to persistent memory in
/// the event of platform power loss. Note: If bit 0 is set to 1 then this bit
/// shall be set to 1 as well.
pub const CAPABILITY_MEM_FLUSH: u32 = 2;
/// Byte Addressable Persistent Memory Hardware Mirroring Capable. If set to 1,
/// indicates that platform supports mirroring multiple byte addressable
/// persistent memory regions together. If this feature is supported and enabled,
/// healthy hardware mirrored interleave sets will have the
/// EFI_MEMORY_MORE_RELIABLE Address Range Memory Mapping Attribute set in the
/// System Physical Address Range structure in the NFIT table.
pub const CAPABILITY_MEM_MIRRORING: u32 = 4;

#[derive(Clone, Copy)]
pub struct NfitGuid(pub u32, pub u16, pub u16, pub [u8; 8]);

impl fmt::Debug for NfitGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "NfitGuid {{ 0x{:08x}, 0x{:04x}, 0x{:04x}",
            self.0, self.1, self.2
        )?;
        for i in self.3 {
            write!(f, ", 0x{:02x}", i)?;
        }
        write!(f, " }}")
    }
}
