//! 模糊匹配模块
//! 提供模糊字符串匹配功能，支持评分和匹配位置标记

/// 模糊匹配结果
#[derive(Debug, Clone)]
pub struct FuzzyMatch {
    /// 匹配得分（越低越好）
    pub score: i32,
    /// 匹配的字符位置
    pub indices: Vec<usize>,
}

impl FuzzyMatch {
    /// 创建新的匹配结果
    pub fn new(score: i32, indices: Vec<usize>) -> Self {
        Self { score, indices }
    }
}

/// 检查字符是否为单词边界字符
fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || c == '-' || c == '_' || c == '.' || c == '/' || c == ':'
}

/// 对单个字符串进行模糊匹配
/// 
/// # Arguments
/// * `pattern` - 搜索模式
/// * `text` - 要匹配的文本
/// 
/// # Returns
/// 如果匹配成功返回 Some(FuzzyMatch)，否则返回 None
pub fn fuzzy_match(pattern: &str, text: &str) -> Option<FuzzyMatch> {
    if pattern.is_empty() {
        return Some(FuzzyMatch::new(0, Vec::new()));
    }

    if pattern.len() > text.len() {
        return None;
    }

    let pattern_lower = pattern.to_lowercase();
    let text_lower = text.to_lowercase();

    // 尝试主要匹配
    if let Some(result) = match_query(&pattern_lower, &text_lower, text) {
        return Some(result);
    }

    // 尝试字母数字交换匹配（如 "a1" -> "1a"）
    if let Some(swapped) = try_swapped_match(&pattern_lower, &text_lower, text) {
        return Some(swapped);
    }

    None
}

/// 执行实际的匹配逻辑
fn match_query(pattern: &str, text_lower: &str, original_text: &str) -> Option<FuzzyMatch> {
    let mut query_index = 0;
    let mut score: i32 = 0;
    let mut last_match_index: i32 = -1;
    let mut consecutive_matches = 0;
    let mut indices = Vec::new();

    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text_lower.chars().collect();

    for (i, &text_char) in text_chars.iter().enumerate() {
        if query_index >= pattern_chars.len() {
            break;
        }

        if text_char == pattern_chars[query_index] {
            let is_word_boundary = i == 0 || is_word_boundary(text_chars.get(i - 1).copied().unwrap_or(' '));

            // 奖励连续匹配
            if last_match_index == i as i32 - 1 {
                consecutive_matches += 1;
                score -= consecutive_matches * 5;
            } else {
                consecutive_matches = 0;
                // 惩罚间隙
                if last_match_index >= 0 {
                    score += (i as i32 - last_match_index - 1) * 2;
                }
            }

            // 奖励单词边界匹配
            if is_word_boundary {
                score -= 10;
            }

            // 轻微惩罚后面的匹配
            score += i as i32;

            // 记录原始文本中的位置（考虑多字节字符）
            let byte_pos = original_text.chars().take(i).map(|c| c.len_utf8()).sum::<usize>();
            let char_pos = original_text.char_indices()
                .position(|(pos, _)| pos == byte_pos)
                .unwrap_or(i);
            indices.push(char_pos);

            last_match_index = i as i32;
            query_index += 1;
        }
    }

    if query_index < pattern_chars.len() {
        return None;
    }

    Some(FuzzyMatch::new(score, indices))
}

/// 尝试字母数字交换匹配
fn try_swapped_match(pattern: &str, text_lower: &str, original_text: &str) -> Option<FuzzyMatch> {
    // 尝试匹配 "abc123" 或 "123abc" 模式
    let swapped = if let Some(caps) = regex_captures(pattern, r"^([a-z]+)([0-9]+)$") {
        // 字母+数字 -> 数字+字母
        format!("{}{}", &caps[1], &caps[0])
    } else if let Some(caps) = regex_captures(pattern, r"^([0-9]+)([a-z]+)$") {
        // 数字+字母 -> 字母+数字
        format!("{}{}", &caps[1], &caps[0])
    } else {
        return None;
    };

    match_query(&swapped, text_lower, original_text)
        .map(|mut m| {
            m.score += 5; // 交换匹配的惩罚
            m
        })
}

/// 简单的正则捕获（不使用 regex crate 以减少依赖）
fn regex_captures(text: &str, _pattern: &str) -> Option<Vec<String>> {
    // 简化实现：手动解析字母+数字或数字+字母模式
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }

    // 查找字母和数字的分界点
    let mut split_idx = 0;
    let first_is_letter = chars[0].is_ascii_alphabetic();
    
    for (i, &c) in chars.iter().enumerate() {
        let is_letter = c.is_ascii_alphabetic();
        let is_digit = c.is_ascii_digit();
        
        if i > 0 && ((first_is_letter && is_digit) || (!first_is_letter && is_letter)) {
            split_idx = i;
            break;
        }
        
        // 如果类型不一致，记录分界
        if i > 0 {
            let prev_is_letter = chars[i - 1].is_ascii_alphabetic();
            let prev_is_digit = chars[i - 1].is_ascii_digit();
            
            if (prev_is_letter && is_digit) || (prev_is_digit && is_letter) {
                split_idx = i;
                break;
            }
        }
    }

    if split_idx == 0 || split_idx >= chars.len() {
        return None;
    }

    let first: String = chars[..split_idx].iter().collect();
    let second: String = chars[split_idx..].iter().collect();

    Some(vec![first, second])
}

/// 过滤和排序列表
/// 
/// # Arguments
/// * `pattern` - 搜索模式
/// * `items` - 要过滤的条目列表
/// * `get_text` - 获取条目文本的函数
/// 
/// # Returns
/// 返回 (原始索引, 匹配结果) 的向量，按匹配质量排序
pub fn fuzzy_filter<T>(
    pattern: &str,
    items: &[T],
    get_text: impl Fn(&T) -> &str,
) -> Vec<(usize, FuzzyMatch)> {
    if pattern.trim().is_empty() {
        return items.iter().enumerate()
            .map(|(i, _)| (i, FuzzyMatch::new(0, Vec::new())))
            .collect();
    }

    let tokens: Vec<&str> = pattern.split_whitespace().filter(|t| !t.is_empty()).collect();

    if tokens.is_empty() {
        return items.iter().enumerate()
            .map(|(i, _)| (i, FuzzyMatch::new(0, Vec::new())))
            .collect();
    }

    let mut results: Vec<(usize, FuzzyMatch)> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        let text = get_text(item);
        let mut total_score = 0;
        let mut all_match = true;
        let mut all_indices: Vec<usize> = Vec::new();

        for token in &tokens {
            if let Some(m) = fuzzy_match(token, text) {
                total_score += m.score;
                all_indices.extend(m.indices);
            } else {
                all_match = false;
                break;
            }
        }

        if all_match {
            // 去重并排序索引
            all_indices.sort_unstable();
            all_indices.dedup();
            results.push((idx, FuzzyMatch::new(total_score, all_indices)));
        }
    }

    // 按分数排序（越低越好）
    results.sort_by(|a, b| a.1.score.cmp(&b.1.score));

    results
}

/// 简单的模糊过滤，返回匹配项的索引
/// 
/// # Arguments
/// * `pattern` - 搜索模式
/// * `items` - 要过滤的条目列表
/// * `get_text` - 获取条目文本的函数
/// 
/// # Returns
/// 返回匹配项的原始索引列表
pub fn fuzzy_filter_simple<T>(
    pattern: &str,
    items: &[T],
    get_text: impl Fn(&T) -> &str,
) -> Vec<usize> {
    fuzzy_filter(pattern, items, get_text)
        .into_iter()
        .map(|(idx, _)| idx)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_basic() {
        // 基本匹配
        let result = fuzzy_match("abc", "abc");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.indices, vec![0, 1, 2]);

        // 不匹配
        assert!(fuzzy_match("xyz", "abc").is_none());

        // 空模式
        let result = fuzzy_match("", "abc");
        assert!(result.is_some());
        assert_eq!(result.unwrap().score, 0);
    }

    #[test]
    fn test_fuzzy_match_subsequence() {
        // 子序列匹配
        let result = fuzzy_match("abc", "aabbcc");
        assert!(result.is_some());
        
        let result = fuzzy_match("abc", "axbxcx");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        let result = fuzzy_match("ABC", "abc");
        assert!(result.is_some());

        let result = fuzzy_match("abc", "ABC");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_scoring() {
        // 连续匹配应该得分更好（更低）
        let consecutive = fuzzy_match("abc", "abc").unwrap();
        let scattered = fuzzy_match("abc", "axbxcx").unwrap();
        
        // 连续匹配的分数应该更低（更好）
        assert!(consecutive.score < scattered.score);
    }

    #[test]
    fn test_fuzzy_match_word_boundary() {
        // 单词边界匹配应该得分更好
        let boundary = fuzzy_match("t", "test").unwrap();
        let middle = fuzzy_match("e", "test").unwrap();
        
        // 边界匹配的分数应该更低（更好）
        assert!(boundary.score < middle.score);
    }

    #[test]
    fn test_fuzzy_filter() {
        let items = vec!["apple", "banana", "cherry", "date"];
        
        let results = fuzzy_filter("a", &items, |&x| x);
        assert!(!results.is_empty());
        
        // 应该包含 apple, banana, date
        let indices: Vec<usize> = results.iter().map(|(i, _)| *i).collect();
        assert!(indices.contains(&0)); // apple
        assert!(indices.contains(&1)); // banana
        assert!(indices.contains(&3)); // date
    }

    #[test]
    fn test_fuzzy_filter_multi_token() {
        let items = vec!["foo bar", "foo baz", "hello world"];
        
        let results = fuzzy_filter("foo bar", &items, |&x| x);
        assert!(!results.is_empty());
        
        // "foo bar" 应该排在最前面
        assert_eq!(results[0].0, 0);
    }

    #[test]
    fn test_fuzzy_filter_empty_pattern() {
        let items = vec!["a", "b", "c"];
        
        let results = fuzzy_filter("", &items, |&x| x);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fuzzy_match_unicode() {
        let result = fuzzy_match("中文", "中文测试");
        assert!(result.is_some());
        
        let result = fuzzy_match("测试", "中文测试");
        assert!(result.is_some());
    }

    // === 边界条件测试 ===

    #[test]
    fn test_fuzzy_match_pattern_longer_than_text() {
        // 模式比文本长应该返回 None
        assert!(fuzzy_match("abcdef", "abc").is_none());
    }

    #[test]
    fn test_fuzzy_match_empty_text() {
        // 空文本
        assert!(fuzzy_match("abc", "").is_none());
        
        // 空模式和空文本
        let result = fuzzy_match("", "");
        assert!(result.is_some());
        assert_eq!(result.unwrap().score, 0);
    }

    #[test]
    fn test_fuzzy_match_single_char() {
        // 单字符匹配
        let result = fuzzy_match("a", "abc").unwrap();
        assert_eq!(result.indices, vec![0]);
        
        // 单字符不匹配
        assert!(fuzzy_match("x", "abc").is_none());
    }

    #[test]
    fn test_fuzzy_match_special_characters() {
        // 特殊字符匹配
        let result = fuzzy_match("a-b", "a-b-c");
        assert!(result.is_some());
        
        let result = fuzzy_match("a_b", "a_b_c");
        assert!(result.is_some());
        
        let result = fuzzy_match("a.b", "a.b.c");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_numbers() {
        // 数字匹配
        let result = fuzzy_match("123", "abc123def");
        assert!(result.is_some());
        
        let result = fuzzy_match("abc123", "abc123def");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_whitespace() {
        // 空白字符匹配
        let result = fuzzy_match("foo bar", "foo bar baz");
        assert!(result.is_some());
        
        let result = fuzzy_match("  ", "foo  bar");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_long_pattern() {
        // 长模式
        let text = "the quick brown fox jumps over the lazy dog";
        let result = fuzzy_match("quick brown fox", text);
        assert!(result.is_some());
        
        let result = fuzzy_match("the quick brown fox jumps over the lazy dog", text);
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_repeated_chars() {
        // 重复字符
        let result = fuzzy_match("aa", "aaaa").unwrap();
        assert_eq!(result.indices, vec![0, 1]);
        
        let result = fuzzy_match("abc", "aabbcc").unwrap();
        assert_eq!(result.indices, vec![0, 2, 4]);
    }

    #[test]
    fn test_fuzzy_filter_empty_items() {
        let items: Vec<&str> = vec![];
        let results = fuzzy_filter("test", &items, |&x| x);
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_filter_no_match() {
        let items = vec!["apple", "banana", "cherry"];
        let results = fuzzy_filter("xyz", &items, |&x| x);
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_filter_all_match() {
        let items = vec!["test", "testing", "tester"];
        let results = fuzzy_filter("test", &items, |&x| x);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fuzzy_filter_whitespace_only_pattern() {
        let items = vec!["apple", "banana", "cherry"];
        let results = fuzzy_filter("   ", &items, |&x| x);
        // 空白字符模式应该返回所有项
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fuzzy_filter_simple() {
        let items = vec!["apple", "banana", "cherry"];
        let results = fuzzy_filter_simple("app", &items, |&x| x);
        assert!(!results.is_empty());
        assert!(results.contains(&0)); // apple
    }

    #[test]
    fn test_fuzzy_match_score_ordering() {
        // 测试分数排序
        let items = vec!["zzzzzz", "abc", "abcxyz"];
        let results = fuzzy_filter("abc", &items, |&x| x);
        
        // 验证结果按分数排序（越低越好）
        for i in 1..results.len() {
            assert!(results[i].1.score >= results[i - 1].1.score);
        }
        
        // "abc" 应该排在最前面（完全匹配，分数最低）
        assert_eq!(results[0].0, 1);
    }

    #[test]
    fn test_fuzzy_match_indices_order() {
        let result = fuzzy_match("abc", "axbxcx").unwrap();
        // 索引应该是递增的
        for i in 1..result.indices.len() {
            assert!(result.indices[i] > result.indices[i - 1]);
        }
    }

    #[test]
    fn test_is_word_boundary() {
        assert!(is_word_boundary(' '));
        assert!(is_word_boundary('-'));
        assert!(is_word_boundary('_'));
        assert!(is_word_boundary('.'));
        assert!(is_word_boundary('/'));
        assert!(is_word_boundary(':'));
        assert!(!is_word_boundary('a'));
        assert!(!is_word_boundary('1'));
    }

    #[test]
    fn test_fuzzy_match_new() {
        let m = FuzzyMatch::new(100, vec![0, 1, 2]);
        assert_eq!(m.score, 100);
        assert_eq!(m.indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_fuzzy_match_clone() {
        let m = FuzzyMatch::new(100, vec![0, 1, 2]);
        let cloned = m.clone();
        assert_eq!(m.score, cloned.score);
        assert_eq!(m.indices, cloned.indices);
    }

    #[test]
    fn test_fuzzy_match_debug() {
        let m = FuzzyMatch::new(100, vec![0, 1, 2]);
        let debug_str = format!("{:?}", m);
        assert!(debug_str.contains("FuzzyMatch"));
        assert!(debug_str.contains("100"));
    }

    #[test]
    fn test_fuzzy_filter_sorted_results() {
        let items = vec!["zzzzabc", "abc", "aabbcc"];
        let results = fuzzy_filter("abc", &items, |&x| x);
        
        // 结果应该按分数排序（越低越好）
        for i in 1..results.len() {
            assert!(results[i].1.score >= results[i - 1].1.score);
        }
    }

    #[test]
    fn test_fuzzy_filter_simple_empty() {
        let items: Vec<&str> = vec![];
        let results = fuzzy_filter_simple("test", &items, |&x| x);
        assert!(results.is_empty());
    }
}
