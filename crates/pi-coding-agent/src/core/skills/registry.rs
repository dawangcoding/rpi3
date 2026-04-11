use std::collections::HashMap;
use std::collections::HashSet;
use anyhow::Result;
use super::types::{Skill, SkillCategory};

/// 技能注册表
pub struct SkillRegistry {
    /// 技能映射表
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    /// 创建新的技能注册表
    pub fn new() -> Self {
        Self { skills: HashMap::new() }
    }
    
    /// 注册一个技能
    pub fn register(&mut self, skill: Skill) -> Result<()> {
        if self.skills.contains_key(&skill.id) {
            anyhow::bail!("Skill '{}' already registered", skill.id);
        }
        self.skills.insert(skill.id.clone(), skill);
        Ok(())
    }
    
    /// 注销一个技能
    pub fn unregister(&mut self, id: &str) -> Option<Skill> {
        self.skills.remove(id)
    }
    
    /// 获取技能
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id)
    }
    
    /// 获取所有技能
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }
    
    /// 按分类筛选
    pub fn list_by_category(&self, category: &SkillCategory) -> Vec<&Skill> {
        self.skills.values()
            .filter(|s| &s.category == category)
            .collect()
    }
    
    /// 搜索技能（名称或描述匹配）
    pub fn search(&self, query: &str) -> Vec<&Skill> {
        let query_lower = query.to_lowercase();
        self.skills.values()
            .filter(|s| {
                s.name.to_lowercase().contains(&query_lower)
                || s.description.to_lowercase().contains(&query_lower)
                || s.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }
    
    /// 技能数量
    pub fn count(&self) -> usize {
        self.skills.len()
    }
    
    /// 获取所有分类
    pub fn categories(&self) -> Vec<SkillCategory> {
        let cats: HashSet<SkillCategory> = self.skills.values()
            .map(|s| s.category.clone())
            .collect();
        let mut result: Vec<_> = cats.into_iter().collect();
        result.sort_by(|a, b| format!("{:?}", a).cmp(&format!("{:?}", b)));
        result
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{SkillParameter, ParameterType};

    fn create_test_skill(id: &str, name: &str, category: SkillCategory) -> Skill {
        Skill {
            id: id.to_string(),
            name: name.to_string(),
            description: format!("Description for {}", name),
            prompt_template: "Template {code}".to_string(),
            parameters: vec![],
            category,
            tags: vec!["test".to_string()],
            builtin: false,
        }
    }

    #[test]
    fn test_new_registry_is_empty() {
        let registry = SkillRegistry::new();
        assert_eq!(registry.count(), 0);
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_register_skill() {
        let mut registry = SkillRegistry::new();
        let skill = create_test_skill("test-skill", "Test Skill", SkillCategory::Custom);
        
        registry.register(skill).unwrap();
        assert_eq!(registry.count(), 1);
        assert!(registry.get("test-skill").is_some());
    }

    #[test]
    fn test_register_duplicate_fails() {
        let mut registry = SkillRegistry::new();
        let skill1 = create_test_skill("test-skill", "Test 1", SkillCategory::Custom);
        let skill2 = create_test_skill("test-skill", "Test 2", SkillCategory::Custom);
        
        registry.register(skill1).unwrap();
        let result = registry.register(skill2);
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }

    #[test]
    fn test_unregister_skill() {
        let mut registry = SkillRegistry::new();
        let skill = create_test_skill("test-skill", "Test", SkillCategory::Custom);
        
        registry.register(skill).unwrap();
        assert_eq!(registry.count(), 1);
        
        let removed = registry.unregister("test-skill");
        assert!(removed.is_some());
        assert_eq!(registry.count(), 0);
        assert!(registry.get("test-skill").is_none());
    }

    #[test]
    fn test_unregister_nonexistent() {
        let mut registry = SkillRegistry::new();
        let result = registry.unregister("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_skill() {
        let mut registry = SkillRegistry::new();
        let skill = create_test_skill("my-skill", "My Skill", SkillCategory::CodeReview);
        
        registry.register(skill).unwrap();
        
        let retrieved = registry.get("my-skill").unwrap();
        assert_eq!(retrieved.name, "My Skill");
        assert_eq!(retrieved.category, SkillCategory::CodeReview);
    }

    #[test]
    fn test_list_skills() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "Skill 1", SkillCategory::Custom)).unwrap();
        registry.register(create_test_skill("s2", "Skill 2", SkillCategory::Custom)).unwrap();
        registry.register(create_test_skill("s3", "Skill 3", SkillCategory::Custom)).unwrap();
        
        let list = registry.list();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_list_by_category() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "Skill 1", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("s2", "Skill 2", SkillCategory::Refactoring)).unwrap();
        registry.register(create_test_skill("s3", "Skill 3", SkillCategory::CodeReview)).unwrap();
        
        let code_review_skills = registry.list_by_category(&SkillCategory::CodeReview);
        assert_eq!(code_review_skills.len(), 2);
        
        let refactoring_skills = registry.list_by_category(&SkillCategory::Refactoring);
        assert_eq!(refactoring_skills.len(), 1);
        
        let testing_skills = registry.list_by_category(&SkillCategory::Testing);
        assert_eq!(testing_skills.len(), 0);
    }

    #[test]
    fn test_search_by_name() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("code-review", "Code Review", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("refactor", "Refactoring Helper", SkillCategory::Refactoring)).unwrap();
        registry.register(create_test_skill("bug-fix", "Bug Fixer", SkillCategory::Debugging)).unwrap();
        
        let results = registry.search("code");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "code-review");
    }

    #[test]
    fn test_search_by_description() {
        let mut registry = SkillRegistry::new();
        let mut skill = create_test_skill("s1", "Skill", SkillCategory::Custom);
        skill.description = "Analyzes performance bottlenecks".to_string();
        registry.register(skill).unwrap();
        
        let results = registry.search("performance");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_by_tag() {
        let mut registry = SkillRegistry::new();
        let mut skill = create_test_skill("s1", "Skill", SkillCategory::Custom);
        skill.tags = vec!["security".to_string(), "audit".to_string()];
        registry.register(skill).unwrap();
        
        let results = registry.search("security");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("code-review", "Code Review", SkillCategory::CodeReview)).unwrap();
        
        let results = registry.search("CODE");
        assert_eq!(results.len(), 1);
        
        let results = registry.search("Review");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_match() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "Skill", SkillCategory::Custom)).unwrap();
        
        let results = registry.search("nonexistent");
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_categories() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "S1", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("s2", "S2", SkillCategory::Refactoring)).unwrap();
        registry.register(create_test_skill("s3", "S3", SkillCategory::CodeReview)).unwrap();
        
        let categories = registry.categories();
        assert_eq!(categories.len(), 2);
        assert!(categories.contains(&SkillCategory::CodeReview));
        assert!(categories.contains(&SkillCategory::Refactoring));
    }

    #[test]
    fn test_default() {
        let registry = SkillRegistry::default();
        assert_eq!(registry.count(), 0);
    }

    // ========== 搜索功能扩展测试 ==========

    #[test]
    fn test_search_fuzzy_matching() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("code-review", "Code Review", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("refactor", "Refactoring Helper", SkillCategory::Refactoring)).unwrap();
        registry.register(create_test_skill("bug-fix", "Bug Fixer", SkillCategory::Debugging)).unwrap();
        
        // 模糊搜索 - 部分匹配
        let results = registry.search("rev");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "code-review");
        
        // 模糊搜索 - 多词匹配
        let results = registry.search("factor");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "refactor");
    }

    #[test]
    fn test_search_multiple_results() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("code-review", "Code Review", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("doc-review", "Documentation Review", SkillCategory::Documentation)).unwrap();
        registry.register(create_test_skill("bug-fix", "Bug Fixer", SkillCategory::Debugging)).unwrap();
        
        // 搜索 "review" 应该返回两个结果
        let results = registry.search("review");
        assert_eq!(results.len(), 2);
        let ids: Vec<_> = results.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"code-review"));
        assert!(ids.contains(&"doc-review"));
    }

    #[test]
    fn test_search_by_multiple_tags() {
        let mut registry = SkillRegistry::new();
        let mut skill1 = create_test_skill("s1", "Skill 1", SkillCategory::Custom);
        skill1.tags = vec!["performance".to_string(), "optimization".to_string()];
        registry.register(skill1).unwrap();
        
        let mut skill2 = create_test_skill("s2", "Skill 2", SkillCategory::Custom);
        skill2.tags = vec!["security".to_string(), "audit".to_string()];
        registry.register(skill2).unwrap();
        
        // 通过不同标签搜索
        assert_eq!(registry.search("performance").len(), 1);
        assert_eq!(registry.search("optimization").len(), 1);
        assert_eq!(registry.search("security").len(), 1);
        assert_eq!(registry.search("audit").len(), 1);
    }

    // ========== 分类过滤扩展测试 ==========

    #[test]
    fn test_list_by_category_all_categories() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "Skill 1", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("s2", "Skill 2", SkillCategory::Refactoring)).unwrap();
        registry.register(create_test_skill("s3", "Skill 3", SkillCategory::Documentation)).unwrap();
        registry.register(create_test_skill("s4", "Skill 4", SkillCategory::Debugging)).unwrap();
        registry.register(create_test_skill("s5", "Skill 5", SkillCategory::Performance)).unwrap();
        registry.register(create_test_skill("s6", "Skill 6", SkillCategory::Testing)).unwrap();
        registry.register(create_test_skill("s7", "Skill 7", SkillCategory::Security)).unwrap();
        registry.register(create_test_skill("s8", "Skill 8", SkillCategory::Custom)).unwrap();
        
        // 验证所有分类都能正确过滤
        assert_eq!(registry.list_by_category(&SkillCategory::CodeReview).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Refactoring).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Documentation).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Debugging).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Performance).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Testing).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Security).len(), 1);
        assert_eq!(registry.list_by_category(&SkillCategory::Custom).len(), 1);
    }

    #[test]
    fn test_categories_empty_registry() {
        let registry = SkillRegistry::new();
        let categories = registry.categories();
        assert!(categories.is_empty());
    }

    #[test]
    fn test_categories_deduplication() {
        let mut registry = SkillRegistry::new();
        // 注册多个相同分类的技能
        registry.register(create_test_skill("s1", "Skill 1", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("s2", "Skill 2", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("s3", "Skill 3", SkillCategory::CodeReview)).unwrap();
        
        let categories = registry.categories();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0], SkillCategory::CodeReview);
    }

    // ========== 参数验证测试 ==========

    #[test]
    fn test_skill_with_parameters() {
        let mut registry = SkillRegistry::new();
        let skill = Skill {
            id: "test-params".to_string(),
            name: "Test with Params".to_string(),
            description: "A skill with parameters".to_string(),
            prompt_template: "Process {input} with {option}".to_string(),
            parameters: vec![
                SkillParameter {
                    name: "input".to_string(),
                    description: "Input data".to_string(),
                    param_type: ParameterType::String,
                    required: true,
                    default: None,
                },
                SkillParameter {
                    name: "option".to_string(),
                    description: "Option setting".to_string(),
                    param_type: ParameterType::Enum(vec!["fast".to_string(), "slow".to_string()]),
                    required: false,
                    default: Some("fast".to_string()),
                },
            ],
            category: SkillCategory::Custom,
            tags: vec!["test".to_string()],
            builtin: false,
        };
        
        registry.register(skill).unwrap();
        
        let retrieved = registry.get("test-params").unwrap();
        assert_eq!(retrieved.parameters.len(), 2);
        
        // 验证参数属性
        let input_param = &retrieved.parameters[0];
        assert_eq!(input_param.name, "input");
        assert!(input_param.required);
        assert!(input_param.default.is_none());
        
        let option_param = &retrieved.parameters[1];
        assert_eq!(option_param.name, "option");
        assert!(!option_param.required);
        assert_eq!(option_param.default, Some("fast".to_string()));
        
        // 验证枚举类型
        if let ParameterType::Enum(options) = &option_param.param_type {
            assert_eq!(options.len(), 2);
            assert!(options.contains(&"fast".to_string()));
            assert!(options.contains(&"slow".to_string()));
        } else {
            panic!("Expected Enum type");
        }
    }

    #[test]
    fn test_skill_parameter_types() {
        let skill = Skill {
            id: "param-types".to_string(),
            name: "Parameter Types".to_string(),
            description: "Test different parameter types".to_string(),
            prompt_template: "Test".to_string(),
            parameters: vec![
                SkillParameter {
                    name: "string_param".to_string(),
                    description: "String".to_string(),
                    param_type: ParameterType::String,
                    required: true,
                    default: None,
                },
                SkillParameter {
                    name: "number_param".to_string(),
                    description: "Number".to_string(),
                    param_type: ParameterType::Number,
                    required: true,
                    default: None,
                },
                SkillParameter {
                    name: "bool_param".to_string(),
                    description: "Boolean".to_string(),
                    param_type: ParameterType::Boolean,
                    required: true,
                    default: None,
                },
            ],
            category: SkillCategory::Custom,
            tags: vec![],
            builtin: false,
        };
        
        assert!(matches!(skill.parameters[0].param_type, ParameterType::String));
        assert!(matches!(skill.parameters[1].param_type, ParameterType::Number));
        assert!(matches!(skill.parameters[2].param_type, ParameterType::Boolean));
    }

    // ========== 重复注册处理测试 ==========

    #[test]
    fn test_register_duplicate_preserves_first() {
        let mut registry = SkillRegistry::new();
        let skill1 = create_test_skill("test-skill", "First Skill", SkillCategory::Custom);
        let skill2 = create_test_skill("test-skill", "Second Skill", SkillCategory::CodeReview);
        
        registry.register(skill1).unwrap();
        let result = registry.register(skill2);
        
        assert!(result.is_err());
        
        // 验证第一个技能仍然保留
        let retrieved = registry.get("test-skill").unwrap();
        assert_eq!(retrieved.name, "First Skill");
        assert_eq!(retrieved.category, SkillCategory::Custom);
    }

    #[test]
    fn test_register_after_unregister() {
        let mut registry = SkillRegistry::new();
        let skill1 = create_test_skill("test-skill", "First", SkillCategory::Custom);
        let skill2 = create_test_skill("test-skill", "Second", SkillCategory::CodeReview);
        
        registry.register(skill1).unwrap();
        registry.unregister("test-skill");
        
        // 现在可以注册同名技能
        registry.register(skill2).unwrap();
        
        let retrieved = registry.get("test-skill").unwrap();
        assert_eq!(retrieved.name, "Second");
        assert_eq!(retrieved.category, SkillCategory::CodeReview);
    }

    // ========== 空注册表查询测试 ==========

    #[test]
    fn test_empty_registry_operations() {
        let registry = SkillRegistry::new();
        
        // 查询空注册表
        assert!(registry.get("any-id").is_none());
        assert!(registry.list().is_empty());
        assert!(registry.search("anything").is_empty());
        assert!(registry.list_by_category(&SkillCategory::Custom).is_empty());
        assert!(registry.categories().is_empty());
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_search_empty_query() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "Skill 1", SkillCategory::Custom)).unwrap();
        registry.register(create_test_skill("s2", "Skill 2", SkillCategory::Custom)).unwrap();
        
        // 空查询应该返回所有技能（因为空字符串匹配所有）
        let results = registry.search("");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_returns_all_skills() {
        let mut registry = SkillRegistry::new();
        registry.register(create_test_skill("s1", "Skill 1", SkillCategory::Custom)).unwrap();
        registry.register(create_test_skill("s2", "Skill 2", SkillCategory::CodeReview)).unwrap();
        registry.register(create_test_skill("s3", "Skill 3", SkillCategory::Refactoring)).unwrap();
        
        let list = registry.list();
        assert_eq!(list.len(), 3);
        
        let ids: std::collections::HashSet<_> = list.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains("s1"));
        assert!(ids.contains("s2"));
        assert!(ids.contains("s3"));
    }
}
