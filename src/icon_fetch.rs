// 缺失战备图标的在线补齐 — 下载 helldivers.wiki.gg 的 SVG 图标并栅格化为 PNG
//
// 数据源（helldivers.wiki.gg/Stratagems）的战备图标以 SVG 文件形式托管在
// https://helldivers.wiki.gg/images/ 下。应用本身只消费 PNG，因此流程为：
//   下载 SVG → usvg 解析 → resvg 栅格化（RGBA）→ 反预乘 → PNG 编码。
// 生成的 PNG 由调用方写入 exe 旁 assets/icons/{key}.png 并注册进 IconStore，
// 与「exe 旁新增 PNG 自动发现」的既有约定一致，重启后无需重新下载。
use std::io::Read;
use std::time::Duration;

const USER_AGENT: &str =
    "h2ac-rs/1.1 (Helldivers 2 Auto Stratagem Caller; icon fetcher)";
const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// 输出 PNG 边长（与内置图标纹理同尺寸，文件也保持此分辨率）
const ICON_PX: u32 = 128;

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()
}

/// 下载原始 SVG 字节
pub fn download_svg(url: &str) -> Result<Vec<u8>, String> {
    let resp = build_agent()
        .get(url)
        .call()
        .map_err(|e| format!("图标下载失败: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("图标读取失败: {e}"))?;
    if bytes.is_empty() {
        return Err(format!("图标为空: {url}"));
    }
    Ok(bytes)
}

/// SVG 字节 → PNG 字节（128×128 RGBA）
pub fn svg_to_png(svg: &[u8]) -> Result<Vec<u8>, String> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg, &opt).map_err(|e| format!("SVG 解析失败: {e}"))?;

    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("SVG 尺寸无效".into());
    }
    // 等比缩放到目标画布（图标基本为正方形，非正方形时留透明边）
    let (w, h) = (size.width(), size.height());
    let scale = (ICON_PX as f32 / w).min(ICON_PX as f32 / h);
    let transform = tiny_skia::Transform::from_scale(scale, scale);

    let mut pixmap = tiny_skia::Pixmap::new(ICON_PX, ICON_PX)
        .ok_or_else(|| "无法分配栅格画布".to_string())?;
    let mut canvas = pixmap.as_mut();
    resvg::render(&tree, transform, &mut canvas);

    // tiny-skia 输出预乘 alpha 的 RGBA；PNG 需要直通（非预乘）像素
    let data = pixmap.data();
    let mut straight = Vec::with_capacity(data.len());
    for px in data.chunks_exact(4) {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        if a == 0 {
            straight.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let scale = 255.0 / a as f32;
            straight.extend_from_slice(&[
                (r as f32 * scale).round() as u8,
                (g as f32 * scale).round() as u8,
                (b as f32 * scale).round() as u8,
                a,
            ]);
        }
    }

    let img = image::RgbaImage::from_raw(ICON_PX, ICON_PX, straight)
        .ok_or_else(|| "像素缓冲尺寸异常".to_string())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    Ok(out.into_inner())
}

/// 下载并栅格化一张图标，直接产出 PNG 字节
pub fn fetch_icon_png(url: &str) -> Result<Vec<u8>, String> {
    let svg = download_svg(url)?;
    svg_to_png(&svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用仓库根目录页面快照中的真实 wiki SVG 图标做端到端栅格化验证
    /// （快照仅作结构/数据参考；缺失时跳过）。含 clip-path / transform /
    /// 大 viewBox 等特性，可确认 usvg 解析与 PNG 编码链路正常。
    #[test]
    fn rasterize_snapshot_wiki_svg() {
        let dir = std::path::Path::new("Stratagems - The Helldivers Wiki_files");
        let candidates = [
            "Eagle_Rearm_Stratagem_Icon_Background.svg",
            "Orbital_Precision_Strike_Stratagem_Icon_Background.svg",
            "Meltagun_Stratagem_Icon_Background.svg",
        ];
        let Some(path) = candidates.iter().map(|n| dir.join(n)).find(|p| p.exists()) else {
            eprintln!("skip: 快照图标目录不存在（仅作离线验证）");
            return;
        };
        let svg = std::fs::read(&path).expect("读取快照 SVG 失败");
        let png = svg_to_png(&svg).expect("SVG 栅格化失败");
        let img = image::load_from_memory(&png).expect("生成的 PNG 无法解码");
        assert_eq!(img.width(), ICON_PX);
        assert_eq!(img.height(), ICON_PX);
    }
}
