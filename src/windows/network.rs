//
// Sysinfo
//
// Copyright (c) 2017 Guillaume Gomez
//

use crate::{NetworkExt, NetworksExt, NetworksIter};

use std::collections::{hash_map, HashMap};

use winapi::shared::ifdef::{MediaConnectStateDisconnected, NET_LUID};
use winapi::shared::netioapi::{
    FreeMibTable, GetIfEntry2, GetIfTable2, MIB_IF_ROW2, PMIB_IF_TABLE2,
};
use winapi::shared::winerror::NO_ERROR;

macro_rules! old_and_new {
    ($ty_:expr, $name:ident, $old:ident, $new_val:expr) => {{
        $ty_.$old = $ty_.$name;
        $ty_.$name = $new_val;
    }};
}

/// Network interfaces.
///
/// ```no_run
/// use sysinfo::{NetworksExt, System, SystemExt};
///
/// let s = System::new_all();
/// let networks = s.get_networks();
/// ```
pub struct Networks {
    interfaces: HashMap<String, NetworkData>,
}

impl Networks {
    pub(crate) fn new() -> Networks {
        Networks {
            interfaces: HashMap::new(),
        }
    }
}

impl NetworksExt for Networks {
    #[allow(clippy::needless_lifetimes)]
    fn iter<'a>(&'a self) -> NetworksIter<'a> {
        NetworksIter::new(self.interfaces.iter())
    }

    fn refresh_networks_list(&mut self) {
        let mut table: PMIB_IF_TABLE2 = std::ptr::null_mut();
        if unsafe { GetIfTable2(&mut table) } != NO_ERROR {
            return;
        }

        for (_, data) in self.interfaces.iter_mut() {
            data.updated = false;
        }

        // In here, this is tricky: we have to filter out the software interfaces to only keep
        // the hardware ones. To do so, we first check the connection potential speed (if 0, not
        // interesting), then we check its state: if not open, not interesting either. And finally,
        // we count the members of a same group: if there is more than 1, then it's software level.
        let mut groups = HashMap::new();
        let mut indexes = Vec::new();
        let ptr = unsafe { (*table).Table.as_ptr() };
        for i in 0..unsafe { *table }.NumEntries {
            let ptr = unsafe { &*ptr.offset(i as _) };
            if (ptr.TransmitLinkSpeed == 0 && ptr.ReceiveLinkSpeed == 0)
                || ptr.MediaConnectState == MediaConnectStateDisconnected
                || ptr.PhysicalAddressLength == 0
            {
                continue;
            }
            let id = vec![
                ptr.InterfaceGuid.Data2,
                ptr.InterfaceGuid.Data3,
                ptr.InterfaceGuid.Data4[0] as _,
                ptr.InterfaceGuid.Data4[1] as _,
                ptr.InterfaceGuid.Data4[2] as _,
                ptr.InterfaceGuid.Data4[3] as _,
                ptr.InterfaceGuid.Data4[4] as _,
                ptr.InterfaceGuid.Data4[5] as _,
                ptr.InterfaceGuid.Data4[6] as _,
                ptr.InterfaceGuid.Data4[7] as _,
            ];
            let entry = groups.entry(id.clone()).or_insert(0);
            *entry += 1;
            if *entry > 1 {
                continue;
            }
            indexes.push((i, id));
        }
        for (i, id) in indexes {
            let ptr = unsafe { &*ptr.offset(i as _) };
            if *groups.get(&id).unwrap_or(&0) > 1 {
                continue;
            }
            let mut pos = 0;
            for x in ptr.Alias.iter() {
                if *x == 0 {
                    break;
                }
                pos += 1;
            }
            let interface_name = match String::from_utf16(&ptr.Alias[..pos]) {
                Ok(s) => s,
                _ => continue,
            };
            match self.interfaces.entry(interface_name) {
                hash_map::Entry::Occupied(mut e) => {
                    let mut interface = e.get_mut();
                    old_and_new!(interface, current_out, old_out, ptr.OutOctets);
                    old_and_new!(interface, current_in, old_in, ptr.InOctets);
                    old_and_new!(
                        interface,
                        packets_in,
                        old_packets_in,
                        ptr.InUcastPkts + ptr.InNUcastPkts
                    );
                    old_and_new!(
                        interface,
                        packets_out,
                        old_packets_out,
                        ptr.OutUcastPkts + ptr.OutNUcastPkts
                    );
                    old_and_new!(interface, errors_in, old_errors_in, ptr.InErrors);
                    old_and_new!(interface, errors_out, old_errors_out, ptr.OutErrors);
                    interface.updated = true;
                }
                hash_map::Entry::Vacant(e) => {
                    let packets_in = ptr.InUcastPkts + ptr.InNUcastPkts;
                    let packets_out = ptr.OutUcastPkts + ptr.OutNUcastPkts;

                    e.insert(NetworkData {
                        id: ptr.InterfaceLuid,
                        current_out: ptr.OutOctets,
                        old_out: ptr.OutOctets,
                        current_in: ptr.InOctets,
                        old_in: ptr.InOctets,
                        packets_in,
                        old_packets_in: packets_in,
                        packets_out,
                        old_packets_out: packets_out,
                        errors_in: ptr.InErrors,
                        old_errors_in: ptr.InErrors,
                        errors_out: ptr.OutErrors,
                        old_errors_out: ptr.OutErrors,
                        updated: true,
                    });
                }
            }
        }
        unsafe {
            FreeMibTable(table as _);
        }
        // Remove interfaces which are gone.
        self.interfaces.retain(|_, d| d.updated);
    }

    #[allow(clippy::uninit_assumed_init)]
    fn refresh(&mut self) {
        let mut entry = unsafe { std::mem::MaybeUninit::<MIB_IF_ROW2>::uninit().assume_init() };
        for (_, interface) in self.interfaces.iter_mut() {
            entry.InterfaceLuid = interface.id;
            entry.InterfaceIndex = 0; // to prevent the function to pick this one as index
            if unsafe { GetIfEntry2(&mut entry) } != NO_ERROR {
                continue;
            }
            old_and_new!(interface, current_out, old_out, entry.OutOctets);
            old_and_new!(interface, current_in, old_in, entry.InOctets);
            old_and_new!(
                interface,
                packets_in,
                old_packets_in,
                entry.InUcastPkts + entry.InNUcastPkts
            );
            old_and_new!(
                interface,
                packets_out,
                old_packets_out,
                entry.OutUcastPkts + entry.OutNUcastPkts
            );
            old_and_new!(interface, errors_in, old_errors_in, entry.InErrors);
            old_and_new!(interface, errors_out, old_errors_out, entry.OutErrors);
        }
    }
}

/// Contains network information.
pub struct NetworkData {
    id: NET_LUID,
    current_out: u64,
    old_out: u64,
    current_in: u64,
    old_in: u64,
    packets_in: u64,
    old_packets_in: u64,
    packets_out: u64,
    old_packets_out: u64,
    errors_in: u64,
    old_errors_in: u64,
    errors_out: u64,
    old_errors_out: u64,
    updated: bool,
}

impl NetworkExt for NetworkData {
    fn get_received(&self) -> u64 {
        self.current_in.saturating_sub(self.old_in)
    }

    fn get_total_received(&self) -> u64 {
        self.current_in
    }

    fn get_transmitted(&self) -> u64 {
        self.current_out.saturating_sub(self.old_out)
    }

    fn get_total_transmitted(&self) -> u64 {
        self.current_out
    }

    fn get_packets_received(&self) -> u64 {
        self.packets_in.saturating_sub(self.old_packets_in)
    }

    fn get_total_packets_received(&self) -> u64 {
        self.packets_in
    }

    fn get_packets_transmitted(&self) -> u64 {
        self.packets_out.saturating_sub(self.old_packets_out)
    }

    fn get_total_packets_transmitted(&self) -> u64 {
        self.packets_out
    }

    fn get_errors_on_received(&self) -> u64 {
        self.errors_in.saturating_sub(self.old_errors_in)
    }

    fn get_total_errors_on_received(&self) -> u64 {
        self.errors_in
    }

    fn get_errors_on_transmitted(&self) -> u64 {
        self.errors_out.saturating_sub(self.old_errors_out)
    }

    fn get_total_errors_on_transmitted(&self) -> u64 {
        self.errors_out
    }
}
