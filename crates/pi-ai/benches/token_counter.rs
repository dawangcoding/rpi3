//! Token 计数器性能基准测试
//!
//! 测试各种 token 计数器的性能特征

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use pi_ai::token_counter::{
    create_token_counter, EstimateTokenCounter, ModelTokenCounter,
    TokenCounter, TiktokenCounter, GeminiTokenCounter, MistralTokenCounter,
};
use pi_ai::types::{Message, UserMessage};

/// 生成指定长度的英文文本
fn generate_english_text(size: usize) -> String {
    let words = [
        "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
        "hello", "world", "rust", "code", "test", "benchmark", "performance",
    ];
    let mut text = String::with_capacity(size);
    let mut rng = 0usize;
    while text.len() < size {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(words[rng % words.len()]);
        rng += 1;
    }
    text
}

/// 生成指定长度的中文文本
fn generate_chinese_text(size: usize) -> String {
    let chars = ['你', '好', '世', '界', '测', '试', '中', '文', '本', '件',
                 '编', '程', '序', '代', '码', '性', '能', '基', '准', '检'];
    let mut text = String::with_capacity(size * 3);
    let mut rng = 0usize;
    while text.chars().count() < size {
        text.push(chars[rng % chars.len()]);
        rng += 1;
    }
    text
}

/// 生成代码文本
fn generate_code_text(size: usize) -> String {
    let lines = [
        "fn main() {",
        "    let x = 42;",
        "    println!(\"Hello, world!\");",
        "    for i in 0..10 {",
        "        println!(\"i = {}\", i);",
        "    }",
        "}",
        "",
        "struct Point { x: i32, y: i32 }",
        "",
        "impl Point {",
        "    fn new(x: i32, y: i32) -> Self { Self { x, y } }",
        "    fn distance(&self) -> f64 { ((self.x.pow(2) + self.y.pow(2)) as f64).sqrt() }",
        "}",
        "",
        "fn process<T: Clone>(data: Vec<T>) -> Vec<T> {",
        "    data.iter().cloned().collect()",
        "}",
    ];
    let mut code = String::with_capacity(size);
    let mut line_idx = 0;
    while code.len() < size {
        if line_idx > 0 {
            code.push('\n');
        }
        code.push_str(lines[line_idx % lines.len()]);
        line_idx += 1;
    }
    code
}

/// 创建测试消息列表
fn create_test_messages(count: usize, text_per_msg: usize) -> Vec<Message> {
    (0..count)
        .map(|i| {
            let text = format!("Message {}: {}", i, generate_english_text(text_per_msg));
            Message::User(UserMessage::new(text))
        })
        .collect()
}

/// 测试 EstimateTokenCounter 的基本性能
fn bench_estimate_counter(c: &mut Criterion) {
    let counter = EstimateTokenCounter::new();
    
    // 短文本
    let short_text = "Hello, world!";
    c.bench_function("estimate/short_text", |b| {
        b.iter(|| counter.count_text(black_box(short_text)))
    });

    // 不同长度的文本
    for size in [100, 1_000, 10_000, 100_000].iter() {
        let text = generate_english_text(*size);
        c.bench_with_input(
            BenchmarkId::new("estimate/english", size),
            &text,
            |b, text| b.iter(|| counter.count_text(black_box(text))),
        );
    }

    // 中文文本
    for size in [100, 1_000, 10_000].iter() {
        let text = generate_chinese_text(*size);
        c.bench_with_input(
            BenchmarkId::new("estimate/chinese", size),
            &text,
            |b, text| b.iter(|| counter.count_text(black_box(text))),
        );
    }

    // 代码文本
    for size in [100, 1_000, 10_000].iter() {
        let code = generate_code_text(*size);
        c.bench_with_input(
            BenchmarkId::new("estimate/code", size),
            &code,
            |b, code| b.iter(|| counter.count_text(black_box(code))),
        );
    }
}

/// 测试 ModelTokenCounter 的性能
fn bench_model_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("model_counter");
    
    let claude_counter = ModelTokenCounter::new("claude");
    let gpt_counter = ModelTokenCounter::new("gpt");
    let gemini_counter = ModelTokenCounter::new("gemini");
    let mistral_counter = ModelTokenCounter::new("mistral");

    let text = generate_english_text(10_000);

    group.bench_function("claude", |b| {
        b.iter(|| claude_counter.count_text(black_box(&text)))
    });
    
    group.bench_function("gpt", |b| {
        b.iter(|| gpt_counter.count_text(black_box(&text)))
    });
    
    group.bench_function("gemini", |b| {
        b.iter(|| gemini_counter.count_text(black_box(&text)))
    });
    
    group.bench_function("mistral", |b| {
        b.iter(|| mistral_counter.count_text(black_box(&text)))
    });
    
    group.finish();
}

/// 测试 TiktokenCounter 的性能
fn bench_tiktoken_counter(c: &mut Criterion) {
    let counter = TiktokenCounter::new("gpt-4o").expect("Failed to create TiktokenCounter");
    
    let mut group = c.benchmark_group("tiktoken");
    
    // 短文本
    let short_text = "Hello, world! This is a test.";
    group.bench_function("short_text", |b| {
        b.iter(|| counter.count_text(black_box(short_text)))
    });

    // 不同长度
    for size in [100, 1_000, 10_000, 100_000].iter() {
        let text = generate_english_text(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("english", size),
            &text,
            |b, text| b.iter(|| counter.count_text(black_box(text))),
        );
    }

    // 中文
    for size in [100, 1_000, 10_000].iter() {
        let text = generate_chinese_text(*size);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("chinese", size),
            &text,
            |b, text| b.iter(|| counter.count_text(black_box(text))),
        );
    }
    
    group.finish();
}

/// 测试 Gemini Token Counter
fn bench_gemini_counter(c: &mut Criterion) {
    let counter = GeminiTokenCounter::new().expect("Failed to create GeminiTokenCounter");
    
    let mut group = c.benchmark_group("gemini_counter");
    
    for size in [100, 1_000, 10_000].iter() {
        let english = generate_english_text(*size);
        let chinese = generate_chinese_text(*size);
        
        group.bench_with_input(
            BenchmarkId::new("english", size),
            &english,
            |b, text| b.iter(|| counter.count_text(black_box(text))),
        );
        
        group.bench_with_input(
            BenchmarkId::new("chinese", size),
            &chinese,
            |b, text| b.iter(|| counter.count_text(black_box(text))),
        );
    }
    
    group.finish();
}

/// 测试 Mistral Token Counter
fn bench_mistral_counter(c: &mut Criterion) {
    // Mistral counter may fail if tokenizer not available
    if let Some(counter) = MistralTokenCounter::new() {
        let mut group = c.benchmark_group("mistral_counter");
        
        for size in [100, 1_000, 10_000].iter() {
            let text = generate_english_text(*size);
            group.bench_with_input(
                BenchmarkId::new("english", size),
                &text,
                |b, text| b.iter(|| counter.count_text(black_box(text))),
            );
        }
        
        group.finish();
    }
}

/// 测试消息计数性能
fn bench_message_counting(c: &mut Criterion) {
    let counter = create_token_counter("gpt-4o");
    
    let mut group = c.benchmark_group("message_counting");
    
    // 单条消息
    let single_msg = Message::User(UserMessage::new(generate_english_text(1_000)));
    group.bench_function("single_message", |b| {
        b.iter(|| counter.count_message(black_box(&single_msg)))
    });

    // 多条消息
    for count in [10, 50, 100].iter() {
        let messages = create_test_messages(*count, 500);
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::new("messages", count),
            &messages,
            |b, msgs| b.iter(|| counter.count_messages(black_box(msgs))),
        );
    }
    
    group.finish();
}

/// 测试工厂函数 create_token_counter 的性能
fn bench_factory(c: &mut Criterion) {
    let mut group = c.benchmark_group("factory");
    
    group.bench_function("create/gpt-4o", |b| {
        b.iter(|| create_token_counter(black_box("gpt-4o")))
    });
    
    group.bench_function("create/claude", |b| {
        b.iter(|| create_token_counter(black_box("claude-sonnet-4")))
    });
    
    group.bench_function("create/gemini", |b| {
        b.iter(|| create_token_counter(black_box("gemini-pro")))
    });
    
    group.bench_function("create/mistral", |b| {
        b.iter(|| create_token_counter(black_box("mistral-small")))
    });
    
    group.bench_function("create/unknown", |b| {
        b.iter(|| create_token_counter(black_box("unknown-model")))
    });
    
    group.finish();
}

/// 测试带内容块的消息计数
fn bench_content_blocks(c: &mut Criterion) {
    let counter = create_token_counter("gpt-4o");
    
    // 创建带复杂内容块的消息
    let complex_msg = Message::User(UserMessage::new(generate_english_text(500)));
    
    c.bench_function("complex_message", |b| {
        b.iter(|| counter.count_message(black_box(&complex_msg)))
    });
}

criterion_group!(
    benches,
    bench_estimate_counter,
    bench_model_counter,
    bench_tiktoken_counter,
    bench_gemini_counter,
    bench_mistral_counter,
    bench_message_counting,
    bench_factory,
    bench_content_blocks,
);

criterion_main!(benches);
