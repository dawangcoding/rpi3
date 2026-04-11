//! 终端图像协议支持
//! 提供 Kitty 和 iTerm2 图像协议的检测和编码功能

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::sync::atomic::{AtomicU32, Ordering};

/// 终端图像协议
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    /// Kitty 图像协议
    Kitty,
    /// iTerm2 图像协议
    ITerm2,
    /// 不支持图像
    None,
}

/// 终端能力
#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    /// 图像协议
    pub image_protocol: ImageProtocol,
    /// 是否支持 Sixel
    pub sixel: bool,
    /// 是否支持真彩色
    pub true_color: bool,
}

/// 图像尺寸
#[derive(Debug, Clone, Copy)]
pub struct ImageDimensions {
    /// 宽度
    pub width: u32,
    /// 高度
    pub height: u32,
}

/// 单元格尺寸
#[derive(Debug, Clone, Copy)]
pub struct CellDimensions {
    /// 宽度
    pub width: u16,
    /// 高度
    pub height: u16,
}

// 默认单元格尺寸
const DEFAULT_CELL_WIDTH: u16 = 9;
const DEFAULT_CELL_HEIGHT: u16 = 18;

// 图像 ID 计数器
static IMAGE_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

/// 检测终端能力
pub fn detect_capabilities() -> TerminalCapabilities {
    // 检查环境变量来检测终端类型
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
    let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();

    // Kitty 终端
    if std::env::var("KITTY_WINDOW_ID").is_ok() || term_program == "kitty" {
        return TerminalCapabilities {
            image_protocol: ImageProtocol::Kitty,
            sixel: false,
            true_color: true,
        };
    }

    // Ghostty 终端
    if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok() 
        || term_program == "ghostty" 
        || term.contains("ghostty") {
        return TerminalCapabilities {
            image_protocol: ImageProtocol::Kitty,
            sixel: false,
            true_color: true,
        };
    }

    // WezTerm 终端
    if std::env::var("WEZTERM_PANE").is_ok() || term_program == "wezterm" {
        return TerminalCapabilities {
            image_protocol: ImageProtocol::Kitty,
            sixel: false,
            true_color: true,
        };
    }

    // iTerm2 终端
    if std::env::var("ITERM_SESSION_ID").is_ok() || term_program == "iterm.app" {
        return TerminalCapabilities {
            image_protocol: ImageProtocol::ITerm2,
            sixel: false,
            true_color: true,
        };
    }

    // 其他终端
    let true_color = colorterm == "truecolor" || colorterm == "24bit";
    
    TerminalCapabilities {
        image_protocol: ImageProtocol::None,
        sixel: false,
        true_color,
    }
}

/// 分配图像 ID (Kitty)
pub fn allocate_image_id() -> u32 {
    IMAGE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// 编码图像为 Kitty 协议
pub fn encode_kitty(data: &[u8], id: u32, cols: u16, rows: u16) -> String {
    const CHUNK_SIZE: usize = 4096;
    
    let base64_data = BASE64.encode(data);
    let mut result = String::new();
    
    let params = format!("a=T,f=100,q=2,c={},r={},i={}", cols, rows, id);
    
    if base64_data.len() <= CHUNK_SIZE {
        // 单块传输
        result.push_str(&format!("\x1b_G{};{}\x1b\\", params, base64_data));
    } else {
        // 分块传输
        let mut offset = 0;
        let mut is_first = true;
        
        while offset < base64_data.len() {
            let chunk_end = (offset + CHUNK_SIZE).min(base64_data.len());
            let chunk = &base64_data[offset..chunk_end];
            let is_last = chunk_end >= base64_data.len();
            
            if is_first {
                result.push_str(&format!("\x1b_G{},m=1;{}\x1b\\", params, chunk));
                is_first = false;
            } else if is_last {
                result.push_str(&format!("\x1b_Gm=0;{}\x1b\\", chunk));
            } else {
                result.push_str(&format!("\x1b_Gm=1;{}\x1b\\", chunk));
            }
            
            offset = chunk_end;
        }
    }
    
    result
}

/// 编码图像为 iTerm2 协议
pub fn encode_iterm2(data: &[u8], width: u16, height: u16) -> String {
    let base64_data = BASE64.encode(data);
    format!(
        "\x1b]1337;File=inline=1;width={};height={}:{}\x07",
        width, height, base64_data
    )
}

/// 删除 Kitty 图像
pub fn delete_kitty_image(id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={}\x1b\\", id)
}

/// 获取 PNG 图像尺寸
pub fn get_png_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if data.len() < 24 {
        return None;
    }
    
    // 检查 PNG 签名
    if data[0] != 0x89 || data[1] != 0x50 || data[2] != 0x4e || data[3] != 0x47 {
        return None;
    }
    
    // 宽度和高度在大端字节序的 16-23 字节
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    
    Some(ImageDimensions { width, height })
}

/// 获取 JPEG 图像尺寸
pub fn get_jpeg_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if data.len() < 2 {
        return None;
    }
    
    // 检查 JPEG 签名
    if data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    
    let mut offset = 2;
    while offset < data.len() - 9 {
        if data[offset] != 0xff {
            offset += 1;
            continue;
        }
        
        let marker = data[offset + 1];
        
        // SOF0, SOF1, SOF2 标记
        if (0xc0..=0xc2).contains(&marker) {
            let height = u16::from_be_bytes([data[offset + 5], data[offset + 6]]) as u32;
            let width = u16::from_be_bytes([data[offset + 7], data[offset + 8]]) as u32;
            return Some(ImageDimensions { width, height });
        }
        
        // 跳过当前段
        if offset + 3 >= data.len() {
            return None;
        }
        let length = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        if length < 2 {
            return None;
        }
        offset += 2 + length;
    }
    
    None
}

/// 获取图像尺寸（根据 MIME 类型）
pub fn get_image_dimensions(data: &[u8], mime_type: &str) -> Option<ImageDimensions> {
    match mime_type {
        "image/png" => get_png_dimensions(data),
        "image/jpeg" | "image/jpg" => get_jpeg_dimensions(data),
        _ => None,
    }
}

/// 计算图像在终端中的行数
pub fn calculate_image_rows(
    image_dims: &ImageDimensions,
    cell_dims: &CellDimensions,
    max_cols: u16,
) -> u16 {
    let target_width_px = max_cols as u32 * cell_dims.width as u32;
    let scale = target_width_px as f32 / image_dims.width as f32;
    let scaled_height_px = image_dims.height as f32 * scale;
    let rows = (scaled_height_px / cell_dims.height as f32).ceil() as u16;
    rows.max(1)
}

/// 渲染图像到终端字符串行
pub fn render_image(
    data: &[u8],
    max_width: u16,
    max_height: u16,
    capabilities: &TerminalCapabilities,
) -> Vec<String> {
    let dims = match get_image_dimensions(data, "image/png")
        .or_else(|| get_image_dimensions(data, "image/jpeg")) {
        Some(d) => d,
        None => return image_fallback(max_width, max_height),
    };
    
    let cell_dims = CellDimensions {
        width: DEFAULT_CELL_WIDTH,
        height: DEFAULT_CELL_HEIGHT,
    };
    
    let cols = max_width;
    let rows = calculate_image_rows(&dims, &cell_dims, max_width)
        .min(max_height);
    
    match capabilities.image_protocol {
        ImageProtocol::Kitty => {
            let id = allocate_image_id();
            let sequence = encode_kitty(data, id, cols, rows);
            
            // 返回多行：前 rows-1 行是空的，最后一行包含图像序列
            let mut lines = vec![String::new(); (rows as usize).saturating_sub(1)];
            
            // 移动光标到第一行然后输出图像
            let move_up = if rows > 1 {
                format!("\x1b[{}A", rows - 1)
            } else {
                String::new()
            };
            lines.push(format!("{}{}", move_up, sequence));
            
            lines
        }
        ImageProtocol::ITerm2 => {
            let sequence = encode_iterm2(data, cols, rows);
            let mut lines = vec![String::new(); (rows as usize).saturating_sub(1)];
            
            let move_up = if rows > 1 {
                format!("\x1b[{}A", rows - 1)
            } else {
                String::new()
            };
            lines.push(format!("{}{}", move_up, sequence));
            
            lines
        }
        ImageProtocol::None => {
            image_fallback(max_width, max_height)
        }
    }
}

/// 无图像协议时的回退方案
pub fn image_fallback(width: u16, height: u16) -> Vec<String> {
    let mut lines = Vec::new();
    let top_bottom = format!("┌{}┐", "─".repeat(width as usize - 2));
    let middle = format!("│{}│", " ".repeat(width as usize - 2));
    let bottom = format!("└{}┘", "─".repeat(width as usize - 2));
    
    lines.push(top_bottom);
    for _ in 1..height.saturating_sub(1) {
        lines.push(middle.clone());
    }
    if height > 1 {
        lines.push(bottom);
    }
    
    lines
}

/// 检查行是否为图像行（包含 Kitty 或 iTerm2 序列）
pub fn is_image_line(line: &str) -> bool {
    line.contains("\x1b_G") || line.contains("\x1b]1337;File=")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_png_dimensions() {
        // 创建一个最小的有效 PNG 文件头
        let mut png_data = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        // IHDR chunk
        png_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x0d]); // length
        png_data.extend_from_slice(b"IHDR");
        png_data.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // width: 256
        png_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x80]); // height: 128
        png_data.extend_from_slice(&[0x08, 0x02, 0x00, 0x00, 0x00]); // bit depth, color type, etc.
        
        let dims = get_png_dimensions(&png_data);
        assert!(dims.is_some());
        let dims = dims.unwrap();
        assert_eq!(dims.width, 256);
        assert_eq!(dims.height, 128);
    }

    #[test]
    fn test_calculate_image_rows() {
        let dims = ImageDimensions { width: 100, height: 50 };
        let cell_dims = CellDimensions { width: 10, height: 20 };
        
        // 100px / 10px per cell = 10 cells wide
        // Scale = 10 * 10 / 100 = 1.0
        // Scaled height = 50 * 1.0 = 50px
        // Rows = 50 / 20 = 2.5 -> 3 rows
        let rows = calculate_image_rows(&dims, &cell_dims, 10);
        assert_eq!(rows, 3);
    }

    #[test]
    fn test_encode_kitty() {
        let data = b"test data";
        let encoded = encode_kitty(data, 1, 10, 5);
        assert!(encoded.starts_with("\x1b_G"));
        assert!(encoded.contains("i=1"));
        assert!(encoded.contains("c=10"));
        assert!(encoded.contains("r=5"));
    }

    #[test]
    fn test_encode_iterm2() {
        let data = b"test data";
        let encoded = encode_iterm2(data, 10, 5);
        assert!(encoded.starts_with("\x1b]1337;File="));
        assert!(encoded.contains("width=10"));
        assert!(encoded.contains("height=5"));
    }
}
