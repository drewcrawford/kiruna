use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_long, c_uint, c_void};
use priority::Priority;
use crate::MicroPriority;

#[repr(C)]
struct pthread_attr_t {
    sig: c_long,
    opaque: [c_char;56]
}

#[repr(C)]
struct pthread_t {
    sig: c_long,
    cleanup_stack: *mut c_void,
    opaque: [c_char; 8176]
}

pub enum ThreadPriority {

}

#[repr(C)]
struct QosClassT(c_uint);
#[allow(unused)]
impl QosClassT {
    const USER_INTERACTIVE: QosClassT = QosClassT(0x21);
    const USER_INITIATED: QosClassT = QosClassT(0x19);
    const DEFAULT: QosClassT = QosClassT(0x15);
    const UTILITY: QosClassT = QosClassT(0x11);
    const BACKGROUND: QosClassT = QosClassT(0x09);
    const UNSPECIFIED: QosClassT = QosClassT(0x00);
}
#[repr(C)]
struct RelativePriority(c_int);
#[allow(unused)]
impl RelativePriority {
    ///Priority we use for new threads?
    const NEW_THREADS: RelativePriority = Self(-10);
    const MIN: RelativePriority = Self(-15);
}
extern "C" {
    fn pthread_attr_init(attr: *mut pthread_attr_t) -> c_int;
    fn pthread_attr_destroy(attr: *mut pthread_attr_t) -> c_int;
    fn pthread_create(thread: *mut pthread_t, attr: *const pthread_attr_t, start: extern "C" fn(*const c_void), arg: *const c_void) -> c_int;
    fn pthread_attr_set_qos_class_np(attr: *mut pthread_attr_t, class: QosClassT, relative_priority: RelativePriority) -> c_int;
}
/**
Spawns a thread that executes the closure specified.

This sets the apprpriate priority on the thread prior to launch.
 */
pub fn spawn_thread<F: FnOnce() + Send + 'static>(priority: priority::Priority,micro_priority: MicroPriority,  f: F) {
    let mut attr = MaybeUninit::uninit();
    unsafe {
        let r = pthread_attr_init(attr.assume_init_mut());
        assert!(r==0);

        let qos_class = match priority {
            Priority::UserWaiting =>  QosClassT::USER_INTERACTIVE,
            Priority::Testing => { QosClassT::USER_INTERACTIVE},
            _ => todo!()
        };
        let relative_priority = match micro_priority {
            MicroPriority::NEW => {
                RelativePriority::NEW_THREADS
            }
        };
        let r = pthread_attr_set_qos_class_np(attr.assume_init_mut(), qos_class, relative_priority);
        assert!(r==0);

        extern "C" fn begin<F: FnOnce() + Send + 'static>(arg: *const c_void) {
            unsafe {
                let unboxed = Box::from_raw(arg as *const F as *mut F);
                unboxed()
            }
        }
        let mut pthread = MaybeUninit::uninit();
        let boxed_fn = Box::into_raw(Box::new(f));
        let r = pthread_create(pthread.assume_init_mut(), attr.assume_init_ref(), begin::<F>, boxed_fn as *const c_void);
        assert!(r == 0); //if thread did not start, this probably leaks the box?

        let r = pthread_attr_destroy(attr.assume_init_mut());
        assert!(r==0);
    }

}