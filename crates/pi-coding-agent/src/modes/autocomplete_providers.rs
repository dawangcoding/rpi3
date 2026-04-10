//! 自动完成提供者模块
//! 提供文件路径补全和模型名称补全功能

use std::path::Path;
use std::path::PathBuf;
use pi_tui::autocomplete::{AutocompleteItem, AutocompleteProvider, AutocompleteSuggestions};

/// 文件路径自动完成提供者
/// 监听 `@` 前缀触发，从工作目录遍历文件
pub struct FileAutocompleteProvider {
    cwd: PathBuf,
}

impl FileAutocompleteProvider {
    /// 创建新的文件路径提供者
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    /// 递归收集文件和目录
    fn collect_files(
        &self,
        dir: &Path,
        root: &Path,
        query: &str,
        items: &mut Vec<AutocompleteItem>,
        depth: usize,
        max_items: usize,
    ) {
        if depth > 5 || items.len() >= max_items {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            if items.len() >= max_items {
                break;
            }

            let name = entry.file_name().to_string_lossy().to_string();

            // 跳过隐藏文件和常见排除目录
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }

            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            // 模糊匹配
            let query_lower = query.to_lowercase();
            let relative_lower = relative.to_lowercase();

            if query.is_empty() || relative_lower.contains(&query_lower) {
                let is_dir = path.is_dir();
                items.push(AutocompleteItem {
                    label: relative.clone(),
                    detail: Some(if is_dir {
                        "directory".to_string()
                    } else {
                        "file".to_string()
                    }),
                    insert_text: Some(format!("@{}", relative)),
                    kind: Some("file".to_string()),
                });
            }

            // 递归进入目录
            if path.is_dir() {
                self.collect_files(&path, root, query, items, depth + 1, max_items);
            }
        }
    }
}

impl AutocompleteProvider for FileAutocompleteProvider {
    fn provide(&self, input: &str, cursor_pos: usize) -> Option<AutocompleteSuggestions> {
        // 找到光标位置前最后一个 @ 符号
        let text_before_cursor = &input[..cursor_pos.min(input.len())];
        let at_pos = text_before_cursor.rfind('@')?;
        let query = &text_before_cursor[at_pos + 1..];

        // 遍历文件并匹配
        let mut items = Vec::new();
        self.collect_files(&self.cwd, &self.cwd, query, &mut items, 0, 10);

        if items.is_empty() {
            return None;
        }

        Some(AutocompleteSuggestions {
            items,
            prefix: format!("@{}", query),
        })
    }
}

/// 模型名称自动完成提供者
/// 在 `/model ` 后触发
pub struct ModelAutocompleteProvider;

impl ModelAutocompleteProvider {
    /// 创建新的模型名称提供者
    pub fn new() -> Self {
        Self
    }

    /// 获取模型列表
    fn model_list() -> Vec<(&'static str, &'static str)> {
        vec![
            ("claude-sonnet-4-20250514", "Anthropic"),
            ("claude-3-5-haiku-20241022", "Anthropic"),
            ("gpt-4o", "OpenAI"),
            ("gpt-4o-mini", "OpenAI"),
            ("o3-mini", "OpenAI"),
            ("gemini-2.0-flash", "Google"),
            ("gemini-2.5-pro-preview-05-06", "Google"),
            ("mistral-large-latest", "Mistral"),
        ]
    }
}

impl Default for ModelAutocompleteProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AutocompleteProvider for ModelAutocompleteProvider {
    fn provide(&self, input: &str, cursor_pos: usize) -> Option<AutocompleteSuggestions> {
        let text = &input[..cursor_pos.min(input.len())];
        if !text.starts_with("/model ") {
            return None;
        }
        let query = text[7..].trim();

        let items: Vec<AutocompleteItem> = Self::model_list()
            .iter()
            .filter(|(name, _)| {
                query.is_empty() || name.to_lowercase().contains(&query.to_lowercase())
            })
            .map(|(name, provider)| AutocompleteItem {
                label: name.to_string(),
                detail: Some(provider.to_string()),
                insert_text: Some(format!("/model {}", name)),
                kind: Some("model".to_string()),
            })
            .collect();

        if items.is_empty() {
            return None;
        }

        Some(AutocompleteSuggestions {
            items,
            prefix: format!("/model {}", query),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_file_autocomplete_provider() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // 创建测试文件
        fs::write(temp_path.join("test.rs"), "// test").unwrap();
        fs::create_dir(temp_path.join("src")).unwrap();
        fs::write(temp_path.join("src/main.rs"), "fn main() {}").unwrap();

        let provider = FileAutocompleteProvider::new(temp_path.to_path_buf());

        // 测试 @ 触发
        let result = provider.provide("@src", 4);
        assert!(result.is_some());
        let suggestions = result.unwrap();
        assert!(!suggestions.items.is_empty());

        // 测试不匹配
        let result = provider.provide("@nonexistent", 12);
        assert!(result.is_none());
    }

    #[test]
    fn test_model_autocomplete_provider() {
        let provider = ModelAutocompleteProvider::new();

        // 测试 /model 触发
        let result = provider.provide("/model gpt", 10);
        assert!(result.is_some());
        let suggestions = result.unwrap();
        assert!(!suggestions.items.is_empty());

        // 测试不匹配
        let result = provider.provide("/model unknown", 14);
        assert!(result.is_none());

        // 测试非 /model 命令不触发
        let result = provider.provide("/help gpt", 9);
        assert!(result.is_none());
    }
}
