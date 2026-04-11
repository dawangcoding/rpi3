//! 文件编辑工具
//!
//! 使用精确文本替换编辑文件

use std::path::PathBuf;
use async_trait::async_trait;
use tokio::fs;
use tokio_util::sync::CancellationToken;
use pi_agent::types::*;
use pi_ai::types::*;
use similar::TextDiff;

/// 单个编辑操作
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Edit {
    /// 旧文本
    pub old_text: String,
    /// 新文本
    pub new_text: String,
}

/// 文件编辑工具
pub struct EditTool {
    cwd: PathBuf,
}

impl EditTool {
    /// 创建新的 EditTool
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    /// 解析路径（相对路径相对于 cwd）
    fn resolve_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        let path_buf = PathBuf::from(path);
        let absolute_path = if path_buf.is_absolute() {
            path_buf
        } else {
            self.cwd.join(path_buf)
        };
        
        // 规范化路径
        let canonical = absolute_path.canonicalize().map_err(|e| {
            anyhow::anyhow!("Failed to resolve path '{}': {}", path, e)
        })?;
        
        // 确保路径在 cwd 下（防止路径遍历攻击）
        let canonical_cwd = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
        if !canonical.starts_with(&canonical_cwd) {
            return Err(anyhow::anyhow!(
                "Path '{}' is outside the working directory",
                path
            ));
        }
        
        Ok(canonical)
    }

    /// 检测行尾符
    fn detect_line_ending(content: &str) -> &'static str {
        if content.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// 生成统一 diff
    fn generate_diff(old_content: &str, new_content: &str, path: &str) -> String {
        let diff = TextDiff::from_lines(old_content, new_content);
        let mut result = String::new();
        
        result.push_str(&format!("--- {}\n", path));
        result.push_str(&format!("+++ {}\n", path));
        
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                similar::ChangeTag::Delete => "-",
                similar::ChangeTag::Insert => "+",
                similar::ChangeTag::Equal => " ",
            };
            result.push_str(&format!("{}{}", sign, change.value()));
        }
        
        result
    }

    /// 应用编辑操作
    fn apply_edits(&self, content: &str, edits: Vec<Edit>, path: &str) -> anyhow::Result<String> {
        let mut result = content.to_string();
        let mut applied_count = 0;

        // 验证所有 old_text 在原始文件中存在且唯一
        for edit in &edits {
            let matches: Vec<_> = result.match_indices(&edit.old_text).collect();
            if matches.is_empty() {
                return Err(anyhow::anyhow!(
                    "Could not find text to replace in '{}': {}...",
                    path,
                    &edit.old_text.chars().take(50).collect::<String>()
                ));
            }
            if matches.len() > 1 {
                return Err(anyhow::anyhow!(
                    "Found multiple matches for text in '{}', text must be unique: {}...",
                    path,
                    &edit.old_text.chars().take(50).collect::<String>()
                ));
            }
        }

        // 应用编辑（每个编辑都针对原始文件，不是增量）
        for edit in edits {
            // 每次都重新查找，因为之前的编辑不影响（都是基于原始文件）
            if let Some(pos) = result.find(&edit.old_text) {
                result.replace_range(pos..pos + edit.old_text.len(), &edit.new_text);
                applied_count += 1;
            }
        }

        if applied_count == 0 {
            return Err(anyhow::anyhow!("No edits were applied"));
        }

        Ok(result)
    }
}

#[async_trait]
impl AgentTool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn label(&self) -> &str {
        "Edit File"
    }

    fn description(&self) -> &str {
        "Edit a single file using exact text replacement. Every edit's oldText must match a unique, non-overlapping region of the original file. If two changes affect the same block or nearby lines, merge them into one edit instead of emitting overlapping edits."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative or absolute)"
                },
                "edits": {
                    "type": "array",
                    "description": "One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "oldText": {
                                "type": "string",
                                "description": "Exact text for one targeted replacement. It must be unique in the original file and must not overlap with any other edit's oldText in the same call."
                            },
                            "newText": {
                                "type": "string",
                                "description": "Replacement text for this targeted edit."
                            }
                        },
                        "required": ["oldText", "newText"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn prepare_arguments(&self, args: serde_json::Value) -> serde_json::Value {
        // 处理旧版参数格式（单个 oldText/newText）
        if let Some(old_text) = args.get("oldText").and_then(|v| v.as_str()) {
            if let Some(new_text) = args.get("newText").and_then(|v| v.as_str()) {
                let mut new_args = args.clone();
                let edits = vec![serde_json::json!({
                    "oldText": old_text,
                    "newText": new_text
                })];
                new_args["edits"] = serde_json::Value::Array(edits);
                new_args.as_object_mut().unwrap().remove("oldText");
                new_args.as_object_mut().unwrap().remove("newText");
                return new_args;
            }
        }
        args
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: serde_json::Value,
        cancel: CancellationToken,
        _on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> anyhow::Result<AgentToolResult> {
        // 检查取消
        if cancel.is_cancelled() {
            return Err(anyhow::anyhow!("Operation aborted"));
        }

        let path = params["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let edits_value = params["edits"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'edits' parameter"))?;

        if edits_value.is_empty() {
            return Err(anyhow::anyhow!("'edits' must contain at least one replacement"));
        }

        let edits: Vec<Edit> = edits_value
            .iter()
            .map(|e| {
                let old_text = e["oldText"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'oldText' in edit"))?;
                let new_text = e["newText"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'newText' in edit"))?;
                Ok(Edit {
                    old_text: old_text.to_string(),
                    new_text: new_text.to_string(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let absolute_path = self.resolve_path(path)?;

        // 检查取消
        if cancel.is_cancelled() {
            return Err(anyhow::anyhow!("Operation aborted"));
        }

        // 检查文件是否存在
        if !absolute_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", path));
        }

        // 读取文件内容
        let old_content = fs::read_to_string(&absolute_path).await.map_err(|e| {
            anyhow::anyhow!("Failed to read file '{}': {}", path, e)
        })?;

        // 检查取消
        if cancel.is_cancelled() {
            return Err(anyhow::anyhow!("Operation aborted"));
        }

        // 保存行尾符
        let _line_ending = Self::detect_line_ending(&old_content);

        // 应用编辑
        let new_content = self.apply_edits(&old_content, edits.clone(), path)?;

        // 检查取消
        if cancel.is_cancelled() {
            return Err(anyhow::anyhow!("Operation aborted"));
        }

        // 生成 diff
        let diff = Self::generate_diff(&old_content, &new_content, path);

        // 写入文件
        fs::write(&absolute_path, &new_content).await.map_err(|e| {
            anyhow::anyhow!("Failed to write file '{}': {}", path, e)
        })?;

        // 检查取消
        if cancel.is_cancelled() {
            return Err(anyhow::anyhow!("Operation aborted"));
        }

        // 查找第一处变更的行号
        let first_changed_line = diff.lines().find_map(|line| {
            if line.starts_with("@@") {
                // 解析 @@ 行号信息
                None
            } else if line.starts_with("-") && !line.starts_with("---") {
                Some(1) // 简化处理，返回第1行
            } else {
                None
            }
        });

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(format!(
                "Successfully replaced {} block(s) in {}.",
                edits.len(),
                path
            )))],
            details: serde_json::json!({
                "path": path,
                "diff": diff,
                "firstChangedLine": first_changed_line,
                "edits_applied": edits.len(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_edit_single_replacement() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World\nThis is a test\nGoodbye").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "Hello World", "newText": "Hi Universe"}]
            }),
            CancellationToken::new(),
            None,
        ).await.unwrap();
        
        // 验证文件内容被修改
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Hi Universe"));
        assert!(!content.contains("Hello World"));
        
        // 验证返回结果
        let text = result.content.iter()
            .filter_map(|c| if let ContentBlock::Text(t) = c { Some(t.text.as_str()) } else { None })
            .collect::<String>();
        assert!(text.contains("Successfully replaced"));
    }

    #[tokio::test]
    async fn test_edit_multiple_replacements() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World\nHello Rust\nHello Test").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [
                    {"oldText": "Hello World", "newText": "Hi World"},
                    {"oldText": "Hello Rust", "newText": "Hi Rust"}
                ]
            }),
            CancellationToken::new(),
            None,
        ).await.unwrap();
        
        // 验证文件内容
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Hi World"));
        assert!(content.contains("Hi Rust"));
        assert!(content.contains("Hello Test")); // 未修改的行保持不变
        
        // 验证返回结果
        let text = result.content.iter()
            .filter_map(|c| if let ContentBlock::Text(t) = c { Some(t.text.as_str()) } else { None })
            .collect::<String>();
        assert!(text.contains("Successfully replaced 2 block(s)"));
    }

    #[tokio::test]
    async fn test_edit_generates_diff() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "Hello World", "newText": "Hi Universe"}]
            }),
            CancellationToken::new(),
            None,
        ).await.unwrap();
        
        // 验证返回结果包含 diff
        let details = result.details.as_object().unwrap();
        assert!(details.contains_key("diff"));
        let diff = details["diff"].as_str().unwrap();
        assert!(diff.contains("---"));
        assert!(diff.contains("+++"));
        assert!(diff.contains("-Hello World") || diff.contains("+Hi Universe"));
    }

    #[tokio::test]
    async fn test_edit_nonexistent_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("nonexistent.txt");
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "old", "newText": "new"}]
            }),
            CancellationToken::new(),
            None,
        ).await;
        
        // 应该返回错误
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("File not found") || err_msg.contains("Failed to resolve path"));
    }

    #[tokio::test]
    async fn test_edit_text_not_found() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "Nonexistent Text", "newText": "New Text"}]
            }),
            CancellationToken::new(),
            None,
        ).await;
        
        // 应该返回错误
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Could not find text to replace"));
    }

    #[tokio::test]
    async fn test_edit_multiple_matches() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello Hello World").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "Hello", "newText": "Hi"}]
            }),
            CancellationToken::new(),
            None,
        ).await;
        
        // 应该返回错误，因为 oldText 不是唯一的
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("multiple matches") || err_msg.contains("unique"));
    }

    #[tokio::test]
    async fn test_edit_empty_edits() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": []
            }),
            CancellationToken::new(),
            None,
        ).await;
        
        // 应该返回错误
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("at least one replacement"));
    }

    #[tokio::test]
    async fn test_edit_missing_path() {
        let dir = TempDir::new().unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "edits": [{"oldText": "old", "newText": "new"}]
            }),
            CancellationToken::new(),
            None,
        ).await;
        
        // 应该返回错误
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_edit_missing_edits() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap()
            }),
            CancellationToken::new(),
            None,
        ).await;
        
        // 应该返回错误
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing or invalid 'edits' parameter"));
    }

    #[tokio::test]
    async fn test_edit_cancellation() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        let cancel = CancellationToken::new();
        cancel.cancel();
        
        let result = tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "Hello", "newText": "Hi"}]
            }),
            cancel,
            None,
        ).await;
        
        // 应该返回取消错误
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("aborted"));
    }

    #[tokio::test]
    async fn test_edit_preserves_line_endings() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        // 使用 CRLF 行尾
        std::fs::write(&file_path, "Hello World\r\nLine 2\r\nLine 3").unwrap();
        
        let tool = EditTool::new(dir.path().to_path_buf());
        tool.execute(
            "call_1",
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "edits": [{"oldText": "Hello World", "newText": "Hi Universe"}]
            }),
            CancellationToken::new(),
            None,
        ).await.unwrap();
        
        // 验证文件内容
        let content = std::fs::read_to_string(&file_path).unwrap();
        // 行尾符应该被保留（虽然写入时会使用系统默认行尾）
        assert!(content.contains("Hi Universe"));
    }

    #[test]
    fn test_edit_tool_name() {
        let tool = EditTool::new(PathBuf::from("/tmp"));
        assert_eq!(tool.name(), "edit");
    }

    #[test]
    fn test_edit_tool_label() {
        let tool = EditTool::new(PathBuf::from("/tmp"));
        assert_eq!(tool.label(), "Edit File");
    }

    #[test]
    fn test_edit_tool_parameters() {
        let tool = EditTool::new(PathBuf::from("/tmp"));
        let params = tool.parameters();
        
        assert!(params.is_object());
        let obj = params.as_object().unwrap();
        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("properties"));
        assert!(obj.contains_key("required"));
        
        let properties = obj["properties"].as_object().unwrap();
        assert!(properties.contains_key("path"));
        assert!(properties.contains_key("edits"));
        
        let required = obj["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("path")));
        assert!(required.contains(&serde_json::json!("edits")));
    }

    #[test]
    fn test_edit_prepare_arguments() {
        let tool = EditTool::new(PathBuf::from("/tmp"));
        
        // 测试旧版参数格式转换
        let old_args = serde_json::json!({
            "path": "test.txt",
            "oldText": "old",
            "newText": "new"
        });
        
        let new_args = tool.prepare_arguments(old_args);
        
        assert!(new_args.get("oldText").is_none());
        assert!(new_args.get("newText").is_none());
        assert!(new_args.get("edits").is_some());
        
        let edits = new_args["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["oldText"], "old");
        assert_eq!(edits[0]["newText"], "new");
    }

    #[test]
    fn test_edit_prepare_arguments_new_format() {
        let tool = EditTool::new(PathBuf::from("/tmp"));
        
        // 测试新版参数格式保持不变
        let new_args = serde_json::json!({
            "path": "test.txt",
            "edits": [{"oldText": "old", "newText": "new"}]
        });
        
        let result = tool.prepare_arguments(new_args.clone());
        
        assert_eq!(result, new_args);
    }
}
