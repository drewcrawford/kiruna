use criterion::{black_box, criterion_group, criterion_main, Criterion};
use kiruna::test::test_await;
use kiruna_tpc::set::vec::{set_sync, Strategy};
fn build_arr(arr_len: usize, jobs: u32) -> Vec<usize> {
    let big_fut = set_sync(priority::Priority::Testing, arr_len,Strategy::Jobs(jobs), |idx| {
        let mut val = idx;
        for i in 0..100_000 {
            val ^= idx ^ i;
        }
        val
    });
    let my_vec = test_await(big_fut, std::time::Duration::new(10, 0));
    my_vec
}

fn criterion_benchmark(c: &mut Criterion) {
    let arr_len = 5_000;
    c.bench_function("vec 2000 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 2000))));
    c.bench_function("vec 1600 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 1600))));
    c.bench_function("vec 1400 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 1400))));
    c.bench_function("vec 1200 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 1200))));
    c.bench_function("vec 1000 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 1000))));
    c.bench_function("vec 800 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 800))));
    c.bench_function("vec 400 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 400))));
    c.bench_function("vec 200 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 200))));
    c.bench_function("vec 100 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 100))));
    c.bench_function("vec 50 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 50))));
    c.bench_function("vec 10 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 10))));
    c.bench_function("vec 5 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 5))));
    c.bench_function("vec 1 jobs", |b| b.iter(|| black_box(build_arr(arr_len, 1))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);