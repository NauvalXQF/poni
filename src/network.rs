use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use windows::core::{w, Interface, HSTRING, PCWSTR};
use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristicUuids, GattCommunicationStatus, GattServiceUuids,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::IPropertyValue;
use windows::Storage::Streams::DataReader;
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
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
    REG_DWORD,
};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Default, Clone, PartialEq, Eq)]
pub struct BluetoothDevice {
    pub name: String,
    pub battery: Option<u8>,
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct NetworkState {
    pub wifi: Option<String>,
    pub bluetooth: Vec<BluetoothDevice>,
    pub airplane: bool,
}

/// Polls the network APIs away from the UI thread so Wi-Fi and Bluetooth
/// enumeration can never stall the animation loop.
pub fn spawn_worker() -> Receiver<NetworkState> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        loop {
            let batteries = bluetooth_battery_levels();
            let state = NetworkState {
                wifi: unsafe { current_wifi_ssid() },
                bluetooth: unsafe { connected_bluetooth_devices(&batteries) },
                airplane: airplane_mode_enabled(),
            };
            if sender.send(state).is_err() {
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
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

unsafe fn connected_bluetooth_devices(batteries: &HashMap<String, u8>) -> Vec<BluetoothDevice> {
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
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };

    let Ok(find) = BluetoothFindFirstDevice(&params, &mut info) else {
        return Vec::new();
    };

    let mut devices = Vec::new();
    loop {
        if info.fConnected.as_bool() {
            let name = wide_string(&info.szName);
            if !name.is_empty() && !devices.iter().any(|d: &BluetoothDevice| d.name == name) {
                let battery = batteries.get(&name).copied();
                devices.push(BluetoothDevice { name, battery });
            }
        }
        if BluetoothFindNextDevice(find, &mut info).is_err() {
            break;
        }
    }
    let _ = BluetoothFindDeviceClose(find);
    devices
}

fn bluetooth_battery_levels() -> HashMap<String, u8> {
    let mut map = HashMap::new();
    // Best-effort: query connected Bluetooth devices and read battery
    // properties exposed by Windows for some peripherals.
    if let Ok(aqs) =
        windows::Devices::Bluetooth::BluetoothDevice::GetDeviceSelectorFromConnectionStatus(
            windows::Devices::Bluetooth::BluetoothConnectionStatus::Connected,
        )
    {
        collect_bluetooth_batteries(&aqs, &mut map);
    }
    if let Ok(aqs_le) =
        windows::Devices::Bluetooth::BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(
            windows::Devices::Bluetooth::BluetoothConnectionStatus::Connected,
        )
    {
        collect_bluetooth_batteries(&aqs_le, &mut map);
    }
    map
}

fn collect_bluetooth_batteries(aqs: &HSTRING, map: &mut HashMap<String, u8>) {
    let Ok(collection) = DeviceInformation::FindAllAsyncAqsFilter(aqs).and_then(|op| op.get())
    else {
        return;
    };
    let Ok(count) = collection.Size() else {
        return;
    };
    for i in 0..count {
        let Ok(info) = collection.GetAt(i) else {
            continue;
        };
        let Ok(name_h) = info.Name() else {
            continue;
        };
        let name = name_h.to_string();
        if name.is_empty() || map.contains_key(&name) {
            continue;
        }
        let mut battery = info.Properties().ok().and_then(|properties| {
            read_battery_property(&properties, "System.Devices.Aep.BatteryLevel")
                .or_else(|| {
                    read_battery_property(&properties, "System.Devices.Aep.BatteryPercentage")
                })
                .or_else(|| {
                    read_battery_property(&properties, "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2")
                })
        });
        if battery.is_none() {
            if let Ok(id) = info.Id() {
                battery = read_gatt_battery_level(&id);
            }
        }
        if let Some(level) = battery {
            map.insert(name, level.min(100));
        }
    }
}

fn read_gatt_battery_level(device_id: &HSTRING) -> Option<u8> {
    let device = BluetoothLEDevice::FromIdAsync(device_id).ok()?.get().ok()?;
    let service_uuid = GattServiceUuids::Battery().ok()?;
    let services_result = device
        .GetGattServicesForUuidAsync(service_uuid)
        .ok()?
        .get()
        .ok()?;
    if services_result.Status().ok()? != GattCommunicationStatus::Success {
        return None;
    }
    let services = services_result.Services().ok()?;
    let count = services.Size().ok()?;
    let char_uuid = GattCharacteristicUuids::BatteryLevel().ok()?;
    for i in 0..count {
        let service = services.GetAt(i).ok()?;
        let chars_result = service
            .GetCharacteristicsForUuidAsync(char_uuid)
            .ok()?
            .get()
            .ok()?;
        if chars_result.Status().ok()? != GattCommunicationStatus::Success {
            continue;
        }
        let chars = chars_result.Characteristics().ok()?;
        let char_count = chars.Size().ok()?;
        for j in 0..char_count {
            let characteristic = chars.GetAt(j).ok()?;
            let read = characteristic.ReadValueAsync().ok()?.get().ok()?;
            if read.Status().ok()? != GattCommunicationStatus::Success {
                continue;
            }
            let buffer = read.Value().ok()?;
            let len = buffer.Length().ok()? as usize;
            if len == 0 {
                continue;
            }
            let reader = DataReader::FromBuffer(&buffer).ok()?;
            let level = reader.ReadByte().ok()?;
            if level <= 100 {
                return Some(level);
            }
        }
    }
    None
}

fn read_battery_property(
    properties: &windows::Foundation::Collections::IMapView<HSTRING, windows::core::IInspectable>,
    key: &str,
) -> Option<u8> {
    let key = HSTRING::from(key);
    if !properties.HasKey(&key).ok()? {
        return None;
    }
    let value = properties.Lookup(&key).ok()?;
    let prop: IPropertyValue = value.cast().ok()?;
    if let Ok(v) = prop.GetUInt8() {
        if v <= 100 {
            return Some(v);
        }
    }
    if let Ok(v) = prop.GetInt32() {
        if (0..=100).contains(&v) {
            return Some(v as u8);
        }
    }
    if let Ok(v) = prop.GetUInt32() {
        if v <= 100 {
            return Some(v as u8);
        }
    }
    None
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
