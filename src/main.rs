use std::{iter, sync::Barrier, time};

use crossbeam_utils::{thread::scope, CachePadded};

fn main() {
    let mut args = std::env::args()
        .skip(1)
        .map(|it| it.parse::<u32>().unwrap());

    let options = Options {
        n_readers: args.next().unwrap(),
        n_reads: args.next().unwrap(),
        n_writers: args.next().unwrap(),
        n_writes: args.next().unwrap(),
        n_locks: args.next().unwrap(),
        n_rounds: args.next().unwrap(),
    };
    println!("{:#?}\n", options);

    bench::<rwlocks::Std>(&options);
    bench::<rwlocks::ParkingLot>(&options);
    bench::<rwlocks::Spin>(&options);

    println!();
    bench::<rwlocks::Std>(&options);
    bench::<rwlocks::ParkingLot>(&options);
    bench::<rwlocks::Spin>(&options);
}

fn bench<M: RwLock>(options: &Options) {
    let mut times = (0..options.n_rounds)
        .map(|_| run_bench::<M>(options))
        .collect::<Vec<_>>();
    times.sort();

    let avg = times.iter().sum::<time::Duration>() / options.n_rounds;
    let min = times[0];
    let max = *times.last().unwrap();

    let avg = format!("{:?}", avg);
    let min = format!("{:?}", min);
    let max = format!("{:?}", max);

    println!(
        "{:<20} avg {:<12} min {:<12} max {:<12}",
        M::LABEL,
        avg,
        min,
        max
    )
}

#[derive(Debug)]
struct Options {
    n_readers: u32,
    n_reads: u32,
    n_writers: u32,
    n_writes: u32,
    n_locks: u32,
    n_rounds: u32,
}

fn random_numbers(seed: u32) -> impl Iterator<Item = u32> {
    let mut random = seed;
    iter::repeat_with(move || {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        random
    })
}

trait RwLock: Sync + Send + Default {
    const LABEL: &'static str;
    fn read(&self) -> u32;
    fn write(&self, f: impl FnOnce(&mut u32));
}

fn run_bench<M: RwLock>(options: &Options) -> time::Duration {
    let locks = &(0..options.n_locks)
        .map(|_| CachePadded::new(M::default()))
        .collect::<Vec<_>>();

    let start_barrier = &Barrier::new(options.n_readers as usize + options.n_writers as usize+ 1);
    let end_barrier = &Barrier::new(options.n_readers as usize + options.n_writers as usize+ 1);

    let elapsed = scope(|scope| {
        let reader_thread_seeds = random_numbers(0x6F4A955E).scan(0x9BA2BF27, |state, n| {
            *state ^= n;
            Some(*state)
        });
        let writer_thread_seeds = random_numbers(0x6F4A955E).scan(0x9BA2BF27, |state, n| {
            *state ^= n;
            Some(*state)
        });

        for thread_seed in reader_thread_seeds.take(options.n_readers as usize) {
            scope.spawn(move |_| {
                start_barrier.wait();
                let indexes = random_numbers(thread_seed)
                    .map(|it| it % options.n_locks)
                    .map(|it| it as usize)
                    .take(options.n_reads as usize);
                
                for idx in indexes {
                    locks[idx].read();
                }
                end_barrier.wait();
            });
        }

        for thread_seed in writer_thread_seeds.take(options.n_writers as usize) {
            scope.spawn(move |_| {
                start_barrier.wait();
                let indexes = random_numbers(thread_seed)
                    .map(|it| it % options.n_locks)
                    .map(|it| it as usize)
                    .take(options.n_writes as usize);
                
                for idx in indexes {
                    locks[idx].write(|cnt| *cnt += 1);
                }
                end_barrier.wait();
            });
        }

        std::thread::sleep(time::Duration::from_millis(100));
        start_barrier.wait();
        let start = time::Instant::now();
        end_barrier.wait();
        let elapsed = start.elapsed();

        let mut total = 0;
        for lock in locks.iter() {
            total += lock.read();
        }
        assert_eq!(total, options.n_writers * options.n_writes);

        elapsed
    })
    .unwrap();
    elapsed
}

mod rwlocks {
    use super::RwLock;

    pub(crate) type Std = std::sync::RwLock<u32>;
    impl RwLock for Std {
        const LABEL: &'static str = "std::sync::RwLock";
        fn read(&self) -> u32 {
            let guard = self.read().unwrap();
            *guard
        }
        fn write(&self, f: impl FnOnce(&mut u32)) {
            let mut guard = self.write().unwrap();
            f(&mut guard)
        }
    }

    pub(crate) type ParkingLot = parking_lot::RwLock<u32>;
    impl RwLock for ParkingLot {
        const LABEL: &'static str = "parking_lot::RwLock";
        fn read(&self) -> u32 {
            let guard = self.read();
            *guard
        }
        fn write(&self, f: impl FnOnce(&mut u32)) {
            let mut guard = self.write();
            f(&mut guard)
        }
    }

    pub(crate) type Spin = spin::RwLock<u32>;
    impl RwLock for Spin {
        const LABEL: &'static str = "spin::RwLock";
        fn read(&self) -> u32 {
            let guard = self.read();
            *guard
        }
        fn write(&self, f: impl FnOnce(&mut u32)) {
            let mut guard = self.write();
            f(&mut guard)
        }
    }

}
