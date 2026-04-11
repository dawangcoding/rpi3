//! 技能系统
//!
//! 提供预设技能库框架，支持技能注册、查询、搜索和参数化应用

#![allow(dead_code)] // 技能系统尚未完全集成

/// 技能类型定义模块
pub mod types;
/// 技能注册表模块
pub mod registry;
/// 内置技能模块
pub mod builtin;

pub use registry::SkillRegistry;
pub use builtin::builtin_skills;

/// 创建包含所有内置技能的注册表
pub fn create_default_registry() -> SkillRegistry {
    let mut registry = SkillRegistry::new();
    for skill in builtin_skills() {
        registry.register(skill).expect("Failed to register builtin skill");
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::types::SkillCategory;

    #[test]
    fn test_create_default_registry() {
        let registry = create_default_registry();
        assert_eq!(registry.count(), 5);
    }

    #[test]
    fn test_default_registry_contains_builtin_skills() {
        let registry = create_default_registry();
        
        assert!(registry.get("code-review").is_some());
        assert!(registry.get("refactoring").is_some());
        assert!(registry.get("doc-generation").is_some());
        assert!(registry.get("bug-analysis").is_some());
        assert!(registry.get("performance-optimization").is_some());
    }

    #[test]
    fn test_all_builtin_skills_are_marked() {
        let registry = create_default_registry();
        
        for skill in registry.list() {
            assert!(skill.builtin, "Skill '{}' should be marked as builtin", skill.id);
        }
    }

    #[test]
    fn test_categories_covered() {
        let registry = create_default_registry();
        let categories = registry.categories();
        
        // 验证至少包含这些分类
        assert!(categories.contains(&SkillCategory::CodeReview));
        assert!(categories.contains(&SkillCategory::Refactoring));
        assert!(categories.contains(&SkillCategory::Documentation));
        assert!(categories.contains(&SkillCategory::Debugging));
        assert!(categories.contains(&SkillCategory::Performance));
    }

    #[test]
    fn test_search_functionality() {
        let registry = create_default_registry();
        
        // 搜索 "review" 应该找到 code-review
        let results = registry.search("review");
        assert!(!results.is_empty());
        assert!(results.iter().any(|s| s.id == "code-review"));
        
        // 搜索 "performance" 应该找到 performance-optimization
        let results = registry.search("performance");
        assert!(!results.is_empty());
        assert!(results.iter().any(|s| s.id == "performance-optimization"));
    }

    #[test]
    fn test_list_by_category() {
        let registry = create_default_registry();
        
        let code_review_skills = registry.list_by_category(&SkillCategory::CodeReview);
        assert_eq!(code_review_skills.len(), 1);
        assert_eq!(code_review_skills[0].id, "code-review");
    }
}
