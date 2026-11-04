//! The calls behind the display scale: read what a display allows, set a step.
//!
//! The scale of Windows is per display. The system reports, for a display, the lowest step, the step in use and the highest step it allows without signing out, relative to the step it recommends
//! (DisplayConfigGetDeviceInfo with the undocumented GET_DPI_SCALE, type -3); a step is set with the matching SET_DPI_SCALE (-4). The arithmetic is in dpi_scale_core.rs. This replaces
//! tools/SetDpi.exe, which is the same pair of calls but is started and not waited for, takes only the primary display, and fails in its own console ("Invalid DPI scale value") where nobody sees it.
//!
//! A display is named by its GDI name (`\\.\DISPLAY3`, the name display_sliders.rs lists); `None` is the primary display. The range changes with the display's resolution and with which display it is, so it is
//! read at every call and never kept.

use crate::dpi_scale_core::{self, Resolved, ScaleRange};
use std::time::Duration;

/// Longest a read or a write may take on the blocking thread before the caller goes on without it.
const CALL_TIMEOUT: Duration = Duration::from_millis(2500);

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use windows::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
        DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    };
    use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, LUID, POINT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW, MonitorFromPoint};

    const GET_DPI_SCALE: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(-3);
    const SET_DPI_SCALE: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(-4);

    #[repr(C)]
    struct GetPacket {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
        min_rel: i32,
        cur_rel: i32,
        max_rel: i32,
    }

    #[repr(C)]
    struct SetPacket {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
        scale_rel: i32,
    }

    #[derive(Clone, Copy)]
    struct Source {
        adapter: LUID,
        id: u32,
    }

    fn text(wide: &[u16]) -> String {
        let end = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
        String::from_utf16_lossy(&wide[..end])
    }

    /// The GDI name of the primary display.
    fn primary_device() -> Result<String, String> {
        unsafe {
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
                return Err("读不到主显示器".to_string());
            }
            Ok(text(&info.szDevice))
        }
    }

    /// The active display sources with their GDI names.
    fn sources() -> Result<Vec<(String, Source)>, String> {
        unsafe {
            for _ in 0..3 {
                let (mut paths_n, mut modes_n) = (0u32, 0u32);
                let rc = GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_n, &mut modes_n);
                if rc != ERROR_SUCCESS {
                    return Err(format!("GetDisplayConfigBufferSizes 返回 {}", rc.0));
                }
                let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = (0..paths_n).map(|_| DISPLAYCONFIG_PATH_INFO::default()).collect();
                let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = (0..modes_n).map(|_| DISPLAYCONFIG_MODE_INFO::default()).collect();
                let rc = QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut paths_n, paths.as_mut_ptr(), &mut modes_n, modes.as_mut_ptr(), None);
                if rc == ERROR_INSUFFICIENT_BUFFER {
                    continue; // the topology changed between the two calls
                }
                if rc != ERROR_SUCCESS {
                    return Err(format!("QueryDisplayConfig 返回 {}", rc.0));
                }
                let mut found = Vec::new();
                for path in paths.iter().take(paths_n as usize) {
                    let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                    name.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
                    name.header.adapterId = path.sourceInfo.adapterId;
                    name.header.id = path.sourceInfo.id;
                    if DisplayConfigGetDeviceInfo(&mut name.header) == 0 {
                        found.push((text(&name.viewGdiDeviceName), Source { adapter: path.sourceInfo.adapterId, id: path.sourceInfo.id }));
                    }
                }
                return Ok(found);
            }
            Err("显示配置在查询时一直在变".to_string())
        }
    }

    fn source_of(device: Option<&str>) -> Result<(String, Source), String> {
        let wanted = match device {
            Some(name) => name.to_string(),
            None => primary_device()?,
        };
        sources()?.into_iter().find(|(name, _)| name.eq_ignore_ascii_case(&wanted)).ok_or_else(|| format!("显示器 {wanted} 不在当前的显示配置里"))
    }

    fn get_rel(source: Source) -> Result<(i32, i32, i32), String> {
        let mut packet = GetPacket {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: GET_DPI_SCALE, size: std::mem::size_of::<GetPacket>() as u32, adapterId: source.adapter, id: source.id },
            min_rel: 0,
            cur_rel: 0,
            max_rel: 0,
        };
        let rc = unsafe { DisplayConfigGetDeviceInfo(&mut packet as *mut GetPacket as *mut DISPLAYCONFIG_DEVICE_INFO_HEADER) };
        if rc != 0 {
            return Err(format!("读取显示器的缩放范围失败 ({rc})"));
        }
        Ok((packet.min_rel, packet.cur_rel, packet.max_rel))
    }

    pub(super) fn read(device: Option<&str>) -> Result<ScaleRange, String> {
        let (name, source) = source_of(device)?;
        let (min_rel, cur_rel, max_rel) = get_rel(source)?;
        dpi_scale_core::range_from_rel(min_rel, cur_rel, max_rel).ok_or_else(|| format!("显示器 {name} 报告的缩放范围无效 ({min_rel}, {cur_rel}, {max_rel})"))
    }

    pub(super) fn write(device: Option<&str>, requested: u32) -> Result<(Resolved, ScaleRange), String> {
        let (name, source) = source_of(device)?;
        let (min_rel, cur_rel, max_rel) = get_rel(source)?;
        let range = dpi_scale_core::range_from_rel(min_rel, cur_rel, max_rel).ok_or_else(|| format!("显示器 {name} 报告的缩放范围无效 ({min_rel}, {cur_rel}, {max_rel})"))?;
        let resolved = dpi_scale_core::resolve(requested, &range.options).ok_or("显示器没有可用的缩放档")?;
        if resolved.applied != range.current {
            let scale_rel = dpi_scale_core::rel_for(min_rel, resolved.applied).ok_or("缩放档无效")?;
            let packet = SetPacket {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: SET_DPI_SCALE, size: std::mem::size_of::<SetPacket>() as u32, adapterId: source.adapter, id: source.id },
                scale_rel,
            };
            let rc = unsafe { DisplayConfigSetDeviceInfo(&packet as *const SetPacket as *const DISPLAYCONFIG_DEVICE_INFO_HEADER) };
            if rc != 0 {
                return Err(format!("设置显示器 {name} 的缩放失败 ({rc})"));
            }
        }
        Ok((resolved, range))
    }
}

/// What the display allows now (`None` = the primary display). Runs on the blocking pool.
pub async fn read_range(device: Option<String>) -> Result<ScaleRange, String> {
    #[cfg(target_os = "windows")]
    {
        let task = tokio::task::spawn_blocking(move || win::read(device.as_deref()));
        tokio::time::timeout(CALL_TIMEOUT, task).await.map_err(|_| "读取显示器的缩放范围超时".to_string())?.map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, CALL_TIMEOUT);
        Err("缩放调整仅在 Windows 上可用".to_string())
    }
}

/// Sets the scale of a display (`None` = the primary display) to the step the display allows nearest to `requested`. Returns what came of the request and the range the display had.
pub async fn set_scale(device: Option<String>, requested: u32) -> Result<(Resolved, ScaleRange), String> {
    #[cfg(target_os = "windows")]
    {
        let task = tokio::task::spawn_blocking(move || win::write(device.as_deref(), requested));
        tokio::time::timeout(CALL_TIMEOUT, task).await.map_err(|_| "设置显示器的缩放超时".to_string())?.map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, requested, CALL_TIMEOUT);
        let _ = dpi_scale_core::DPI_VALS;
        Err("缩放调整仅在 Windows 上可用".to_string())
    }
}
