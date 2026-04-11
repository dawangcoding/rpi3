//! 输出截断工具
//!
//! 提供工具输出的截断功能，限制输出行数和字节数

/// 截断结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TruncationResult {
    /// 是否被截断
    pub was_truncated: bool,
    /// 原始行数
    pub original_lines: usize,
    /// 保留行数
    pub kept_lines: usize,
    /// 原始字节数
    pub original_bytes: usize,
    /// 保留字节数
    pub kept_bytes: usize,
}

/// 默认最大行数
pub const DEFAULT_MAX_LINES: usize = 500;
/// 默认最大字节数 (1MB)
pub const DEFAULT_MAX_BYTES: usize = 1_000_000;

/// 截断文本到指定限制
/// 
/// 从头部开始保留内容，适用于 read 等工具
pub fn truncate_output_head(text: &str, max_lines: usize, max_bytes: usize) -> (String, TruncationResult) {
    let original_bytes = text.len();
    let lines: Vec<&str> = text.lines().collect();
    let original_lines = lines.len();

    // 检查是否需要截断
    if original_lines <= max_lines && original_bytes <= max_bytes {
        return (
            text.to_string(),
            TruncationResult {
                was_truncated: false,
                original_lines,
                kept_lines: original_lines,
                original_bytes,
                kept_bytes: original_bytes,
            },
        );
    }

    // 按行数和字节数限制截断
    let mut result_lines = Vec::new();
    let mut current_bytes = 0;

    for (i, line) in lines.iter().enumerate() {
        if i >= max_lines {
            break;
        }
        let line_bytes = line.len() + 1; // +1 for newline
        if current_bytes + line_bytes > max_bytes {
            break;
        }
        result_lines.push(*line);
        current_bytes += line_bytes;
    }

    let kept_lines = result_lines.len();
    let kept_bytes = current_bytes;
    let mut result = result_lines.join("\n");
    
    // 添加截断提示
    if kept_lines < original_lines || kept_bytes < original_bytes {
        result.push_str(&format!(
            "\n\n[truncated: showing {} of {} lines, {} of {} bytes]",
            kept_lines, original_lines, format_size(kept_bytes), format_size(original_bytes)
        ));
    }

    (
        result,
        TruncationResult {
            was_truncated: true,
            original_lines,
            kept_lines,
            original_bytes,
            kept_bytes,
        },
    )
}

/// 截断文本到指定限制（从尾部保留）
/// 
/// 从尾部开始保留内容，适用于 bash 等工具（错误通常在最后）
pub fn truncate_output_tail(text: &str, max_lines: usize, max_bytes: usize) -> (String, TruncationResult) {
    let original_bytes = text.len();
    let lines: Vec<&str> = text.lines().collect();
    let original_lines = lines.len();

    // 检查是否需要截断
    if original_lines <= max_lines && original_bytes <= max_bytes {
        return (
            text.to_string(),
            TruncationResult {
                was_truncated: false,
                original_lines,
                kept_lines: original_lines,
                original_bytes,
                kept_bytes: original_bytes,
            },
        );
    }

    // 从尾部开始保留
    let mut result_lines = Vec::new();
    let mut current_bytes = 0;

    for line in lines.iter().rev().take(max_lines) {
        let line_bytes = line.len() + 1; // +1 for newline
        if current_bytes + line_bytes > max_bytes {
            break;
        }
        result_lines.push(*line);
        current_bytes += line_bytes;
    }

    // 反转回正确顺序
    result_lines.reverse();

    let kept_lines = result_lines.len();
    let kept_bytes = current_bytes;
    let mut result = result_lines.join("\n");
    
    // 添加截断提示
    if kept_lines < original_lines || kept_bytes < original_bytes {
        let start_line = original_lines - kept_lines + 1;
        result = format!(
            "[truncated: showing lines {}-{} of {}]\n{}",
            start_line, original_lines, original_lines, result
        );
    }

    (
        result,
        TruncationResult {
            was_truncated: true,
            original_lines,
            kept_lines,
            original_bytes,
            kept_bytes,
        },
    )
}

/// 格式化字节大小为人类可读格式
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 截断单行文本到最大字符数
pub fn truncate_line(line: &str, max_chars: usize) -> (String, bool) {
    if line.len() <= max_chars {
        (line.to_string(), false)
    } else {
        (format!("{}... [truncated]", &line[..max_chars]), true)
    }
}

/// 流式截断结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)] // 预留结构体供未来使用
pub struct StreamingTruncationResult {
    /// 截断后的内容
    pub content: String,
    /// 是否被截断
    pub was_truncated: bool,
    /// 原始行数
    pub original_lines: usize,
    /// 保留行数
    pub kept_lines: usize,
    /// 保留字节数
    pub kept_bytes: usize,
}

/// 从文件流中按行截断，无需全量加载到内存
/// 适用于超大文件的处理
#[allow(dead_code)] // 预留函数供未来使用
pub async fn truncate_file_streaming(
    path: &std::path::Path,
    max_lines: usize,
    max_bytes: usize,
    keep_head: bool,  // true = 保留头部, false = 保留尾部
) -> anyhow::Result<StreamingTruncationResult> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    
    let file = tokio::fs::File::open(path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    
    let mut collected_lines = Vec::new();
    let mut total_bytes = 0usize;
    let mut total_lines = 0usize;
    
    if keep_head {
        // 保留头部：读取直到达到限制
        while let Some(line) = lines.next_line().await? {
            total_lines += 1;
            let line_bytes = line.len() + 1; // +1 for newline
            
            if collected_lines.len() < max_lines && total_bytes + line_bytes <= max_bytes {
                total_bytes += line_bytes;
                collected_lines.push(line);
            }
        }
    } else {
        // 保留尾部：使用环形缓冲区
        use std::collections::VecDeque;
        let mut ring: VecDeque<String> = VecDeque::with_capacity(max_lines + 1);
        let mut ring_bytes = 0usize;
        
        while let Some(line) = lines.next_line().await? {
            total_lines += 1;
            let line_bytes = line.len() + 1;
            
            ring.push_back(line);
            ring_bytes += line_bytes;
            
            // 移除超出限制的行
            while ring.len() > max_lines || ring_bytes > max_bytes {
                if let Some(removed) = ring.pop_front() {
                    ring_bytes -= removed.len() + 1;
                }
            }
        }
        
        collected_lines = ring.into_iter().collect();
        total_bytes = collected_lines.iter().map(|l| l.len() + 1).sum();
    }
    
    let was_truncated = total_lines > collected_lines.len();
    let output = collected_lines.join("\n");
    
    Ok(StreamingTruncationResult {
        content: output,
        was_truncated,
        original_lines: total_lines,
        kept_lines: collected_lines.len(),
        kept_bytes: total_bytes,
    })
}
