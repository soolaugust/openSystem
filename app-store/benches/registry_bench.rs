use app_store::registry::{AppEntry, AppRegistry};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

fn make_registry() -> (AppRegistry, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("bench.db");
    let registry = AppRegistry::new(&db, dir.path()).unwrap();
    (registry, dir)
}

fn insert_entries(registry: &AppRegistry, n: usize) {
    for i in 0..n {
        let entry = AppEntry {
            id: format!("app-{}", i),
            name: format!("App {}", i),
            version: "1.0.0".to_string(),
            description: format!("Description for app number {}", i),
            permissions: vec!["net".to_string()],
            public_key: "ed25519_key".to_string(),
            created_at: i as i64,
            osp_path: format!("/apps/app-{}.osp", i),
            ui_spec: None,
        };
        registry.insert(&entry).unwrap();
    }
}

fn bench_search_empty(c: &mut Criterion) {
    let (registry, _dir) = make_registry();
    insert_entries(&registry, 100);
    c.bench_function("registry_search_miss", |b| {
        b.iter(|| registry.search(black_box("zzznomatch")).unwrap())
    });
}

fn bench_search_hit(c: &mut Criterion) {
    let (registry, _dir) = make_registry();
    insert_entries(&registry, 100);
    c.bench_function("registry_search_hit", |b| {
        b.iter(|| registry.search(black_box("App 5")).unwrap())
    });
}

fn bench_list_all(c: &mut Criterion) {
    let (registry, _dir) = make_registry();
    insert_entries(&registry, 100);
    c.bench_function("registry_list_all_100", |b| {
        b.iter(|| registry.list_all().unwrap())
    });
}

fn bench_insert(c: &mut Criterion) {
    c.bench_function("registry_insert", |b| {
        let (registry, _dir) = make_registry();
        let mut i = 0u64;
        b.iter(|| {
            let entry = AppEntry {
                id: format!("bench-{}", i),
                name: "bench app".to_string(),
                version: "1.0.0".to_string(),
                description: "bench".to_string(),
                permissions: vec![],
                public_key: "key".to_string(),
                created_at: i as i64,
                osp_path: "/tmp/bench.osp".to_string(),
                ui_spec: None,
            };
            registry.insert(black_box(&entry)).unwrap();
            i += 1;
        });
    });
}

criterion_group!(
    benches,
    bench_search_empty,
    bench_search_hit,
    bench_list_all,
    bench_insert
);
criterion_main!(benches);
