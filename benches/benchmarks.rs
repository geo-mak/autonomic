use autonomic::core::serde::AnySerializable;
use autonomic::testkit::params::TestRetry;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_any_serializable_serialize(c: &mut Criterion) {
    let value = TestRetry::new(5, 100);
    let any_serializable = AnySerializable::new_register(value);
    c.bench_function("any_serializable_serialize", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(black_box(&any_serializable)).unwrap();
            black_box(serialized);
        })
    });
}

fn benchmark_any_serializable_deserialize(c: &mut Criterion) {
    let value = TestRetry::new(5, 100);
    let any_serializable = AnySerializable::new_register(value);
    let serialized = serde_json::to_string(&any_serializable).unwrap();
    c.bench_function("any_serializable_deserialize", |b| {
        b.iter(|| {
            let deserialized: AnySerializable =
                serde_json::from_str(black_box(&serialized)).unwrap();
            black_box(deserialized);
        })
    });
}

fn benchmark_any_serializable_downcast_single(c: &mut Criterion) {
    let value = TestRetry::new(5, 100);
    let any_serializable = AnySerializable::new_register(value);
    c.bench_function("any_serializable_serde_downcast", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(black_box(&any_serializable)).unwrap();
            let deserialized: AnySerializable =
                serde_json::from_str(black_box(&serialized)).unwrap();
            let concrete_type: &TestRetry = deserialized.downcast_ref::<TestRetry>().unwrap();
            black_box(concrete_type);
        })
    });
}

fn benchmark_any_serializable_downcast_many(c: &mut Criterion) {
    let value = TestRetry::new(5, 100);
    let any_serializable = AnySerializable::new_register(value);
    c.bench_function("any_serializable_serde_downcast_10000", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                let serialized = serde_json::to_string(black_box(&any_serializable)).unwrap();
                let deserialized: AnySerializable =
                    serde_json::from_str(black_box(&serialized)).unwrap();
                let concrete_type: &TestRetry = deserialized.downcast_ref::<TestRetry>().unwrap();
                black_box(concrete_type);
            }
        })
    });
}

criterion_group!(
    serde_any_serializable,
    benchmark_any_serializable_serialize,
    benchmark_any_serializable_deserialize,
    benchmark_any_serializable_downcast_single,
    benchmark_any_serializable_downcast_many
);

criterion_main!(serde_any_serializable);
