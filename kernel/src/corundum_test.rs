use corundum::stm::{Chaperon, Journal};
use corundum::{open_flags, MemPool, MemPoolTraits, PCell, Pbox, RootObj};
use kernel::println;

mod ffi {
    use core::ffi::{c_char, CStr};
    use core::ptr;

    #[no_mangle]
    extern "C" fn getenv(name: *const c_char) -> *const c_char {
        let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
            return ptr::null();
        };

        (match name {
            "CPUS" => "1\0".as_ptr(),
            //"VERBOSE" => "1\0".as_ptr(),
            //"RECOVERY_INFO" => "3\0".as_ptr(),
            "VERIFY" => "2\0".as_ptr(),
            _ => ptr::null(),
        }) as *const c_char
    }
}

corundum::pool!(pool1);
corundum::pool!(pool2);

type P1 = pool1::Allocator;
type P2 = pool2::Allocator;

struct Root<M: MemPool> {
    val: Pbox<PCell<i32, M>, M>,
}

impl<M: MemPool> RootObj<M> for Root<M> {
    fn init(j: &Journal<M>) -> Self {
        Root {
            val: Pbox::new(PCell::new(0), j),
        }
    }
}

pub fn corundum_test() {
    let root1 = P1::open::<Root<P1>>("pool1.pool", open_flags::O_CF).unwrap();
    let root2 = P2::open::<Root<P2>>("pool2.pool", open_flags::O_CF).unwrap();

    let v1 = root1.val.get();
    let v2 = root2.val.get();
    println!("root1 = {}", v1);
    println!("root2 = {}", v2);

    Chaperon::session("chaperon.pool", || {
        let v = P2::transaction(|j| {
            let old = root2.val.get();
            root2.val.set(old + 1, j);
            old
        })
        .unwrap();
        P1::transaction(|j| {
            let p1 = root1.val.get();
            root1.val.set(p1 + v, j);
        })
        .unwrap();
    })
    .unwrap();

    let v1 = root1.val.get();
    let v2 = root2.val.get();
    println!("root1 = {}", v1);
    println!("root2 = {}", v2);
    assert_eq!(v1, calc(v2 - 1));

    fn calc(n: i32) -> i32 {
        if n < 1 {
            0
        } else {
            n + calc(n - 1)
        }
    }
}
