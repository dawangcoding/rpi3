//! Notebook 状态持久化模块
//!
//! 实现 Notebook 执行状态管理、.pinb 格式保存/加载、Jupyter .ipynb 导入导出

#![allow(dead_code)] // Notebook 功能尚未完全集成

use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use chrono::Utc;

/// Notebook 单元格类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CellType {
    /// 代码单元格
    Code,
    /// Markdown 单元格
    Markdown,
}

/// 单元格输出类型（兼容 Jupyter nbformat v4）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "output_type")]
pub enum CellOutput {
    /// 流输出
    #[serde(rename = "stream")]
    Stream {
        /// 流名称（"stdout" 或 "stderr"）
        name: String,
        /// 文本内容
        text: String,
    },
    /// 执行结果
    #[serde(rename = "execute_result")]
    ExecuteResult {
        /// 数据
        data: HashMap<String, String>,
        /// 执行次数
        execution_count: u32,
        /// 元数据
        metadata: serde_json::Value,
    },
    /// 错误
    #[serde(rename = "error")]
    Error {
        /// 错误名称
        ename: String,
        /// 错误值
        evalue: String,
        /// 堆栈跟踪
        traceback: Vec<String>,
    },
    /// 显示数据
    #[serde(rename = "display_data")]
    DisplayData {
        /// 数据
        data: HashMap<String, String>,
        /// 元数据
        metadata: serde_json::Value,
    },
}

impl CellOutput {
    /// 创建标准输出流
    pub fn stdout(text: impl Into<String>) -> Self {
        CellOutput::Stream {
            name: "stdout".to_string(),
            text: text.into(),
        }
    }

    /// 创建标准错误流
    pub fn stderr(text: impl Into<String>) -> Self {
        CellOutput::Stream {
            name: "stderr".to_string(),
            text: text.into(),
        }
    }

    /// 创建执行结果
    pub fn execute_result(execution_count: u32, text: impl Into<String>) -> Self {
        let mut data = HashMap::new();
        data.insert("text/plain".to_string(), text.into());
        CellOutput::ExecuteResult {
            data,
            execution_count,
            metadata: serde_json::Value::Null,
        }
    }

    /// 创建错误输出
    pub fn error(ename: impl Into<String>, evalue: impl Into<String>, traceback: Vec<String>) -> Self {
        CellOutput::Error {
            ename: ename.into(),
            evalue: evalue.into(),
            traceback,
        }
    }

    /// 创建显示数据（如图片）
    pub fn display_data(data: HashMap<String, String>) -> Self {
        CellOutput::DisplayData {
            data,
            metadata: serde_json::Value::Null,
        }
    }
}

/// Notebook 单元格
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookCell {
    /// 单元格类型
    pub cell_type: CellType,
    /// 源代码内容
    pub source: String,
    /// 编程语言
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// 执行次数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_count: Option<u32>,
    /// 输出结果列表
    #[serde(default)]
    pub outputs: Vec<CellOutput>,
    /// 元数据
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl NotebookCell {
    /// 创建新的代码单元格
    pub fn code(source: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            cell_type: CellType::Code,
            source: source.into(),
            language: Some(language.into()),
            execution_count: None,
            outputs: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// 创建新的 Markdown 单元格
    pub fn markdown(source: impl Into<String>) -> Self {
        Self {
            cell_type: CellType::Markdown,
            source: source.into(),
            language: None,
            execution_count: None,
            outputs: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }
}

/// Kernel 规格信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelSpecInfo {
    /// Kernel 名称，如 "python3" / "node"
    pub name: String,
    /// 显示名称，如 "Python 3" / "Node.js"
    pub display_name: String,
}

impl KernelSpecInfo {
    /// 创建 Python kernel 规格
    pub fn python3() -> Self {
        Self {
            name: "python3".to_string(),
            display_name: "Python 3".to_string(),
        }
    }

    /// 创建 Node.js kernel 规格
    pub fn node() -> Self {
        Self {
            name: "node".to_string(),
            display_name: "Node.js".to_string(),
        }
    }

    /// 根据 language 字符串创建 kernel 规格
    pub fn from_language(language: &str) -> Self {
        match language.to_lowercase().as_str() {
            "python" | "python3" => Self::python3(),
            "javascript" | "typescript" | "node" | "nodejs" => Self::node(),
            _ => Self {
                name: language.to_string(),
                display_name: language.to_string(),
            },
        }
    }
}

/// 语言信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    /// 语言名称，如 "python" / "javascript"
    pub name: String,
    /// 语言版本
    pub version: Option<String>,
    /// 文件扩展名，如 ".py" / ".js"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_extension: Option<String>,
}

impl LanguageInfo {
    /// 根据 language 字符串创建语言信息
    pub fn from_language(language: &str) -> Self {
        match language.to_lowercase().as_str() {
            "python" | "python3" => Self {
                name: "python".to_string(),
                version: None,
                file_extension: Some(".py".to_string()),
            },
            "javascript" | "js" => Self {
                name: "javascript".to_string(),
                version: None,
                file_extension: Some(".js".to_string()),
            },
            "typescript" | "ts" => Self {
                name: "typescript".to_string(),
                version: None,
                file_extension: Some(".ts".to_string()),
            },
            "node" | "nodejs" => Self {
                name: "javascript".to_string(),
                version: None,
                file_extension: Some(".js".to_string()),
            },
            _ => Self {
                name: language.to_string(),
                version: None,
                file_extension: None,
            },
        }
    }
}

/// Notebook 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookMetadata {
    /// Kernel 规格信息
    pub kernel_spec: KernelSpecInfo,
    /// 语言信息
    pub language_info: LanguageInfo,
    /// 创建时间（ISO 8601 格式）
    pub created_at: String,
    /// 修改时间（ISO 8601 格式）
    pub modified_at: String,
    /// pi 版本号
    #[serde(default)]
    pub pi_version: Option<String>,
}

impl NotebookMetadata {
    /// 创建新的元数据
    pub fn new(language: &str) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            kernel_spec: KernelSpecInfo::from_language(language),
            language_info: LanguageInfo::from_language(language),
            created_at: now.clone(),
            modified_at: now,
            pi_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }
    }

    /// 更新修改时间
    pub fn touch(&mut self) {
        self.modified_at = Utc::now().to_rfc3339();
    }
}

/// .pinb 格式包装结构
#[derive(Debug, Serialize, Deserialize)]
struct PinbFormat {
    format_version: String,  // "1.0"
    notebook: NotebookState,
}

/// Jupyter nbformat v4 格式
#[derive(Debug, Serialize, Deserialize)]
struct IpynbFormat {
    nbformat: u32,         // 4
    nbformat_minor: u32,   // 5
    metadata: IpynbMetadata,
    cells: Vec<IpynbCell>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IpynbMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    kernelspec: Option<IpynbKernelSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_info: Option<IpynbLanguageInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IpynbKernelSpec {
    name: String,
    display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct IpynbLanguageInfo {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IpynbCell {
    cell_type: String,     // "code" / "markdown"
    source: Vec<String>,   // 按行分割的源代码（Jupyter 格式要求）
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_count: Option<u32>,
    #[serde(default)]
    outputs: Vec<serde_json::Value>,  // Jupyter 原始输出格式
    #[serde(default)]
    metadata: serde_json::Value,
}

/// Notebook 状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookState {
    /// Notebook 元数据
    pub metadata: NotebookMetadata,
    /// 单元格列表
    pub cells: Vec<NotebookCell>,
    /// 会话 ID
    pub session_id: String,
    /// 执行计数器（内部使用）
    #[serde(default)]
    execution_counter: u32,
}

impl NotebookState {
    /// 创建新的 Notebook 状态
    pub fn new(session_id: String, language: &str) -> Self {
        Self {
            metadata: NotebookMetadata::new(language),
            cells: Vec::new(),
            session_id,
            execution_counter: 0,
        }
    }

    /// 添加代码单元格
    pub fn add_code_cell(&mut self, source: String, language: &str) -> usize {
        let cell = NotebookCell::code(source, language);
        self.cells.push(cell);
        self.metadata.touch();
        self.cells.len() - 1
    }

    /// 添加 Markdown 单元格
    pub fn add_markdown_cell(&mut self, source: String) -> usize {
        let cell = NotebookCell::markdown(source);
        self.cells.push(cell);
        self.metadata.touch();
        self.cells.len() - 1
    }

    /// 更新单元格的执行输出
    pub fn update_cell_output(&mut self, cell_index: usize, outputs: Vec<CellOutput>) -> anyhow::Result<()> {
        let cell = self.cells.get_mut(cell_index)
            .ok_or_else(|| anyhow::anyhow!("Cell index {} out of bounds", cell_index))?;
        cell.outputs = outputs;
        self.metadata.touch();
        Ok(())
    }

    /// 设置单元格的执行计数
    pub fn set_execution_count(&mut self, cell_index: usize) -> anyhow::Result<u32> {
        let cell = self.cells.get_mut(cell_index)
            .ok_or_else(|| anyhow::anyhow!("Cell index {} out of bounds", cell_index))?;
        self.execution_counter += 1;
        cell.execution_count = Some(self.execution_counter);
        self.metadata.touch();
        Ok(self.execution_counter)
    }

    /// 获取执行历史（所有已执行的代码单元格）
    pub fn get_execution_history(&self) -> Vec<&NotebookCell> {
        self.cells
            .iter()
            .filter(|cell| cell.cell_type == CellType::Code && cell.execution_count.is_some())
            .collect()
    }

    /// 获取单元格数量
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 获取指定单元格
    pub fn get_cell(&self, index: usize) -> Option<&NotebookCell> {
        self.cells.get(index)
    }

    /// 获取指定单元格（可变引用）
    pub fn get_cell_mut(&mut self, index: usize) -> Option<&mut NotebookCell> {
        self.cells.get_mut(index)
    }

    /// 保存为 .pinb 格式（JSON）
    pub fn save_pinb(&self, path: &Path) -> anyhow::Result<()> {
        let pinb = PinbFormat {
            format_version: "1.0".to_string(),
            notebook: self.clone(),
        };
        let json = serde_json::to_string_pretty(&pinb)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 从 .pinb 格式加载
    pub fn load_pinb(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let pinb: PinbFormat = serde_json::from_str(&content)?;
        Ok(pinb.notebook)
    }

    /// 导出为 Jupyter .ipynb 格式（nbformat v4）
    pub fn export_ipynb(&self, path: &Path) -> anyhow::Result<()> {
        let ipynb = self.to_ipynb();
        let json = serde_json::to_string_pretty(&ipynb)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 从 Jupyter .ipynb 格式导入
    pub fn import_ipynb(path: &Path, session_id: String) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let ipynb: IpynbFormat = serde_json::from_str(&content)?;
        Self::from_ipynb(ipynb, session_id)
    }

    /// 转换为 Jupyter ipynb 格式
    fn to_ipynb(&self) -> IpynbFormat {
        let cells: Vec<IpynbCell> = self.cells.iter().map(|cell| {
            let cell_type = match cell.cell_type {
                CellType::Code => "code",
                CellType::Markdown => "markdown",
            };

            // 将 source 按行分割，每行末尾加 "\n"
            let source = split_source_to_lines(&cell.source);

            // 转换 outputs
            let outputs: Vec<serde_json::Value> = cell.outputs.iter().map(|output| {
                match output {
                    CellOutput::Stream { name, text } => {
                        let text_lines = split_source_to_lines(text);
                        serde_json::json!({
                            "output_type": "stream",
                            "name": name,
                            "text": text_lines
                        })
                    }
                    CellOutput::ExecuteResult { data, execution_count, metadata } => {
                        let data_json: serde_json::Map<String, serde_json::Value> = data
                            .iter()
                            .map(|(k, v)| {
                                let lines = split_source_to_lines(v);
                                (k.clone(), serde_json::Value::Array(
                                    lines.into_iter().map(serde_json::Value::String).collect()
                                ))
                            })
                            .collect();
                        serde_json::json!({
                            "output_type": "execute_result",
                            "execution_count": execution_count,
                            "data": data_json,
                            "metadata": metadata
                        })
                    }
                    CellOutput::Error { ename, evalue, traceback } => {
                        serde_json::json!({
                            "output_type": "error",
                            "ename": ename,
                            "evalue": evalue,
                            "traceback": traceback
                        })
                    }
                    CellOutput::DisplayData { data, metadata } => {
                        let data_json: serde_json::Map<String, serde_json::Value> = data
                            .iter()
                            .map(|(k, v)| {
                                // base64 图片数据不需要分割
                                (k.clone(), serde_json::Value::String(v.clone()))
                            })
                            .collect();
                        serde_json::json!({
                            "output_type": "display_data",
                            "data": data_json,
                            "metadata": metadata
                        })
                    }
                }
            }).collect();

            IpynbCell {
                cell_type: cell_type.to_string(),
                source,
                execution_count: cell.execution_count,
                outputs,
                metadata: cell.metadata.clone(),
            }
        }).collect();

        IpynbFormat {
            nbformat: 4,
            nbformat_minor: 5,
            metadata: IpynbMetadata {
                kernelspec: Some(IpynbKernelSpec {
                    name: self.metadata.kernel_spec.name.clone(),
                    display_name: self.metadata.kernel_spec.display_name.clone(),
                }),
                language_info: Some(IpynbLanguageInfo {
                    name: self.metadata.language_info.name.clone(),
                    version: self.metadata.language_info.version.clone(),
                }),
            },
            cells,
        }
    }

    /// 从 Jupyter ipynb 格式转换
    fn from_ipynb(ipynb: IpynbFormat, session_id: String) -> anyhow::Result<Self> {
        let (kernel_name, kernel_display_name) = ipynb.metadata.kernelspec
            .map(|k| (k.name, k.display_name))
            .unwrap_or_else(|| ("python3".to_string(), "Python 3".to_string()));

        let (language_name, language_version) = ipynb.metadata.language_info
            .map(|l| (l.name, l.version))
            .unwrap_or_else(|| ("python".to_string(), None));

        let now = Utc::now().to_rfc3339();

        let mut cells = Vec::new();
        let mut execution_counter = 0u32;

        for ipynb_cell in ipynb.cells {
            let cell_type = match ipynb_cell.cell_type.as_str() {
                "code" => CellType::Code,
                "markdown" => CellType::Markdown,
                _ => continue, // 跳过未知类型
            };

            // 合并 source 行
            let source = ipynb_cell.source.join("");

            // 转换 outputs
            let outputs: Vec<CellOutput> = ipynb_cell.outputs
                .into_iter()
                .filter_map(|output| {
                    let output_type = output.get("output_type")?.as_str()?;
                    match output_type {
                        "stream" => {
                            let name = output.get("name")?.as_str()?.to_string();
                            let text_lines = output.get("text")?.as_array()?;
                            let text = text_lines.iter()
                                .filter_map(|l| l.as_str())
                                .collect::<String>();
                            Some(CellOutput::Stream { name, text })
                        }
                        "execute_result" => {
                            let execution_count = output.get("execution_count")?.as_u64()? as u32;
                            let data_obj = output.get("data")?.as_object()?;
                            let data: HashMap<String, String> = data_obj.iter()
                                .filter_map(|(k, v)| {
                                    if let serde_json::Value::Array(lines) = v {
                                        let text = lines.iter()
                                            .filter_map(|l| l.as_str())
                                            .collect::<String>();
                                        Some((k.clone(), text))
                                    } else if let serde_json::Value::String(s) = v {
                                        Some((k.clone(), s.clone()))
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            let metadata = output.get("metadata").cloned().unwrap_or(serde_json::Value::Null);
                            Some(CellOutput::ExecuteResult { data, execution_count, metadata })
                        }
                        "error" => {
                            let ename = output.get("ename")?.as_str()?.to_string();
                            let evalue = output.get("evalue")?.as_str()?.to_string();
                            let traceback = output.get("traceback")
                                .and_then(|t| t.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default();
                            Some(CellOutput::Error { ename, evalue, traceback })
                        }
                        "display_data" => {
                            let data_obj = output.get("data")?.as_object()?;
                            let data: HashMap<String, String> = data_obj.iter()
                                .filter_map(|(k, v)| {
                                    if let serde_json::Value::String(s) = v {
                                        Some((k.clone(), s.clone()))
                                    } else if let serde_json::Value::Array(lines) = v {
                                        let text = lines.iter()
                                            .filter_map(|l| l.as_str())
                                            .collect::<String>();
                                        Some((k.clone(), text))
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            let metadata = output.get("metadata").cloned().unwrap_or(serde_json::Value::Null);
                            Some(CellOutput::DisplayData { data, metadata })
                        }
                        _ => None,
                    }
                })
                .collect();

            // 更新最大执行计数
            if let Some(ec) = ipynb_cell.execution_count {
                execution_counter = execution_counter.max(ec);
            }

            cells.push(NotebookCell {
                cell_type: cell_type.clone(),
                source,
                language: if cell_type == CellType::Code {
                    Some(language_name.clone())
                } else {
                    None
                },
                execution_count: ipynb_cell.execution_count,
                outputs,
                metadata: ipynb_cell.metadata,
            });
        }

        Ok(Self {
            metadata: NotebookMetadata {
                kernel_spec: KernelSpecInfo {
                    name: kernel_name,
                    display_name: kernel_display_name,
                },
                language_info: LanguageInfo {
                    name: language_name,
                    version: language_version,
                    file_extension: None,
                },
                created_at: now.clone(),
                modified_at: now,
                pi_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            cells,
            session_id,
            execution_counter,
        })
    }
}

/// 将源代码按行分割，每行末尾加 "\n"
fn split_source_to_lines(source: &str) -> Vec<String> {
    source.lines()
        .map(|line| {
            if source.ends_with('\n') || !source.ends_with(line) {
                format!("{}\n", line)
            } else {
                line.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_notebook_state_new() {
        let state = NotebookState::new("test-session".to_string(), "python");
        
        assert_eq!(state.session_id, "test-session");
        assert_eq!(state.cell_count(), 0);
        assert_eq!(state.execution_counter, 0);
        assert_eq!(state.metadata.kernel_spec.name, "python3");
        assert_eq!(state.metadata.language_info.name, "python");
    }

    #[test]
    fn test_add_code_cell() {
        let mut state = NotebookState::new("test".to_string(), "python");
        let idx = state.add_code_cell("print('hello')".to_string(), "python");
        
        assert_eq!(idx, 0);
        assert_eq!(state.cell_count(), 1);
        
        let cell = state.get_cell(0).unwrap();
        assert_eq!(cell.cell_type, CellType::Code);
        assert_eq!(cell.source, "print('hello')");
        assert_eq!(cell.language, Some("python".to_string()));
        assert!(cell.execution_count.is_none());
    }

    #[test]
    fn test_add_markdown_cell() {
        let mut state = NotebookState::new("test".to_string(), "python");
        let idx = state.add_markdown_cell("# Title".to_string());
        
        assert_eq!(idx, 0);
        assert_eq!(state.cell_count(), 1);
        
        let cell = state.get_cell(0).unwrap();
        assert_eq!(cell.cell_type, CellType::Markdown);
        assert_eq!(cell.source, "# Title");
        assert!(cell.language.is_none());
    }

    #[test]
    fn test_update_cell_output() {
        let mut state = NotebookState::new("test".to_string(), "python");
        let idx = state.add_code_cell("print('hello')".to_string(), "python");
        
        let outputs = vec![CellOutput::stdout("hello\n")];
        state.update_cell_output(idx, outputs).unwrap();
        
        let cell = state.get_cell(idx).unwrap();
        assert_eq!(cell.outputs.len(), 1);
        
        if let CellOutput::Stream { name, text } = &cell.outputs[0] {
            assert_eq!(name, "stdout");
            assert_eq!(text, "hello\n");
        } else {
            panic!("Expected Stream output");
        }
    }

    #[test]
    fn test_execution_counter() {
        let mut state = NotebookState::new("test".to_string(), "python");
        
        let idx1 = state.add_code_cell("1 + 1".to_string(), "python");
        let idx2 = state.add_code_cell("2 + 2".to_string(), "python");
        
        let ec1 = state.set_execution_count(idx1).unwrap();
        let ec2 = state.set_execution_count(idx2).unwrap();
        
        assert_eq!(ec1, 1);
        assert_eq!(ec2, 2);
        
        assert_eq!(state.get_cell(idx1).unwrap().execution_count, Some(1));
        assert_eq!(state.get_cell(idx2).unwrap().execution_count, Some(2));
    }

    #[test]
    fn test_get_execution_history() {
        let mut state = NotebookState::new("test".to_string(), "python");
        
        // 添加一些单元格
        let idx1 = state.add_code_cell("x = 1".to_string(), "python");
        state.add_markdown_cell("# Comment".to_string());
        let _idx3 = state.add_code_cell("y = 2".to_string(), "python");
        
        // 只执行部分单元格
        state.set_execution_count(idx1).unwrap();
        // idx3 不执行
        
        let history = state.get_execution_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].source, "x = 1");
    }

    #[test]
    fn test_save_and_load_pinb() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.pinb");
        
        // 创建 state
        let mut state = NotebookState::new("test-session".to_string(), "python");
        let idx = state.add_code_cell("print('hello')".to_string(), "python");
        state.set_execution_count(idx).unwrap();
        state.update_cell_output(idx, vec![CellOutput::stdout("hello\n")]).unwrap();
        
        // 保存
        state.save_pinb(&path).unwrap();
        
        // 加载
        let loaded = NotebookState::load_pinb(&path).unwrap();
        
        // 验证一致性
        assert_eq!(loaded.session_id, state.session_id);
        assert_eq!(loaded.cell_count(), state.cell_count());
        assert_eq!(loaded.get_cell(0).unwrap().source, "print('hello')");
        assert_eq!(loaded.get_cell(0).unwrap().execution_count, Some(1));
    }

    #[test]
    fn test_export_and_import_ipynb() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.ipynb");
        
        // 创建 state
        let mut state = NotebookState::new("test-session".to_string(), "python");
        state.add_markdown_cell("# Title".to_string());
        let idx = state.add_code_cell("print('hello')".to_string(), "python");
        state.set_execution_count(idx).unwrap();
        state.update_cell_output(idx, vec![CellOutput::stdout("hello\n")]).unwrap();
        
        // 导出
        state.export_ipynb(&path).unwrap();
        
        // 验证 ipynb 格式结构
        let content = std::fs::read_to_string(&path).unwrap();
        let ipynb: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(ipynb["nbformat"], 4);
        assert_eq!(ipynb["nbformat_minor"], 5);
        assert!(ipynb["metadata"]["kernelspec"].is_object());
        
        // 导入
        let imported = NotebookState::import_ipynb(&path, "new-session".to_string()).unwrap();
        
        // 验证一致性
        assert_eq!(imported.session_id, "new-session");
        assert_eq!(imported.cell_count(), 2);
        assert_eq!(imported.get_cell(0).unwrap().cell_type, CellType::Markdown);
        assert_eq!(imported.get_cell(1).unwrap().cell_type, CellType::Code);
        assert_eq!(imported.get_cell(1).unwrap().execution_count, Some(1));
    }

    #[test]
    fn test_ipynb_source_line_splitting() {
        let mut state = NotebookState::new("test".to_string(), "python");
        state.add_code_cell("line1\nline2\nline3".to_string(), "python");
        
        let ipynb = state.to_ipynb();
        let cell = &ipynb.cells[0];
        
        // 验证 source 按行分割
        assert_eq!(cell.source.len(), 3);
        assert_eq!(cell.source[0], "line1\n");
        assert_eq!(cell.source[1], "line2\n");
        assert_eq!(cell.source[2], "line3");  // 最后一行不加换行
    }

    #[test]
    fn test_cell_output_serialization() {
        // 测试 Stream 输出
        let output = CellOutput::stdout("hello world\n");
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"output_type\":\"stream\""));
        assert!(json.contains("\"name\":\"stdout\""));
        assert!(json.contains("\"text\":\"hello world\\n\""));
        
        let deserialized: CellOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, output);
        
        // 测试 ExecuteResult 输出
        let output = CellOutput::execute_result(1, "42");
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"output_type\":\"execute_result\""));
        assert!(json.contains("\"execution_count\":1"));
        
        // 测试 Error 输出
        let output = CellOutput::error("NameError", "name 'x' is not defined", vec!["line 1".to_string()]);
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"output_type\":\"error\""));
        assert!(json.contains("\"ename\":\"NameError\""));
        
        // 测试 DisplayData 输出
        let mut data = HashMap::new();
        data.insert("image/png".to_string(), "base64data".to_string());
        let output = CellOutput::display_data(data);
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"output_type\":\"display_data\""));
    }

    #[test]
    fn test_cell_output_equality() {
        let o1 = CellOutput::stdout("hello");
        let o2 = CellOutput::stdout("hello");
        let o3 = CellOutput::stderr("hello");
        
        assert_eq!(o1, o2);
        assert_ne!(o1, o3);
    }

    #[test]
    fn test_kernel_spec() {
        let ks = KernelSpecInfo::python3();
        assert_eq!(ks.name, "python3");
        assert_eq!(ks.display_name, "Python 3");
        
        let ks = KernelSpecInfo::from_language("javascript");
        assert_eq!(ks.name, "node");
    }

    #[test]
    fn test_language_info() {
        let li = LanguageInfo::from_language("python");
        assert_eq!(li.name, "python");
        assert_eq!(li.file_extension, Some(".py".to_string()));
        
        let li = LanguageInfo::from_language("typescript");
        assert_eq!(li.name, "typescript");
        assert_eq!(li.file_extension, Some(".ts".to_string()));
    }

    #[test]
    fn test_update_cell_output_out_of_bounds() {
        let mut state = NotebookState::new("test".to_string(), "python");
        let result = state.update_cell_output(99, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_execution_count_out_of_bounds() {
        let mut state = NotebookState::new("test".to_string(), "python");
        let result = state.set_execution_count(99);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_cells() {
        let mut state = NotebookState::new("test".to_string(), "python");
        
        let idx1 = state.add_markdown_cell("# Intro".to_string());
        let idx2 = state.add_code_cell("x = 1".to_string(), "python");
        let idx3 = state.add_code_cell("y = 2".to_string(), "python");
        let idx4 = state.add_markdown_cell("## Results".to_string());
        let idx5 = state.add_code_cell("print(x + y)".to_string(), "python");
        
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 2);
        assert_eq!(idx4, 3);
        assert_eq!(idx5, 4);
        assert_eq!(state.cell_count(), 5);
        
        // 执行代码单元格
        state.set_execution_count(idx2).unwrap();
        state.set_execution_count(idx3).unwrap();
        state.set_execution_count(idx5).unwrap();
        
        let history = state.get_execution_history();
        assert_eq!(history.len(), 3);
    }
}
