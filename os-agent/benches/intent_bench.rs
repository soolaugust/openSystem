use criterion::{black_box, criterion_group, criterion_main, Criterion};
use os_agent::utils::extract_json;

fn bench_extract_json_plain(c: &mut Criterion) {
    let input = r#"{"action": "run_app", "app": "calculator"}"#;
    c.bench_function("extract_json_plain_object", |b| {
        b.iter(|| extract_json(black_box(input)))
    });
}

fn bench_extract_json_code_block(c: &mut Criterion) {
    let input = "```json\n{\"action\": \"install_app\", \"query\": \"calculator\"}\n```";
    c.bench_function("extract_json_code_block", |b| {
        b.iter(|| extract_json(black_box(input)))
    });
}

fn bench_extract_json_nested(c: &mut Criterion) {
    let input =
        r#"{"action": "run_app", "params": {"name": "calc", "args": {"mode": "scientific"}}}"#;
    c.bench_function("extract_json_nested_object", |b| {
        b.iter(|| extract_json(black_box(input)))
    });
}

fn bench_extract_json_no_match(c: &mut Criterion) {
    let input = "Sorry, I don't understand what you mean. Can you clarify?";
    c.bench_function("extract_json_no_match", |b| {
        b.iter(|| extract_json(black_box(input)))
    });
}

fn bench_appspec_parse(c: &mut Criterion) {
    let json = r#"{
        "name": "calculator",
        "description": "A simple calculator app",
        "permissions": ["storage"],
        "icon": "🧮"
    }"#;
    c.bench_function("appspec_json_parse", |b| {
        b.iter(|| {
            let _spec: os_agent::app_generator::AppSpec =
                serde_json::from_str(black_box(json)).unwrap();
        })
    });
}

criterion_group!(
    benches,
    bench_extract_json_plain,
    bench_extract_json_code_block,
    bench_extract_json_nested,
    bench_extract_json_no_match,
    bench_appspec_parse
);
criterion_main!(benches);
