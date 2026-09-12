// 屏幕捕获 —— 主后端 Windows Graphics Capture（WGC），兜底 GDI。
//
// 为什么主力必须是 WGC：GDI 的 BitBlt 只能复制「GDI 绘制」的内容，现代游戏使用
// DX11/DX12 + flip model 交换链、由 DWM 直接合成，从窗口 DC 抓取恒为全黑 ——
// 独占全屏、无边框全屏、窗口模式三种情况都一样（实测三种模式全部全黑）。
// WGC 由系统合成器交付窗口内容，三种显示模式都能拿到真实画面。
//
// GDI 仅作为兜底：WGC 不可用（老系统）时改用**桌面 DC**（GetDC(NULL)）按屏幕坐标抓取，
// 而不是窗口 DC（窗口 DC 对 flip model 恒为黑）。两种后端都会做「全黑帧」检测并明确报错，
// 绝不基于黑屏做识别与点击。
#![allow(non_snake_case)]

use image::{GrayImage, RgbaImage};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HGDIOBJ, SRCCOPY,
};

use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::types::{GameWindowInfo, ScreenPoint};

/// 本帧实际使用的捕获后端（记录来源；实际回退诊断由 `note_backend_once` 打点）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureBackend {
    Wgc,
    Gdi,
}

/// 一帧捕获结果。灰度图是识别输入；彩色图仅在需要保存调试图时构建。
#[derive(Clone)]
pub struct CapturedFrame {
    pub gray: GrayImage,
    /// 彩色帧：**稳定输入**（识别/颜色分类/描边验证都需要），不再按需生成
    pub rgba: RgbaImage,
    /// 客户区左上角在虚拟桌面物理像素中的位置（图像坐标 → 屏幕坐标的唯一依据）
    pub origin: ScreenPoint,
    /// 本帧来自哪个后端（由 wgc/兜底路径写入，供调试工具读取）
    #[allow(dead_code)]
    pub backend: CaptureBackend,
}

impl CapturedFrame {
    pub fn width(&self) -> u32 {
        self.gray.width()
    }

    pub fn height(&self) -> u32 {
        self.gray.height()
    }

    /// 保存调试图（优先彩色，无彩色时退化为灰度）。
    pub fn save_png(&self, path: &Path) -> Result<(), LoadoutSyncError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LoadoutSyncError::CaptureFailed {
                detail: format!("创建调试目录失败: {e}"),
            })?;
        }
        let res = self.rgba.save(path);
        res.map_err(|e| LoadoutSyncError::CaptureFailed {
            detail: format!("写入调试图失败: {e}"),
        })
    }
}

/// 捕获游戏客户区。
///
/// 顺序：WGC（独占全屏 / 无边框 / 窗口模式都能用）→ 失败则退回桌面 DC 的 GDI 抓取。
/// 两条路径都返回**客户区**大小的画面，`origin` 为客户区左上角的屏幕坐标。
///
/// * want_color：是否需要彩色帧（调试保存/叠加绘制时开启，正常运行关闭以省内存）
pub fn capture_client_area(
    win: &GameWindowInfo,
    want_color: bool,
) -> Result<CapturedFrame, LoadoutSyncError> {
    match super::wgc::capture_client_area(win, want_color) {
        Ok(frame) => {
            note_backend_once(CaptureBackend::Wgc, None);
            Ok(frame)
        }
        Err(wgc_error) => {
            let frame = capture_client_area_gdi(win, want_color)?;
            note_backend_once(CaptureBackend::Gdi, Some(&wgc_error));
            Ok(frame)
        }
    }
}

/// 每个进程只打印一次「后端选择」信息，避免刷屏；切换后端时再次打印。
fn note_backend_once(backend: CaptureBackend, wgc_error: Option<&LoadoutSyncError>) {
    static LAST: AtomicBool = AtomicBool::new(false);
    let already = LAST.swap(true, Ordering::Relaxed);
    match (backend, wgc_error) {
        (CaptureBackend::Wgc, _) if already => {}
        (CaptureBackend::Wgc, _) => {
            eprintln!("[LoadoutSync] 截图后端：WGC（Windows Graphics Capture）");
        }
        (CaptureBackend::Gdi, Some(error)) => {
            eprintln!("[LoadoutSync] WGC 不可用（{error}），退回 GDI 桌面 DC 抓取");
        }
        (CaptureBackend::Gdi, None) => {}
    }
}

/// 兜底：GDI 抓取。必须用**桌面 DC**（GetDC(NULL)）+ 屏幕坐标 ——
/// 窗口 DC 对 flip model 交换链恒为全黑。
pub fn capture_client_area_gdi(
    win: &GameWindowInfo,
    want_color: bool,
) -> Result<CapturedFrame, LoadoutSyncError> {
    let w = win.client.w;
    let h = win.client.h;
    if w <= 0 || h <= 0 {
        return Err(LoadoutSyncError::CaptureFailed {
            detail: format!("客户区尺寸异常 {w}x{h}"),
        });
    }
    unsafe {
        // 桌面 DC：窗口 DC 抓不到 flip model 的画面（恒为黑）
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "GetDC(NULL) 桌面 DC 失败".into(),
            });
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "CreateCompatibleDC 失败".into(),
            });
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, w, h);
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "CreateCompatibleBitmap 失败".into(),
            });
        }
        let old = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        // 源坐标 = 客户区在桌面上的位置；CAPTUREBLT 让分层窗口一并参与合成
        let blt = BitBlt(
            mem_dc,
            0,
            0,
            w,
            h,
            screen_dc,
            win.client.x,
            win.client.y,
            SRCCOPY | CAPTUREBLT,
        );

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 负高度 = 自上而下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let scanned = if blt.is_ok() {
            GetDIBits(
                mem_dc,
                bitmap,
                0,
                h as u32,
                Some(buf.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };

        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);

        if let Err(e) = blt {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: format!("BitBlt 失败: {e}"),
            });
        }
        if scanned <= 0 {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "GetDIBits 未返回扫描行".into(),
            });
        }
        let frame = frame_from_bgra(&buf, w as u32, h as u32, win, want_color)?;
        Ok(frame)
    }
}

fn frame_from_bgra(
    buf: &[u8],
    w: u32,
    h: u32,
    win: &GameWindowInfo,
    want_color: bool,
) -> Result<CapturedFrame, LoadoutSyncError> {
    let _ = want_color;
    let mut gray = GrayImage::new(w, h);
    let mut rgba = RgbaImage::new(w, h);
    // 亮度归一化：HDR / 不同色彩配置下只依赖相对亮度的识别更稳定
    let mut sum: u64 = 0;
    for (i, px) in buf.chunks_exact(4).enumerate() {
        let (b, g, r) = (px[0] as u32, px[1] as u32, px[2] as u32);
        let luma = ((r * 299 + g * 587 + b * 114) / 1000) as u8;
        let x = (i as u32) % w;
        let y = (i as u32) / w;
        gray.put_pixel(x, y, image::Luma([luma]));
        sum += luma as u64;
        rgba.put_pixel(x, y, image::Rgba([px[2], px[1], px[0], 255]));
    }
    let mean = sum as f32 / (w as f32 * h as f32).max(1.0);
    if mean < 1.0 {
        // 独占全屏 / 受保护画面：GDI 返回全黑。明确失败，绝不基于黑屏做识别与点击。
        return Err(LoadoutSyncError::CaptureFailed {
            detail: "捕获到全黑画面（GDI 兜底也拿不到画面，请确认游戏窗口未被最小化）".into(),
        });
    }
    Ok(CapturedFrame {
        gray,
        rgba,
        origin: ScreenPoint {
            x: win.client.x,
            y: win.client.y,
        },
        backend: CaptureBackend::Gdi,
    })
}
