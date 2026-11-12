use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Devices::Bluetooth::{
    BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindNextDevice,
    BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS,
};
use windows::Win32::Foundation::{BOOL, HANDLE};
use windows::Win32::NetworkManagement::WiFi::{
    wlan_interface_state_connected, wlan_intf_opcode_current_connection, WlanCloseHandle,
    WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
    WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
    REG_DWORD,
};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Default, Clone, PartialEq, Eq)]
pub struct NetworkState {
    pub wifi: Option<String>,
    pub bluetooth: Vec<String>,
    pub airplane: bool,
}

/// Polls the network APIs away from the UI thread so Wi-Fi and Bluetooth
/// enumeration can never stall the animation loop.
pub fn spawn_worker() -> Receiver<NetworkState> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || loop {
        let state = NetworkState {
            wifi: unsafe { current_wifi_ssid() },
            bluetooth: unsafe { connected_bluetooth_devices() },
            airplane: airplane_mode_enabled(),
        };
        if sender.send(state).is_err() {
            break;
        }
        thread::sleep(POLL_INTERVAL);
    });
    receiver
}

unsafe fn current_wifi_ssid() -> Option<String> {
    let mut negotiated = 0u32;
    let mut client = HANDLE::default();
    if WlanOpenHandle(2, None, &mut negotiated, &mut client) != 0 {
        return None;
    }

    let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
    if WlanEnumInterfaces(client, None, &mut list) != 0 || list.is_null() {
        let _ = WlanCloseHandle(client, None);
        return None;
    }

    let mut result = None;
    let count = (*list).dwNumberOfItems as usize;
    for index in 0..count {
        let info = &*(*list).InterfaceInfo.as_ptr().add(index);
        if info.isState != wlan_interface_state_connected {
            continue;
        }

        let mut data_size = 0u32;
        let mut data: *mut c_void = null_mut();
        if WlanQueryInterface(
            client,
            &info.InterfaceGuid,
            wlan_intf_opcode_current_connection,
            None,
            &mut data_size,
            &mut data,
            None,
        ) != 0
            || data.is_null()
        {
            continue;
        }

        let attrs = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
        if attrs.isState == wlan_interface_state_connected {
            let ssid = &attrs.wlanAssociationAttributes.dot11Ssid;
            let length = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
            if length > 0 {
                result = Some(String::from_utf8_lossy(&ssid.ucSSID[..length]).to_string());
            }
        }
        WlanFreeMemory(data);

        if result.is_some() {
            break;
        }
    }

    WlanFreeMemory(list.cast());
    let _ = WlanCloseHandle(client, None);
    result
}

unsafe fn connected_bluetooth_devices() -> Vec<String> {
    let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: BOOL(1),
        fReturnRemembered: BOOL(1),
        fReturnUnknown: BOOL(0),
        fReturnConnected: BOOL(1),
        fIssueInquiry: BOOL(0),
        cTimeoutMultiplier: 0,
        hRadio: HANDLE::default(),
    };
    let mut info = BLUETOOTH_DEVICE_INFO::default();
    info.dwSize = std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32;

    let Ok(find) = BluetoothFindFirstDevice(&params, &mut info) else {
        return Vec::new();
    };

    let mut devices = Vec::new();
    loop {
        if info.fConnected.as_bool() {
            let name = wide_string(&info.szName);
            if !name.is_empty() && !devices.contains(&name) {
                devices.push(name);
            }
        }
        if BluetoothFindNextDevice(find, &mut info).is_err() {
            break;
        }
    }
    let _ = BluetoothFindDeviceClose(find);
    devices
}

fn wide_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

fn airplane_mode_enabled() -> bool {
    const KEY: PCWSTR = w!("SYSTEM\\CurrentControlSet\\Control\\RadioManagement\\SystemRadioState");
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, KEY, 0, KEY_QUERY_VALUE, &mut key).0 != 0 {
            return false;
        }

        let mut value = 0u32;
        let mut value_type = REG_DWORD;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut status = RegQueryValueExW(
            key,
            w!(""),
            None,
            Some(&mut value_type),
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        );
        if status.0 != 0 {
            size = std::mem::size_of::<u32>() as u32;
            status = RegQueryValueExW(
                key,
                w!("SystemRadioState"),
                None,
                Some(&mut value_type),
                Some((&mut value as *mut u32).cast()),
                Some(&mut size),
            );
        }
        let _ = RegCloseKey(key);
        status.0 == 0 && value != 0
    }
}
