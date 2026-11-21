//! Registro COM e de APO (o que o `regsvr32` faz). Precisa de administrador.

use winreg::enums::*;
use winreg::RegKey;
use windows_core::GUID;

/// {5A3E1F0C-6B2D-4C8E-9F17-53A2D0B7E4C1}
pub const CLSID_SR_APO: GUID = GUID::from_u128(0x5a3e1f0c_6b2d_4c8e_9f17_53a2d0b7e4c1);

pub fn clsid_string() -> String {
    format!("{{{:?}}}", CLSID_SR_APO).to_uppercase()
}

fn dll_path() -> std::io::Result<String> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    unsafe {
        let mut h = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows_core::PCWSTR(dll_path as *const u16),
            &mut h,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(Some(h), &mut buf) as usize;
        Ok(String::from_utf16_lossy(&buf[..n]))
    }
}

pub fn register_server() -> std::io::Result<()> {
    let id = clsid_string();
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let (k, _) = hkcr.create_subkey(format!(r"CLSID\{id}"))?;
    k.set_value("", &"Surround Rescue APO")?;
    let (s, _) = k.create_subkey("InprocServer32")?;
    s.set_value("", &dll_path()?)?;
    s.set_value("ThreadingModel", &"Both")?;

    let p = crate::reg_properties();
    let (a, _) = hkcr.create_subkey(format!(r"AudioEngine\AudioProcessingObjects\{id}"))?;
    a.set_value("FriendlyName", &"Surround Rescue")?;
    a.set_value("Copyright", &"Surround Rescue")?;
    a.set_value("MajorVersion", &p.u32MajorVersion)?;
    a.set_value("MinorVersion", &p.u32MinorVersion)?;
    a.set_value("Flags", &(p.Flags.0 as u32))?;
    a.set_value("MinInputConnections", &p.u32MinInputConnections)?;
    a.set_value("MaxInputConnections", &p.u32MaxInputConnections)?;
    a.set_value("MinOutputConnections", &p.u32MinOutputConnections)?;
    a.set_value("MaxOutputConnections", &p.u32MaxOutputConnections)?;
    a.set_value("MaxInstances", &p.u32MaxInstances)?;
    a.set_value("NumAPOInterfaces", &1u32)?;
    a.set_value("APOInterface0", &format!("{{{:?}}}", p.iidAPOInterfaceList[0]).to_uppercase())?;
    Ok(())
}

pub fn unregister_server() -> std::io::Result<()> {
    let id = clsid_string();
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let _ = hkcr.delete_subkey_all(format!(r"AudioEngine\AudioProcessingObjects\{id}"));
    let _ = hkcr.delete_subkey_all(format!(r"CLSID\{id}"));
    Ok(())
}
