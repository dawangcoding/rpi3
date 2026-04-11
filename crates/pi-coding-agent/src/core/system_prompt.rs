//! 系统提示词构建模块
//!
//! 负责构建 Agent 的系统提示词，包括工具描述、指南、上下文文件等

use pi_agent::types::AgentTool;
use std::path::Path;

/// 系统提示词构建选项
pub struct BuildSystemPromptOptions {
    /// 自定义系统提示词（完全替换默认）
    pub custom_prompt: Option<String>,
    /// 追加到系统提示词末尾
    pub append_system_prompt: Option<String>,
    /// 选择的工具列表
    pub tools: Vec<std::sync::Arc<dyn AgentTool>>,
    /// 自定义指南
    pub guidelines: Vec<String>,
    /// 项目上下文文件
    pub context_files: Vec<ContextFile>,
    /// 当前工作目录
    pub cwd: std::path::PathBuf,
}

/// 上下文文件
#[derive(Debug, Clone)]
pub struct ContextFile {
    /// 文件路径
    pub path: String,
    /// 文件内容
    pub content: String,
}

/// 构建系统提示词
pub fn build_system_prompt(options: &BuildSystemPromptOptions) -> String {
    if let Some(custom) = &options.custom_prompt {
        let mut prompt = custom.clone();
        
        // 追加内容
        if let Some(append) = &options.append_system_prompt {
            prompt.push_str("\n\n");
            prompt.push_str(append);
        }
        
        // 添加上下文文件
        if !options.context_files.is_empty() {
            prompt.push_str("\n\n# Project Context\n\n");
            prompt.push_str("Project-specific instructions and guidelines:\n\n");
            for ctx in &options.context_files {
                prompt.push_str(&format!("## {}\n\n{}\n\n", ctx.path, ctx.content));
            }
        }
        
        // 添加日期和工作目录
        let date = chrono::Local::now().format("%Y-%m-%d");
        let prompt_cwd = options.cwd.to_string_lossy().replace('\\', "/");
        prompt.push_str(&format!("\nCurrent date: {}", date));
        prompt.push_str(&format!("\nCurrent working directory: {}", prompt_cwd));
        
        return prompt;
    }

    let mut prompt = String::new();
    
    // 基础角色描述
    prompt.push_str("You are an expert coding assistant operating inside pi, a coding agent harness. ");
    prompt.push_str("You help users by reading files, executing commands, editing code, and writing new files.\n\n");
    
    // 可用工具列表
    prompt.push_str("Available tools:\n");
    for tool in &options.tools {
        prompt.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
    }
    prompt.push('\n');
    
    // 指南
    prompt.push_str("Guidelines:\n");
    
    // 检查有哪些工具
    let has_bash = options.tools.iter().any(|t| t.name() == "bash");
    let has_grep = options.tools.iter().any(|t| t.name() == "grep");
    let has_find = options.tools.iter().any(|t| t.name() == "find");
    let has_ls = options.tools.iter().any(|t| t.name() == "ls");
    let has_notebook = options.tools.iter().any(|t| t.name() == "notebook");
    
    // 根据可用工具添加指南
    if has_bash && !has_grep && !has_find && !has_ls {
        prompt.push_str("- Use bash for file operations like ls, rg, find\n");
    } else if has_bash && (has_grep || has_find || has_ls) {
        prompt.push_str("- Prefer grep/find/ls tools over bash for file exploration (faster, respects .gitignore)\n");
    }
    
    // Notebook 工具指南
    if has_notebook {
        prompt.push_str("- Use the notebook tool to execute Python or Node.js code when you need to run calculations, data analysis, or generate visualizations\n");
        prompt.push_str("- Notebook tool captures stdout, stderr, and image outputs from code execution\n");
    }
    
    // 添加自定义指南
    for guideline in &options.guidelines {
        prompt.push_str(&format!("- {}\n", guideline));
    }
    
    // 默认指南
    prompt.push_str("- Be concise in your responses\n");
    prompt.push_str("- Show file paths clearly when working with files\n");
    prompt.push_str("- When editing files, use the edit tool for precise changes\n");
    prompt.push_str("- When creating new files, use the write tool\n");
    prompt.push_str("- Always verify your changes work correctly\n");
    prompt.push('\n');
    
    // 追加内容
    if let Some(append) = &options.append_system_prompt {
        prompt.push_str(append);
        prompt.push_str("\n\n");
    }
    
    // 上下文文件
    if !options.context_files.is_empty() {
        prompt.push_str("# Project Context\n\n");
        prompt.push_str("Project-specific instructions and guidelines:\n\n");
        for ctx in &options.context_files {
            prompt.push_str(&format!("## {}\n\n{}\n\n", ctx.path, ctx.content));
        }
    }
    
    // 元信息
    let date = chrono::Local::now().format("%Y-%m-%d");
    let prompt_cwd = options.cwd.to_string_lossy().replace('\\', "/");
    prompt.push_str(&format!("Current date: {}", date));
    prompt.push_str(&format!("\nCurrent working directory: {}", prompt_cwd));
    
    prompt
}

/// 加载上下文文件
pub fn load_context_files(paths: &[String], cwd: &Path) -> Vec<ContextFile> {
    let mut result = Vec::new();
    
    for path in paths {
        let full_path = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            cwd.join(path)
        };
        
        if let Ok(content) = std::fs::read_to_string(&full_path) {
            result.push(ContextFile {
                path: path.clone(),
                content,
            });
        }
    }
    
    result
}

/// 查找默认上下文文件
pub fn find_default_context_files(cwd: &Path) -> Vec<String> {
    let mut result = Vec::new();
    
    // 查找 .pi/context.md
    let pi_context = cwd.join(".pi").join("context.md");
    if pi_context.exists() {
        result.push(pi_context.to_string_lossy().to_string());
    }
    
    // 查找 CLAUDE.md
    let claude_md = cwd.join("CLAUDE.md");
    if claude_md.exists() {
        result.push(claude_md.to_string_lossy().to_string());
    }
    
    // 查找 .cursorrules
    let cursor_rules = cwd.join(".cursorrules");
    if cursor_rules.exists() {
        result.push(cursor_rules.to_string_lossy().to_string());
    }
    
    result
}

/// 查找并加载所有上下文文件（包括默认文件）
pub fn load_all_context_files(explicit_paths: &[String], cwd: &Path) -> Vec<ContextFile> {
    let mut all_paths = explicit_paths.to_vec();
    
    // 添加默认上下文文件
    let default_files = find_default_context_files(cwd);
    for file in default_files {
        if !all_paths.contains(&file) {
            all_paths.push(file);
        }
    }
    
    load_context_files(&all_paths, cwd)
}
