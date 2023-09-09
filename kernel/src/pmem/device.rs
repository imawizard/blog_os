use crate::nfit;
use crate::nfit::NfitEntry;
use crate::nfit::SpaRangeEntry;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;
use x86_64::PhysAddr;

#[derive(Clone)]
pub struct NfitDevice {
    pub handle: u32,
    pub physical_id: u16,
    pub phys_addr: PhysAddr,
    pub size: u64,
    pub flush_addresses: Option<Vec<PhysAddr>>,
}

impl Default for NfitDevice {
    fn default() -> Self {
        Self {
            handle: Default::default(),
            physical_id: Default::default(),
            phys_addr: PhysAddr::new(0),
            size: Default::default(),
            flush_addresses: Default::default(),
        }
    }
}

impl fmt::Debug for NfitDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (nl, tb) = if f.alternate() {
            ("\n", "    ")
        } else {
            ("", " ")
        };

        write!(f, "{} {{{}", core::any::type_name::<Self>(), nl)?;
        write!(f, "{}handle: {:x},{}", tb, self.handle, nl)?;
        write!(f, "{}physical_id: 0x{:04x},{}", tb, self.physical_id, nl)?;
        write!(
            f,
            "{}phys_addr: 0x{:012x},{}",
            tb,
            self.phys_addr.as_u64(),
            nl
        )?;
        write!(
            f,
            "{}size: {} MiB,{}",
            tb,
            self.size as f64 / 1024_f64 / 1024_f64,
            nl
        )?;

        if let Some(addrs) = &self.flush_addresses {
            write!(f, "{}flush_addresses: ", tb)?;
            for addr in addrs.iter() {
                write!(f, "0x{:012x},", addr.as_u64())?;
            }
        }

        write!(f, "}}")
    }
}

pub fn get_devices(nfit: &nfit::Nfit) -> Vec<NfitDevice> {
    let mut spas = BTreeMap::<u16, &SpaRangeEntry>::new();
    for e in nfit.entries() {
        if let NfitEntry::SpaRange(e) = e {
            spas.entry(e.index).or_insert(e);
        }
    }

    let mut devices = BTreeMap::<u32, NfitDevice>::new();
    for e in nfit.entries() {
        match e {
            NfitEntry::NvdimmRegionMapping(e) => {
                let device = devices
                    .entry(e.nfit_device_handle)
                    .or_insert(NfitDevice::default());

                device.handle = e.nfit_device_handle;
                device.physical_id = e.nvdimm_physical_id;

                let idx = e.spa_range_index;
                if let Some(spa) = spas.get(&idx) {
                    device.phys_addr = PhysAddr::new(spa.system_physical_address_range_base);
                    device.size = spa.system_physical_address_range_length;
                }
            }
            NfitEntry::FlushHintAddress(e) => {
                let device = devices
                    .entry(e.nfit_device_handle)
                    .or_insert(NfitDevice::default());

                let ary = e.flush_hint_addresses;
                device.flush_addresses = Some(
                    (1..e.num_of_flush_hint_addresses)
                        .map(|i| unsafe { *ary.get_unchecked(i as usize) })
                        .map(PhysAddr::new)
                        .collect(),
                );
            }
            _ => {}
        }
    }

    let mut res: Vec<_> = devices.into_values().collect();
    res.sort_unstable_by_key(|d| d.phys_addr);
    res
}
