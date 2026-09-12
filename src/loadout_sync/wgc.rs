//! Windows Graphics Capture（WGC）截图后端。
//!
//! 为什么必须换成 WGC：
//! GDI 的 `BitBlt` 只能复制「GDI 绘制」的窗口内容。现代游戏（DX11/DX12 + flip model
//! 交换链）由 DWM 直接合成，从窗口 DC 抓取恒为全黑 —— 独占全屏、无边框全屏、窗口模式
//! 三种情况都一样。WGC 由系统合成器直接交付窗口内容，三种显示模式（含独占全屏）都能
//! 拿到真实画面，并且可以顺手关掉鼠标指针。
//!
//! 设计要点：
//! * 会话按窗口缓存并复用（创建 WGC 会话约几十毫秒，不能每帧重建）；
//! * 帧由 `FrameArrived` 事件投递到「帧邮箱」（`CreateFreeThreaded` 的回调可能在任意线程
//!   触发，因此回调里只入队，真正的 D3D 拷贝/映射/转换留在调用线程做）；
//! * 窗口静止时 WGC 不投递新帧，此时按「最后一帧 + 时限」复用，绝不无限等待；
//! * 像素格式以 BGRA8 为主；若系统给的是 FP16（HDR scRGB），按显示器 SDR 白电平做
//!   scRGB→sRGB 映射，避免 HDR 下画面过暗导致识别失效。
#![allow(non_snake_case)]

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use image::{GrayImage, RgbaImage};
use windows::core::{factory, IInspectable, Interface};
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindow};

use super::capture::{CaptureBackend, CapturedFrame};
use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::types::{GameWindowInfo, ScreenPoint};

/// 等待「下一帧」的最长时间：游戏持续出帧时几乎立即返回；
/// 静止画面（暂停菜单 / 加载界面）超时后复用最后一帧，绝不无限等待。
const FRAME_WAIT: Duration = Duration::from_millis(160);
/// 复用最后一帧的最大时限：超过则认为画面已不可信，明确报错。
const STALE_LIMIT: Duration = Duration::from_millis(1500);
/// 帧池缓冲数（2 足够：一帧在用、一帧在攒）
const POOL_BUFFERS: i32 = 2;

// ─── 帧邮箱 ───

#[derive(Default)]
struct SlotState {
    generation: u64,
    latest: Option<Direct3D11CaptureFrame>,
    closed: bool,
}

/// WGC 回调线程与工作线程之间的单帧邮箱（只保留最新帧，旧帧立即 Close）。
struct FrameSlot {
    state: Mutex<SlotState>,
    ready: Condvar,
}

impl FrameSlot {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SlotState::default()),
            ready: Condvar::new(),
        })
    }

    fn publish(&self, frame: Direct3D11CaptureFrame) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = state.latest.replace(frame) {
            let _ = previous.Close();
        }
        state.generation += 1;
        self.ready.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        if let Some(frame) = state.latest.take() {
            let _ = frame.Close();
        }
        self.ready.notify_all();
    }

    fn is_closed(&self) -> bool {
        self.state.lock().map(|s| s.closed).unwrap_or(true)
    }

    /// 取出比 `consumed` 更新的帧；超时返回 None（调用方决定复用还是报错）。
    fn take_newer_than(
        &self,
        consumed: u64,
        timeout: Duration,
    ) -> Option<(u64, Direct3D11CaptureFrame)> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.generation > consumed {
                if let Some(frame) = state.latest.take() {
                    return Some((state.generation, frame));
                }
            }
            if state.closed {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (next, _) = self
                .ready
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
        }
    }
}

// ─── 会话 ───

/// 客户区在「WGC 帧坐标系」中的裁剪矩形
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Crop {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

struct StagingTexture {
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
}

/// 上一次成功读出的客户区画面（窗口静止时复用）
struct CachedFrame {
    gray: GrayImage,
    rgba: RgbaImage,
    at: Instant,
}

/// WinRT `IDirect3DDevice` 是 agile（自由线程）对象，windows-rs 只是保守地没有为它
/// 生成 `Send`；这里显式包装，让它能放进进程级会话缓存。
struct SharedWinrtDevice(#[allow(dead_code)] IDirect3DDevice);

unsafe impl Send for SharedWinrtDevice {}

struct WgcSession {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// 生命周期必须覆盖 frame_pool（WinRT 设备对象）
    _d3d_device: SharedWinrtDevice,
    _item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: EventRegistrationToken,
    slot: Arc<FrameSlot>,
    hwnd: isize,
    client_size: (u32, u32),
    client_origin: ScreenPoint,
    crop: Crop,
    consumed: u64,
    staging: Option<StagingTexture>,
    cached: Option<CachedFrame>,
    /// HDR：scRGB(FP16) → SDR 的亮度缩放系数
    hdr_scale: f32,
    hdr_lut: Option<Arc<[u8; 65536]>>,
}

impl WgcSession {
    fn new(win: &GameWindowInfo) -> Result<Self, LoadoutSyncError> {
        unsafe {
            // 多线程套间：帧回调可能落在任意线程上
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "系统不支持 Windows Graphics Capture（需要 Windows 10 1903+）".into(),
            });
        }

        let hwnd = HWND(win.hwnd as *mut _);
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|e| {
                LoadoutSyncError::CaptureFailed {
                    detail: format!("获取 WGC 工厂失败: {e}"),
                }
            })?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd) }.map_err(|e| {
            LoadoutSyncError::CaptureFailed {
                detail: format!("WGC 无法捕获该游戏窗口: {e}"),
            }
        })?;
        let item_size = item.Size().map_err(|e| LoadoutSyncError::CaptureFailed {
            detail: format!("读取 WGC 尺寸失败: {e}"),
        })?;
        if item_size.Width <= 0 || item_size.Height <= 0 {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: format!("WGC 尺寸异常 {}x{}", item_size.Width, item_size.Height),
            });
        }
        let crop = client_crop(win, hwnd, item_size)?;

        let (device, context) = create_device()?;
        let d3d_device = to_winrt_device(&device)?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            POOL_BUFFERS,
            item_size,
        )
        .map_err(|e| LoadoutSyncError::CaptureFailed {
            detail: format!("创建 WGC 帧池失败: {e}"),
        })?;

        let slot = FrameSlot::new();
        let publish_slot = Arc::clone(&slot);
        let token = pool
            .FrameArrived(&TypedEventHandler::new(
                move |pool: &Option<Direct3D11CaptureFramePool>, _args: &Option<IInspectable>| {
                    // 回调里只入队：D3D 拷贝与像素转换都留到工作线程
                    if let Some(pool) = pool {
                        if let Ok(frame) = pool.TryGetNextFrame() {
                            publish_slot.publish(frame);
                        }
                    }
                    Ok(())
                },
            ))
            .map_err(|e| LoadoutSyncError::CaptureFailed {
                detail: format!("注册 WGC 帧回调失败: {e}"),
            })?;

        let session =
            pool.CreateCaptureSession(&item)
                .map_err(|e| LoadoutSyncError::CaptureFailed {
                    detail: format!("创建 WGC 会话失败: {e}"),
                })?;
        // 自动化自己的鼠标指针不应该出现在识别画面里
        let _ = session.SetIsCursorCaptureEnabled(false);
        // 黄色捕获边框：系统允许时关掉（未打包应用可能被拒绝，失败不影响捕获）
        let _ = session.SetIsBorderRequired(false);
        session
            .StartCapture()
            .map_err(|e| LoadoutSyncError::CaptureFailed {
                detail: format!("启动 WGC 捕获失败: {e}"),
            })?;

        let white_level = sdr_white_level_for_window(hwnd);
        let hdr_scale = 1000.0 / white_level.unwrap_or(1000).max(1) as f32;

        Ok(Self {
            device,
            context,
            _d3d_device: SharedWinrtDevice(d3d_device),
            _item: item,
            pool,
            session,
            token,
            slot,
            hwnd: win.hwnd,
            client_size: (win.client.w.max(0) as u32, win.client.h.max(0) as u32),
            client_origin: ScreenPoint {
                x: win.client.x,
                y: win.client.y,
            },
            crop,
            consumed: 0,
            staging: None,
            cached: None,
            hdr_scale,
            hdr_lut: None,
        })
    }

    /// 会话是否还能继续用（同一个窗口、尺寸没变、会话没被系统掐断）
    fn is_compatible(&self, win: &GameWindowInfo) -> bool {
        self.hwnd == win.hwnd
            && self.client_size == (win.client.w.max(0) as u32, win.client.h.max(0) as u32)
            && !self.slot.is_closed()
            && unsafe { IsWindow(HWND(self.hwnd as *mut _)).as_bool() }
    }

    fn grab(&mut self, want_color: bool) -> Result<CapturedFrame, LoadoutSyncError> {
        match self.slot.take_newer_than(self.consumed, FRAME_WAIT) {
            Some((generation, frame)) => {
                let result = self.read_frame(&frame, want_color);
                let _ = frame.Close();
                self.consumed = generation;
                match result {
                    Ok(captured) => {
                        self.cached = Some(CachedFrame {
                            gray: captured.gray.clone(),
                            rgba: captured.rgba.clone(),
                            at: Instant::now(),
                        });
                        Ok(captured)
                    }
                    // 单帧读取失败（窗口正在重建交换链等）：退回最后一帧
                    Err(e) if self.cached.is_some() => self.stale_frame(want_color, &e),
                    Err(e) => Err(e),
                }
            }
            // 画面静止时 WGC 不投递新帧：复用最后一帧，但设时限
            None => self.stale_frame(
                want_color,
                &LoadoutSyncError::CaptureFailed {
                    detail: "等待 WGC 新帧超时".into(),
                },
            ),
        }
    }

    fn stale_frame(
        &mut self,
        _want_color: bool,
        reason: &LoadoutSyncError,
    ) -> Result<CapturedFrame, LoadoutSyncError> {
        let Some(cached) = self.cached.as_ref() else {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: format!("WGC 未取得任何帧（窗口可能受保护或已最小化）：{reason}"),
            });
        };
        if cached.at.elapsed() > STALE_LIMIT {
            self.cached = None;
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "WGC 帧已过期（画面长时间无更新）".into(),
            });
        }
        Ok(CapturedFrame {
            gray: cached.gray.clone(),
            rgba: cached.rgba.clone(),
            origin: self.client_origin,
            backend: CaptureBackend::Wgc,
        })
    }

    fn read_frame(
        &mut self,
        frame: &Direct3D11CaptureFrame,
        want_color: bool,
    ) -> Result<CapturedFrame, LoadoutSyncError> {
        let surface = frame
            .Surface()
            .map_err(|e| LoadoutSyncError::CaptureFailed {
                detail: format!("WGC 帧表面不可用: {e}"),
            })?;
        let access: IDirect3DDxgiInterfaceAccess =
            surface
                .cast()
                .map_err(|e| LoadoutSyncError::CaptureFailed {
                    detail: format!("WGC 表面缺少 DXGI 接口: {e}"),
                })?;
        let texture: ID3D11Texture2D =
            unsafe { access.GetInterface() }.map_err(|e| LoadoutSyncError::CaptureFailed {
                detail: format!("WGC 帧纹理不可用: {e}"),
            })?;

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        if desc.Width == 0 || desc.Height == 0 {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "WGC 帧纹理尺寸为 0".into(),
            });
        }

        let width = desc.Width;
        let height = desc.Height;
        let format = desc.Format;
        let kind = PixelKind::of(format).ok_or_else(|| LoadoutSyncError::CaptureFailed {
            detail: format!("不支持的 WGC 像素格式 {}", format.0),
        })?;
        // LUT 必须在借用回读纹理之前构建（两者都要 &mut self）
        let lut = if kind == PixelKind::F16 {
            Some(self.hdr_lut())
        } else {
            None
        };
        self.ensure_staging(width, height, format)?;
        let staging = self
            .staging
            .as_ref()
            .map(|s| s.texture.clone())
            .ok_or_else(|| LoadoutSyncError::CaptureFailed {
                detail: "回读纹理不可用".into(),
            })?;

        let crop = clamp_crop(self.crop, width, height);
        let origin = self.client_origin;
        unsafe {
            self.context.CopyResource(&staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped as *mut _))
                .map_err(|e| LoadoutSyncError::CaptureFailed {
                    detail: format!("映射 WGC 帧失败: {e}"),
                })?;
            let result = convert(
                &mapped,
                width,
                height,
                kind,
                want_color,
                crop,
                origin,
                lut.as_deref(),
            );
            self.context.Unmap(&staging, 0);
            result
        }
    }

    fn hdr_lut(&mut self) -> Arc<[u8; 65536]> {
        if self.hdr_lut.is_none() {
            self.hdr_lut = Some(Arc::new(build_srgb_lut(self.hdr_scale)));
        }
        self.hdr_lut.clone().expect("HDR LUT 已构建")
    }

    fn ensure_staging(
        &mut self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Result<(), LoadoutSyncError> {
        let reuse = matches!(
            self.staging.as_ref(),
            Some(s) if s.width == width && s.height == height && s.format == format
        );
        if reuse {
            return Ok(());
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture as *mut _))
        }
        .map_err(|e| LoadoutSyncError::CaptureFailed {
            detail: format!("创建回读纹理失败: {e}"),
        })?;
        let texture = texture.ok_or_else(|| LoadoutSyncError::CaptureFailed {
            detail: "创建回读纹理返回空".into(),
        })?;
        self.staging = Some(StagingTexture {
            texture,
            width,
            height,
            format,
        });
        Ok(())
    }
}

/// 把映射后的原始像素转成灰度图（+ 可选彩色图），并按客户区裁剪。
#[allow(clippy::too_many_arguments)]
unsafe fn convert(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
    kind: PixelKind,
    _want_color: bool,
    crop: Crop,
    origin: ScreenPoint,
    lut: Option<&[u8; 65536]>,
) -> Result<CapturedFrame, LoadoutSyncError> {
    if crop.w == 0 || crop.h == 0 {
        return Err(LoadoutSyncError::CaptureFailed {
            detail: format!("客户区裁剪为空（帧 {width}x{height}）"),
        });
    }
    let width_out = crop.w as usize;
    let mut gray = GrayImage::new(crop.w, crop.h);
    let mut rgba = RgbaImage::new(crop.w, crop.h);
    let base = mapped.pData as *const u8;
    let pitch = mapped.RowPitch as usize;
    let mut sum: u64 = 0;

    // 直接写底层缓冲：整帧 200 万像素，逐点 put_pixel 在 debug 下会慢一个数量级
    let gray_buf: &mut [u8] = &mut gray;
    let rgba_buf: &mut [u8] = &mut rgba;

    for row in 0..crop.h as usize {
        let row_ptr = base.add((crop.y as usize + row) * pitch);
        let out_row = row * width_out;
        for col in 0..width_out {
            let src_x = crop.x as usize + col;
            let (r, g, b) = match kind {
                PixelKind::Bgra8 => {
                    let px = row_ptr.add(src_x * 4);
                    (*px.add(2), *px.add(1), *px)
                }
                PixelKind::F16 => {
                    let px = row_ptr.add(src_x * 8);
                    let lut = lut.expect("HDR LUT 已构建");
                    (
                        lut[read_u16(px) as usize],
                        lut[read_u16(px.add(2)) as usize],
                        lut[read_u16(px.add(4)) as usize],
                    )
                }
            };
            let luma = ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000) as u8;
            gray_buf[out_row + col] = luma;
            sum += luma as u64;
            {
                let dst = (out_row + col) * 4;
                rgba_buf[dst] = r;
                rgba_buf[dst + 1] = g;
                rgba_buf[dst + 2] = b;
                rgba_buf[dst + 3] = 255;
            }
        }
    }

    let mean = sum as f32 / (crop.w as f32 * crop.h as f32).max(1.0);
    if mean < 1.0 {
        return Err(LoadoutSyncError::CaptureFailed {
            detail: "捕获到全黑画面（WGC 返回空帧：游戏可能被保护或已最小化）".into(),
        });
    }

    Ok(CapturedFrame {
        gray,
        rgba,
        origin,
        backend: CaptureBackend::Wgc,
    })
}
impl Drop for WgcSession {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.RemoveFrameArrived(self.token);
        self.slot.close();
        let _ = self.pool.Close();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PixelKind {
    Bgra8,
    F16,
}

impl PixelKind {
    fn of(format: DXGI_FORMAT) -> Option<Self> {
        if format == DXGI_FORMAT_B8G8R8A8_UNORM || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB {
            Some(Self::Bgra8)
        } else if format == DXGI_FORMAT_R16G16B16A16_FLOAT {
            Some(Self::F16)
        } else {
            None
        }
    }
}

unsafe fn read_u16(ptr: *const u8) -> u16 {
    u16::from_le_bytes([*ptr, *ptr.add(1)])
}

// ─── 公开入口（进程内缓存单个会话） ───

static SESSION: Mutex<Option<WgcSession>> = Mutex::new(None);

/// 用 WGC 抓取游戏客户区。
///
/// 失败（系统不支持 / 窗口受保护 / 会话异常）时返回错误，由调用方决定是否退回 GDI。
pub fn capture_client_area(
    win: &GameWindowInfo,
    want_color: bool,
) -> Result<CapturedFrame, LoadoutSyncError> {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let needs_new = match guard.as_ref() {
        Some(session) => !session.is_compatible(win),
        None => true,
    };
    if needs_new {
        *guard = None;
        *guard = Some(WgcSession::new(win)?);
    }
    let session = guard.as_mut().expect("会话已创建");
    let result = session.grab(want_color);
    if result.is_err() {
        // 出错就丢弃会话，下一帧重建（交换链重建 / 分辨率切换都会走到这里）
        *guard = None;
    }
    result
}

/// 丢弃缓存会话（窗口关闭、任务结束、配置变化时调用）
pub fn invalidate() {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

// ─── 内部辅助 ───

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext), LoadoutSyncError> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .map_err(|e| LoadoutSyncError::CaptureFailed {
        detail: format!("创建 D3D11 设备失败: {e}"),
    })?;
    let device = device.ok_or_else(|| LoadoutSyncError::CaptureFailed {
        detail: "D3D11 设备为空".into(),
    })?;
    let context = context.ok_or_else(|| LoadoutSyncError::CaptureFailed {
        detail: "D3D11 上下文为空".into(),
    })?;
    Ok((device, context))
}

fn to_winrt_device(device: &ID3D11Device) -> Result<IDirect3DDevice, LoadoutSyncError> {
    let dxgi: IDXGIDevice = device.cast().map_err(|e| LoadoutSyncError::CaptureFailed {
        detail: format!("D3D11 设备转换为 DXGI 设备失败: {e}"),
    })?;
    let inspectable: IInspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(|e| LoadoutSyncError::CaptureFailed {
            detail: format!("创建 WinRT Direct3D 设备失败: {e}"),
        })?;
    inspectable
        .cast()
        .map_err(|e| LoadoutSyncError::CaptureFailed {
            detail: format!("WinRT 设备转换失败: {e}"),
        })
}

/// 客户区在 WGC 帧中的位置：无边框/独占全屏时窗口尺寸与客户区一致，直接 (0,0)；
/// 否则用「客户区屏幕坐标 − 窗口屏幕坐标」。
fn client_crop(
    win: &GameWindowInfo,
    hwnd: HWND,
    item_size: SizeInt32,
) -> Result<Crop, LoadoutSyncError> {
    let cw = win.client.w.max(0) as u32;
    let ch = win.client.h.max(0) as u32;
    if cw == 0 || ch == 0 {
        return Err(LoadoutSyncError::CaptureFailed {
            detail: "客户区尺寸为 0，无法捕获".into(),
        });
    }
    if cw == item_size.Width as u32 && ch == item_size.Height as u32 {
        return Ok(Crop {
            x: 0,
            y: 0,
            w: cw,
            h: ch,
        });
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|e| LoadoutSyncError::CaptureFailed {
        detail: format!("GetWindowRect 失败: {e}"),
    })?;
    Ok(Crop {
        x: (win.client.x - rect.left).max(0) as u32,
        y: (win.client.y - rect.top).max(0) as u32,
        w: cw,
        h: ch,
    })
}

fn clamp_crop(crop: Crop, width: u32, height: u32) -> Crop {
    let x = crop.x.min(width);
    let y = crop.y.min(height);
    Crop {
        x,
        y,
        w: crop.w.min(width - x),
        h: crop.h.min(height - y),
    }
}

/// 显示器 SDR 白电平（scRGB ×1000：1000 = 80nit = SDR 白）。
/// 该目标不是 HDR 时返回 1000；查询失败返回 None（调用方按 SDR 处理）。
fn sdr_white_level_for_window(hwnd: HWND) -> Option<u32> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            return None;
        }
        let monitor_device = utf16_z(&info.szDevice);

        let mut path_count = 0u32;
        let mut mode_count = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count).0
            != 0
        {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
        .0 != 0
        {
            return None;
        }

        for path in paths.iter().take(path_count as usize) {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }
            if !utf16_z(&source.viewGdiDeviceName).eq_ignore_ascii_case(&monitor_device) {
                continue;
            }

            let adapter = path.targetInfo.adapterId;
            let target_id = path.targetInfo.id;
            let mut advanced = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            advanced.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            advanced.header.size =
                std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            advanced.header.adapterId = adapter;
            advanced.header.id = target_id;
            if DisplayConfigGetDeviceInfo(&mut advanced.header) != 0 {
                return Some(1000);
            }
            // bit1 = advancedColorEnabled
            if (advanced.Anonymous.Anonymous._bitfield >> 1) & 1 == 0 {
                return Some(1000);
            }

            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            white.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            white.header.size = std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
            white.header.adapterId = adapter;
            white.header.id = target_id;
            if DisplayConfigGetDeviceInfo(&mut white.header) != 0 {
                return Some(1000);
            }
            return Some(white.SDRWhiteLevel.max(1));
        }
        None
    }
}

fn utf16_z(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// FP16(scRGB 线性) → sRGB u8 的 65536 项查表
fn build_srgb_lut(scale: f32) -> [u8; 65536] {
    let mut lut = [0u8; 65536];
    for bits in 0..=u16::MAX {
        lut[bits as usize] = linear_to_srgb_u8(half_to_f32(bits) * scale);
    }
    lut
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) as u32;
    let exp = ((bits >> 10) & 0x1f) as i32;
    let frac = (bits & 0x3ff) as u32;
    let value = if exp == 0 {
        (frac as f32) * 2f32.powi(-24)
    } else if exp == 31 {
        if frac == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        (1.0 + frac as f32 / 1024.0) * 2f32.powi(exp - 15)
    };
    if sign == 1 {
        -value
    } else {
        value
    }
}

fn linear_to_srgb_u8(x: f32) -> u8 {
    if !x.is_finite() {
        return 0;
    }
    let x = x.clamp(0.0, 1.0);
    let y = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (y * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_conversion_matches_known_values() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert!((half_to_f32(0x3C00) - 1.0).abs() < 1e-6);
        assert!((half_to_f32(0x3800) - 0.5).abs() < 1e-6);
        assert!((half_to_f32(0xC000) + 2.0).abs() < 1e-6);
        assert!(half_to_f32(0x7C00).is_infinite());
    }

    #[test]
    fn srgb_lut_maps_black_and_white() {
        let lut = build_srgb_lut(1.0);
        assert_eq!(lut[0x0000], 0, "线性 0 → 0");
        assert_eq!(lut[0x3C00], 255, "线性 1.0 → 255");
        let mid = lut[0x3800];
        assert!((180..=195).contains(&mid), "线性 0.5 应约 188，实际 {mid}");
    }

    #[test]
    fn hdr_scale_brightens_when_sdr_white_is_above_80nit() {
        assert!((1000.0f32 / 1000.0 - 1.0).abs() < 1e-6);
        assert!((1000.0f32 / 3000.0 - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn crop_is_clamped_to_frame() {
        let crop = clamp_crop(
            Crop {
                x: 10,
                y: 20,
                w: 100,
                h: 200,
            },
            50,
            60,
        );
        assert_eq!((crop.x, crop.y, crop.w, crop.h), (10, 20, 40, 40));
    }

    /// 合成 BGRA 缓冲走与实机完全相同的转换路径：验证通道顺序、裁剪与灰度换算。
    #[test]
    fn bgra_conversion_keeps_channel_order_and_crops() {
        let (w, h) = (8u32, 4u32);
        // 每像素 B=10, G=20, R=30, A=255
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for px in buf.chunks_exact_mut(4) {
            px[0] = 10;
            px[1] = 20;
            px[2] = 30;
            px[3] = 255;
        }
        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: buf.as_mut_ptr() as *mut _,
            RowPitch: w * 4,
            DepthPitch: w * 4 * h,
        };
        let crop = Crop {
            x: 2,
            y: 1,
            w: 4,
            h: 2,
        };
        let origin = ScreenPoint { x: 100, y: 200 };
        let frame = unsafe {
            convert(&mapped, w, h, PixelKind::Bgra8, true, crop, origin, None).expect("转换应成功")
        };
        assert_eq!(
            (frame.width(), frame.height()),
            (4, 2),
            "输出尺寸 = 裁剪尺寸"
        );
        assert_eq!(frame.origin, origin, "原点必须是客户区屏幕坐标");
        let rgba = &frame.rgba;
        assert_eq!(
            rgba.get_pixel(0, 0).0,
            [30, 20, 10, 255],
            "BGRA → RGBA 通道顺序"
        );
        let expected_luma = ((30 * 299 + 20 * 587 + 10 * 114) / 1000) as u8;
        assert_eq!(frame.gray.get_pixel(3, 1).0[0], expected_luma, "灰度权重");
    }

    /// 全黑帧必须报错（两条后端一致的行为），绝不把黑屏交给识别流程。
    #[test]
    fn all_black_frame_is_rejected() {
        let (w, h) = (4u32, 4u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: buf.as_mut_ptr() as *mut _,
            RowPitch: w * 4,
            DepthPitch: w * 4 * h,
        };
        let result = unsafe {
            convert(
                &mapped,
                w,
                h,
                PixelKind::Bgra8,
                false,
                Crop { x: 0, y: 0, w, h },
                ScreenPoint { x: 0, y: 0 },
                None,
            )
        };
        let message = match result {
            Ok(_) => panic!("全黑帧必须被拒绝"),
            Err(e) => format!("{e}"),
        };
        assert!(
            message.contains("全黑"),
            "错误信息应说明是全黑帧：{message}"
        );
    }

    #[test]
    fn pixel_kinds_are_recognised() {
        assert_eq!(
            PixelKind::of(DXGI_FORMAT_B8G8R8A8_UNORM),
            Some(PixelKind::Bgra8)
        );
        assert_eq!(
            PixelKind::of(DXGI_FORMAT_R16G16B16A16_FLOAT),
            Some(PixelKind::F16)
        );
        assert_eq!(PixelKind::of(DXGI_FORMAT(0)), None);
    }
}
