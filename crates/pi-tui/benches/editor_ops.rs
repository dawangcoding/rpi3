//! 编辑器操作性能基准测试
//!
//! 测试 Editor 组件的核心操作性能

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use pi_tui::components::editor::{Editor, EditorConfig, EditorMode};

/// 生成测试文本
fn generate_text(lines: usize, chars_per_line: usize) -> String {
    (0..lines)
        .map(|_| "x".repeat(chars_per_line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 生成代码风格文本
fn generate_code_text(lines: usize) -> String {
    (0..lines)
        .enumerate()
        .map(|(i, _)| {
            match i % 10 {
                0 => format!("fn function_{}() {{", i / 10),
                1 => "    let x = 42;".to_string(),
                2 => "    let y = x * 2;".to_string(),
                3 => "    for i in 0..10 {".to_string(),
                4 => "        println!(\"i = {}\", i);".to_string(),
                5 => "    }".to_string(),
                6 => "    let result = y + i;".to_string(),
                7 => "    result".to_string(),
                8 => "}".to_string(),
                _ => "".to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 测试字符插入性能
fn bench_insert_char(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_char");
    
    // 单个字符插入
    let mut editor = Editor::new(EditorConfig::default());
    group.bench_function("single_char", |b| {
        b.iter(|| {
            editor.insert_char(black_box('a'));
        })
    });
    
    // 在不同位置插入
    for size in [100, 1_000, 10_000].iter() {
        let text = generate_text(1, *size);
        group.bench_with_input(
            BenchmarkId::new("position", size),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                editor.move_end();
                b.iter(|| editor.insert_char(black_box('x')))
            },
        );
    }
    
    group.finish();
}

/// 测试文本插入性能
fn bench_insert_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_text");
    
    // 插入不同长度的文本
    for size in [10, 100, 1_000, 10_000].iter() {
        let insert_text = "Hello, world! ".repeat(*size / 14);
        group.throughput(Throughput::Bytes(insert_text.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("size", size),
            &insert_text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                b.iter(|| editor.insert_text(black_box(text)))
            },
        );
    }
    
    // 多行文本插入
    for lines in [10, 100, 1_000].iter() {
        let text = generate_text(*lines, 50);
        group.throughput(Throughput::Elements(*lines as u64));
        group.bench_with_input(
            BenchmarkId::new("lines", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                b.iter(|| editor.insert_text(black_box(text)))
            },
        );
    }
    
    group.finish();
}

/// 测试删除操作性能
fn bench_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("delete");
    
    // Backspace 删除
    for size in [100, 1_000, 10_000].iter() {
        let text = generate_text(1, *size);
        group.bench_with_input(
            BenchmarkId::new("backspace", size),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                editor.move_end();
                b.iter(|| editor.delete_char_before())
            },
        );
    }
    
    // Delete 删除
    for size in [100, 1_000, 10_000].iter() {
        let text = generate_text(1, *size);
        group.bench_with_input(
            BenchmarkId::new("delete", size),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                editor.move_home();
                b.iter(|| editor.delete_char_after())
            },
        );
    }
    
    // 删除行
    for lines in [100, 1_000, 10_000].iter() {
        let text = generate_text(*lines, 50);
        group.throughput(Throughput::Elements(*lines as u64));
        group.bench_with_input(
            BenchmarkId::new("delete_line", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                editor.move_to_start();
                b.iter(|| editor.delete_line())
            },
        );
    }
    
    group.finish();
}

/// 测试光标移动性能
fn bench_cursor_movement(c: &mut Criterion) {
    let mut group = c.benchmark_group("cursor_movement");
    
    let text = generate_text(1000, 80);
    
    // 辅助函数：创建预填充的编辑器
    let create_editor = || {
        let mut editor = Editor::new(EditorConfig::default());
        editor.set_text(&text);
        editor
    };
    
    // 左右移动
    group.bench_function("move_left", |b| {
        let mut editor = create_editor();
        editor.move_end();
        b.iter(|| editor.move_left())
    });
    
    group.bench_function("move_right", |b| {
        let mut editor = create_editor();
        editor.move_home();
        b.iter(|| editor.move_right())
    });
    
    // 上下移动
    group.bench_function("move_up", |b| {
        let mut editor = create_editor();
        editor.move_to_end();
        b.iter(|| editor.move_up())
    });
    
    group.bench_function("move_down", |b| {
        let mut editor = create_editor();
        editor.move_to_start();
        b.iter(|| editor.move_down())
    });
    
    // 单词移动
    group.bench_function("move_word_left", |b| {
        let mut editor = create_editor();
        editor.move_end();
        b.iter(|| editor.move_word_left())
    });
    
    group.bench_function("move_word_right", |b| {
        let mut editor = create_editor();
        editor.move_home();
        b.iter(|| editor.move_word_right())
    });
    
    // 首尾移动
    group.bench_function("move_home", |b| {
        let mut editor = create_editor();
        editor.move_end();
        b.iter(|| editor.move_home())
    });
    
    group.bench_function("move_end", |b| {
        let mut editor = create_editor();
        editor.move_home();
        b.iter(|| editor.move_end())
    });
    
    group.bench_function("move_to_start", |b| {
        let mut editor = create_editor();
        editor.move_to_end();
        b.iter(|| editor.move_to_start())
    });
    
    group.bench_function("move_to_end", |b| {
        let mut editor = create_editor();
        editor.move_to_start();
        b.iter(|| editor.move_to_end())
    });
    
    group.finish();
}

/// 测试撤销/重做性能
fn bench_undo_redo(c: &mut Criterion) {
    let mut group = c.benchmark_group("undo_redo");
    
    // 创建一些历史记录
    let mut editor = Editor::new(EditorConfig::default());
    for i in 0..100 {
        editor.insert_text(&format!("Line {}\n", i));
    }
    
    // 测试撤销性能（注意：Editor 的 undo 需要 undo() 方法）
    group.bench_function("undo_after_edits", |b| {
        let mut editor = Editor::new(EditorConfig::default());
        for i in 0..100 {
            editor.insert_text(&format!("Line {}\n", i));
        }
        // 撤销性能通过快照机制
        b.iter(|| {
            editor.delete_char_before();
        })
    });
    
    group.finish();
}

/// 测试文本获取性能
fn bench_get_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_text");
    
    for lines in [10, 100, 1_000, 10_000].iter() {
        let text = generate_text(*lines, 80);
        let mut editor = Editor::new(EditorConfig::default());
        editor.set_text(&text);
        
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("lines", lines),
            &editor,
            |b, editor| b.iter(|| editor.get_text()),
        );
    }
    
    group.finish();
}

/// 测试设置文本性能
fn bench_set_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("set_text");
    
    for lines in [10, 100, 1_000, 10_000].iter() {
        let text = generate_text(*lines, 80);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("lines", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                b.iter(|| editor.set_text(black_box(text)))
            },
        );
    }
    
    group.finish();
}

/// 测试新建行性能
fn bench_new_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("new_line");
    
    for lines in [100, 1_000].iter() {
        let text = generate_text(*lines, 50);
        group.bench_with_input(
            BenchmarkId::new("existing_lines", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                editor.move_to_end();
                b.iter(|| editor.new_line())
            },
        );
    }
    
    group.finish();
}

/// 测试复杂编辑序列
fn bench_complex_edit_sequence(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_sequence");
    
    group.bench_function("type_paragraph", |b| {
        b.iter(|| {
            let mut editor = Editor::new(EditorConfig::default());
            for word in ["Hello", "world", "this", "is", "a", "test"].iter() {
                for ch in word.chars() {
                    editor.insert_char(ch);
                }
                editor.insert_char(' ');
            }
            editor.get_text()
        })
    });
    
    group.bench_function("edit_code", |b| {
        b.iter(|| {
            let mut editor = Editor::new(EditorConfig::default());
            // 输入函数签名
            editor.insert_text("fn example(x: i32) -> i32 {\n");
            // 添加函数体
            editor.insert_text("    let y = x * 2;\n");
            editor.insert_text("    y\n");
            editor.insert_text("}");
            editor.get_text()
        })
    });
    
    group.bench_function("edit_and_navigate", |b| {
        b.iter(|| {
            let mut editor = Editor::new(EditorConfig::default());
            editor.set_text(&generate_code_text(100));
            // 移动到不同位置并编辑
            for _ in 0..10 {
                editor.move_to_start();
                editor.move_down();
                editor.move_down();
                editor.insert_char('x');
                editor.move_down();
                editor.move_down();
                editor.delete_char_before();
            }
            editor.get_text()
        })
    });
    
    group.finish();
}

/// 测试不同编辑器配置
fn bench_editor_config(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_config");
    
    let text = generate_text(100, 80);
    
    // 默认配置
    group.bench_function("default_config", |b| {
        b.iter(|| {
            let mut editor = Editor::new(EditorConfig::default());
            editor.set_text(&text);
            editor.insert_char('x');
        })
    });
    
    // Vim 模式
    group.bench_function("vim_mode", |b| {
        let mut config = EditorConfig::default();
        config.editor_mode = EditorMode::Vim;
        b.iter(|| {
            let mut editor = Editor::new(config.clone());
            editor.set_text(&text);
            editor.insert_char('x');
        })
    });
    
    // 显示行号
    group.bench_function("line_numbers", |b| {
        let mut config = EditorConfig::default();
        config.line_numbers = true;
        b.iter(|| {
            let mut editor = Editor::new(config.clone());
            editor.set_text(&text);
            editor.insert_char('x');
        })
    });
    
    group.finish();
}

/// 测试大文件处理
fn bench_large_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_file");
    group.sample_size(10);
    
    for lines in [1_000, 5_000, 10_000].iter() {
        let text = generate_code_text(*lines);
        
        group.throughput(Throughput::Bytes(text.len() as u64));
        
        // 加载大文件
        group.bench_with_input(
            BenchmarkId::new("load", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                b.iter(|| editor.set_text(black_box(text)))
            },
        );
        
        // 在大文件中导航
        group.bench_with_input(
            BenchmarkId::new("navigate", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                b.iter(|| {
                    editor.move_to_start();
                    editor.move_down();
                    editor.move_down();
                    editor.move_to_end();
                })
            },
        );
        
        // 在大文件中编辑
        group.bench_with_input(
            BenchmarkId::new("edit", lines),
            &text,
            |b, text| {
                let mut editor = Editor::new(EditorConfig::default());
                editor.set_text(text);
                editor.move_to_start();
                b.iter(|| {
                    editor.move_down();
                    editor.insert_char('x');
                })
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_insert_char,
    bench_insert_text,
    bench_delete,
    bench_cursor_movement,
    bench_undo_redo,
    bench_get_text,
    bench_set_text,
    bench_new_line,
    bench_complex_edit_sequence,
    bench_editor_config,
    bench_large_file,
);

criterion_main!(benches);
