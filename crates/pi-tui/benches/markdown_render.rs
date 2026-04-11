//! Markdown 渲染性能基准测试
//!
//! 测试 Markdown 组件的渲染性能

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use pi_tui::{Component, components::markdown::Markdown};

/// 生成纯文本 Markdown
fn generate_plain_text(size: usize) -> String {
    let words = [
        "Lorem", "ipsum", "dolor", "sit", "amet", "consectetur", 
        "adipiscing", "elit", "sed", "do", "eiusmod", "tempor",
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

/// 生成代码块 Markdown
fn generate_code_blocks(count: usize) -> String {
    let languages = ["rust", "python", "javascript", "go", "typescript"];
    let mut md = String::new();
    
    for i in 0..count {
        let lang = languages[i % languages.len()];
        md.push_str(&format!("```{}\n", lang));
        md.push_str(&format!("// Code block {}\n", i));
        md.push_str("fn example() {\n");
        md.push_str("    let x = 42;\n");
        md.push_str("    println!(\"Hello, world!\");\n");
        md.push_str("    for i in 0..10 {\n");
        md.push_str("        println!(\"i = {}\", i);\n");
        md.push_str("    }\n");
        md.push_str("}\n");
        md.push_str("```\n\n");
    }
    md
}

/// 生成复杂 Markdown（表格+列表+代码）
fn generate_complex_markdown(sections: usize) -> String {
    let mut md = String::new();
    
    for i in 0..sections {
        // 标题
        md.push_str(&format!("# Section {}\n\n", i + 1));
        
        // 粗体和斜体
        md.push_str(&format!("This is **bold** and *italic* text in section {}.\n\n", i + 1));
        
        // 列表
        md.push_str("## Features\n\n");
        for j in 0..5 {
            md.push_str(&format!("- Feature {} with `code` inline\n", j + 1));
        }
        md.push('\n');
        
        // 嵌套列表
        md.push_str("### Nested Items\n\n");
        for j in 0..3 {
            md.push_str(&format!("- Item {}\n", j + 1));
            for k in 0..2 {
                md.push_str(&format!("  - Subitem {}.{}\n", j + 1, k + 1));
            }
        }
        md.push('\n');
        
        // 表格
        md.push_str("| Name | Value | Description |\n");
        md.push_str("|------|-------|-------------|\n");
        for j in 0..5 {
            md.push_str(&format!("| Item {} | {} | Description for item {} |\n", j + 1, j * 10, j + 1));
        }
        md.push('\n');
        
        // 代码块
        md.push_str("```rust\n");
        md.push_str(&format!("// Example code for section {}\n", i + 1));
        md.push_str("fn main() {\n");
        md.push_str("    println!(\"Hello, world!\");\n");
        md.push_str("}\n");
        md.push_str("```\n\n");
        
        // 引用
        md.push_str("> This is a blockquote in section ");
        md.push_str(&format!("{}\n\n", i + 1));
        
        // 链接
        md.push_str(&format!("[Link to section {}](#section-{})\n\n", i + 1, i + 1));
        
        // 水平线
        md.push_str("---\n\n");
    }
    
    md
}

/// 生成大文档 Markdown
fn generate_large_document(size_kb: usize) -> String {
    let mut md = String::new();
    let target_size = size_kb * 1024;
    let mut section = 0;
    
    while md.len() < target_size {
        md.push_str(&format!("## Section {}\n\n", section));
        
        // 添加段落
        for _ in 0..3 {
            md.push_str(&generate_plain_text(200));
            md.push_str("\n\n");
        }
        
        // 添加代码块
        md.push_str("```\n");
        md.push_str(&generate_plain_text(100));
        md.push_str("\n```\n\n");
        
        section += 1;
    }
    
    md
}

/// 测试纯文本渲染
fn bench_plain_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("plain_text");
    
    for size in [100, 1_000, 10_000, 100_000].iter() {
        let text = generate_plain_text(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("render", size),
            &text,
            |b, text| {
                let mut md = Markdown::new();
                md.set_content(text);
                b.iter(|| md.render(black_box(80)))
            },
        );
    }
    
    group.finish();
}

/// 测试代码块渲染
fn bench_code_blocks(c: &mut Criterion) {
    let mut group = c.benchmark_group("code_blocks");
    
    for count in [1, 5, 10, 20].iter() {
        let md_content = generate_code_blocks(*count);
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::new("blocks", count),
            &md_content,
            |b, content| {
                let mut md = Markdown::new();
                md.set_content(content);
                b.iter(|| md.render(black_box(80)))
            },
        );
    }
    
    group.finish();
}

/// 测试复杂 Markdown 渲染
fn bench_complex_markdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_markdown");
    
    for sections in [1, 3, 5, 10].iter() {
        let md_content = generate_complex_markdown(*sections);
        group.throughput(Throughput::Elements(*sections as u64));
        group.bench_with_input(
            BenchmarkId::new("sections", sections),
            &md_content,
            |b, content| {
                let mut md = Markdown::new();
                md.set_content(content);
                b.iter(|| md.render(black_box(80)))
            },
        );
    }
    
    group.finish();
}

/// 测试大文档渲染
fn bench_large_document(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_document");
    
    for size_kb in [10, 50, 100].iter() {
        let md_content = generate_large_document(*size_kb);
        group.throughput(Throughput::Bytes(md_content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("kb", size_kb),
            &md_content,
            |b, content| {
                let mut md = Markdown::new();
                md.set_content(content);
                b.iter(|| md.render(black_box(80)))
            },
        );
    }
    
    group.finish();
}

/// 测试缓存渲染效果
fn bench_caching(c: &mut Criterion) {
    let mut group = c.benchmark_group("caching");
    
    let md_content = generate_complex_markdown(5);
    let mut md = Markdown::new();
    md.set_content(&md_content);
    
    // 首次渲染（无缓存）
    group.bench_function("first_render", |b| {
        b.iter(|| {
            let mut md = Markdown::new();
            md.set_content(&md_content);
            md.render(black_box(80))
        })
    });
    
    // 带缓存渲染
    group.bench_function("cached_render", |b| {
        b.iter(|| md.render_with_cache(black_box(80)))
    });
    
    // 相同内容重复渲染（无缓存优化）
    group.bench_function("repeat_render", |b| {
        b.iter(|| md.render(black_box(80)))
    });
    
    group.finish();
}

/// 测试不同渲染宽度
fn bench_render_widths(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_widths");
    
    let md_content = generate_complex_markdown(3);
    
    for width in [40, 80, 120, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("width", width),
            width,
            |b, w| {
                let mut md = Markdown::new();
                md.set_content(&md_content);
                b.iter(|| md.render(black_box(*w)))
            },
        );
    }
    
    group.finish();
}

/// 测试文本换行性能
fn bench_text_wrapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_wrapping");
    
    let long_text = generate_plain_text(10_000);
    
    // 无换行
    group.bench_function("no_wrap", |b| {
        let mut md = Markdown::new();
        md.set_content(&long_text);
        md.set_wrap_width(None);
        b.iter(|| md.render(black_box(80)))
    });
    
    // 窄宽度换行
    for wrap_width in [40, 80, 120].iter() {
        group.bench_with_input(
            BenchmarkId::new("wrap", wrap_width),
            wrap_width,
            |b, w| {
                let mut md = Markdown::new();
                md.set_content(&long_text);
                md.set_wrap_width(Some(*w));
                b.iter(|| md.render(black_box(80)))
            },
        );
    }
    
    group.finish();
}

/// 测试流式内容追加
fn bench_append_content(c: &mut Criterion) {
    let mut group = c.benchmark_group("append_content");
    
    // 模拟流式输入
    group.bench_function("stream_100_chunks", |b| {
        b.iter(|| {
            let mut md = Markdown::new();
            for i in 0..100 {
                let chunk = format!("Line {} of the document. ", i);
                md.append_content(&chunk);
            }
            md.render(black_box(80))
        })
    });
    
    // 大块追加
    group.bench_function("large_chunks", |b| {
        b.iter(|| {
            let mut md = Markdown::new();
            for _ in 0..10 {
                let chunk = generate_plain_text(500);
                md.append_content(&chunk);
            }
            md.render(black_box(80))
        })
    });
    
    group.finish();
}

/// 测试不同 Markdown 元素
fn bench_markdown_elements(c: &mut Criterion) {
    let mut group = c.benchmark_group("elements");
    
    // 标题
    let headings = (1..=6)
        .map(|i| format!("{} Heading {}\n\n", "#".repeat(i), i))
        .collect::<String>();
    group.bench_function("headings", |b| {
        let mut md = Markdown::new();
        md.set_content(&headings);
        b.iter(|| md.render(black_box(80)))
    });
    
    // 列表
    let list = (0..100)
        .map(|i| format!("- Item {} with some longer text to make it realistic\n", i))
        .collect::<String>();
    group.bench_function("list", |b| {
        let mut md = Markdown::new();
        md.set_content(&list);
        b.iter(|| md.render(black_box(80)))
    });
    
    // 引用
    let quotes = (0..20)
        .map(|i| format!("> Quote {} with some text inside\n", i))
        .collect::<String>();
    group.bench_function("quotes", |b| {
        let mut md = Markdown::new();
        md.set_content(&quotes);
        b.iter(|| md.render(black_box(80)))
    });
    
    // 链接
    let links = (0..50)
        .map(|i| format!("[Link {}](https://example.com/link/{})\n", i, i))
        .collect::<String>();
    group.bench_function("links", |b| {
        let mut md = Markdown::new();
        md.set_content(&links);
        b.iter(|| md.render(black_box(80)))
    });
    
    // 内联代码
    let inline_code = (0..50)
        .map(|i| format!("Text with `code_{}` inline\n", i))
        .collect::<String>();
    group.bench_function("inline_code", |b| {
        let mut md = Markdown::new();
        md.set_content(&inline_code);
        b.iter(|| md.render(black_box(80)))
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_plain_text,
    bench_code_blocks,
    bench_complex_markdown,
    bench_large_document,
    bench_caching,
    bench_render_widths,
    bench_text_wrapping,
    bench_append_content,
    bench_markdown_elements,
);

criterion_main!(benches);
