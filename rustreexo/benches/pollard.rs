use criterion::criterion_group;
use criterion::criterion_main;
use criterion::Criterion;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use rustreexo::accumulator::util::hash_from_u8;

pub fn bench_pollard(c: &mut Criterion) {
    c.bench_function("pollard serialization", |b| {
        b.iter(|| {
            let mut p = Pollard::new();
            let values: Vec<u8> = (0..1_000_000).map(|i| i as u8).collect();
            let hashes: Vec<BitcoinNodeHash> = values.into_iter().map(hash_from_u8).collect();
            p.modify(&hashes, &Vec::new()).unwrap();
            let cloned_p = p.clone();
            let _ = bincode::serialize(&cloned_p).unwrap();
            p.restore_used_flag();
            let stripped = p.get_stripped_pollard();
            let _ = bincode::serialize(&stripped).unwrap();
        })
    });
}

criterion_group!(benches, bench_pollard);
criterion_main!(benches);
