use core::arch::x86_64::{__cpuid_count as cpuid, _mm_lfence as lfence, _rdtsc as rdtsc};
use core::ffi::CStr;
use corundum::stl::HashMap;
use corundum::stm::Journal;
use corundum::{open_flags, MemPool, MemPoolTraits, PRefCell, RootObj};
use kernel::println;
use log::trace;

pub fn measure<F: Fn() -> R, R>(f: F, warmup: usize, iterations: usize) -> f64 {
    let mut t;

    (0..warmup).for_each(|_| {
        f();
    });

    unsafe {
        lfence();
        t = rdtsc();
        lfence();
    };

    (0..iterations).for_each(|_| {
        f();
    });

    unsafe {
        lfence();
        t = rdtsc() - t;
        lfence();
    }

    t as f64 / iterations as f64
}

pub fn tsc_khz() -> Option<f64> {
    let mut brand = [0_u8; 48 + 1];
    for (leaf, offset) in (0x80000002..=0x80000004).zip((0..).step_by(4 * 4)) {
        let res = unsafe { cpuid(leaf, 0) };
        [res.eax, res.ebx, res.ecx, res.edx]
            .into_iter()
            .zip((offset..).step_by(4).map(|i| i..(i + 4)))
            .for_each(|(v, r)| brand[r].copy_from_slice(&v.to_le_bytes()));
    }

    let brand = CStr::from_bytes_until_nul(&brand)
        .unwrap()
        .to_str()
        .unwrap();
    // e.g. Intel(R) Core(TM) i7-8665U CPU @ 1.90GHz
    trace!("Processor brand string: {}", brand);

    let end = brand.rfind("GHz")?;
    let start = brand[..end].rfind(|c: char| !c.is_ascii_digit() && c != '.')? + 1;
    let freq = brand[start..end].parse::<f64>().ok()? * 1e9;
    trace!("Processor frequency: {}", freq);

    Some(freq)
}

corundum::pool!(pool1);

type P1 = pool1::Allocator;

struct BenchRoot<M: MemPool> {
    m: PRefCell<HashMap<usize, usize, M>, M>,
}

impl<M: MemPool> RootObj<M> for BenchRoot<M> {
    fn init(j: &Journal<M>) -> Self {
        BenchRoot {
            m: PRefCell::new(corundum::stl::HashMap::new(j)),
        }
    }
}

pub fn corundum_bench() {
    let root1 = P1::open::<BenchRoot<P1>>("bench.pool", open_flags::O_CF).unwrap();
    let _ = tsc_khz().unwrap_or(1.0);

    const WARMUP: usize = 100;
    const ITERATIONS: usize = 1000000;

    P1::transaction(|j| {
        let mut m = root1.m.borrow_mut(j);
        for i in 0..10 {
            m.put(10 - i, i, j);
        }
    })
    .unwrap();

    root1.m.borrow().foreach(|k, v| {
        println!("m[{}] = {}", k, v);
    });

    println!(
        "{}",
        measure(
            || {
                let m = root1.m.borrow();
                *m.get(5).unwrap()
            },
            WARMUP,
            ITERATIONS,
        )
    );

    println!(
        "{}",
        measure(
            || {
                P1::transaction(|_| 0).unwrap();
            },
            WARMUP,
            ITERATIONS,
        )
    );

    println!(
        "{}",
        measure(
            || {
                P1::transaction(|j| {
                    let mut m = root1.m.borrow_mut(j);
                    m.put(5, 500, j);
                })
                .unwrap();
            },
            WARMUP,
            ITERATIONS,
        )
    );

    root1.m.borrow().foreach(|k, v| {
        println!("m[{}] = {}", *k, *v);
    });
}
