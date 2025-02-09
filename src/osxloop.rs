use std::future::Future;

#[cfg(target_vendor = "apple")]
pub(crate) fn apple_run_loop<F: Future>(future: F) -> F::Output {
    use std::{
        mem::transmute,
        os::raw::c_void,
        ptr::{
            null, null_mut
        },
        rc::{Rc, Weak},
        time::Duration,
    };
    use compio::driver::AsRawFd;
    use compio::runtime::Runtime;
    use block2::{
        Block, StackBlock
    };
    use core_foundation::{
        base::{
            Boolean, CFAllocatorRef, CFIndex, CFOptionFlags, CFTypeRef, TCFType,
        },
        date::{CFAbsoluteTime, CFAbsoluteTimeGetCurrent, CFTimeInterval},
        filedescriptor::{kCFFileDescriptorReadCallBack, CFFileDescriptor, CFFileDescriptorContext, CFFileDescriptorInvalidate, CFFileDescriptorRef},
        runloop::{kCFRunLoopAllActivities, kCFRunLoopBeforeWaiting, kCFRunLoopDefaultMode, CFRunLoop,
                  CFRunLoopActivity, CFRunLoopGetCurrent, CFRunLoopObserver, CFRunLoopObserverInvalidate,
                  CFRunLoopObserverRef, kCFRunLoopCommonModes
                  , CFRunLoopRef, CFRunLoopSource, CFRunLoopSourceContext, CFRunLoopSourceCreate, CFRunLoopSourceInvalidate,
                  CFRunLoopSourceSignal, CFRunLoopStop, CFRunLoopTimer, CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
                  CFRunLoopTimerSetNextFireDate, CFRunLoopWakeUp
        },
        string::{
            CFString, CFStringRef
        },
    };
    extern "C" {

        fn CFRunLoopPerformBlock(rl: CFRunLoopRef, mode: CFStringRef, block: &Block<dyn Fn()>);

        fn CFRunLoopTimerCreateWithHandler(
            allocator:CFAllocatorRef,
            fire_date:CFAbsoluteTime,
            interval:CFTimeInterval,
            flags: CFOptionFlags,
            order:CFIndex,
            block: &Block<dyn Fn(CFTypeRef)>
        ) -> CFRunLoopTimerRef;

        fn CFRunLoopObserverCreateWithHandler(
            allocator:CFAllocatorRef,
            activities:CFRunLoopActivity,
            repeats: Boolean,
            order:CFIndex,
            block: &Block<dyn Fn(CFTypeRef, CFRunLoopActivity)>
        ) -> CFRunLoopObserverRef;
    }


    struct CFContextInfo {
        timer: Weak<CFRunLoopTimer>,
        runtime: Weak<Runtime>,
    }

    extern "C" fn callback(
        _fdref: CFFileDescriptorRef,
        _callback_types: usize,
        info: *mut c_void,
    ) {
        let context = unsafe {
            let ptr = info as *const CFContextInfo;
            &*ptr
        };
        // let _ = context_ref.clone().into_raw();
        // let Some(context) = context_ref.upgrade() else { return;};
        let Some(runtime) = context.runtime.upgrade() else {
            unsafe {
                CFFileDescriptorInvalidate(_fdref)
            };
            return;
        };
        runtime.poll_with(Some(Duration::ZERO));
        while runtime.run() {

        }
        let Some(timeout) = runtime.current_timeout() else { return; };
        let Some(timer) = context.timer.upgrade() else {
            unsafe {
                CFFileDescriptorInvalidate(_fdref)
            };
            return;
        };
        unsafe {
            CFRunLoopTimerSetNextFireDate(timer.as_concrete_TypeRef(), timeout.as_secs_f64())
        }

    }

    extern "C" fn retain(info: * const c_void) -> * const c_void {

        let ptr = info as * const CFContextInfo;
        unsafe {
            Rc::increment_strong_count(ptr);
        }
        ptr as *const c_void
    }

    extern "C" fn release(info: * const c_void) {

        let ptr = info as * const CFContextInfo;
        unsafe {
            Rc::decrement_strong_count(ptr);
        }

    }


    extern  "C" fn dummy_perform(_info: * const c_void) {
        unsafe {
            CFRunLoopStop(CFRunLoopGetCurrent());
        }
    }


    let runtime = Rc::new(Runtime::new().unwrap());

    let _timer_runtime = Rc::downgrade(&runtime);
    let _timer_block = StackBlock::new(move |timer_ref:CFTypeRef| {
        let timer = unsafe {
            CFRunLoopTimer::wrap_under_get_rule(transmute(timer_ref))
        };
        let Some(runtime) = _timer_runtime.upgrade() else {
            unsafe {
                CFRunLoopTimerInvalidate(timer.as_concrete_TypeRef())
            }
            return
        };
        runtime.poll_with(Some(Duration::ZERO));
        while runtime.run() { }
        match runtime.current_timeout() {
            Some(timeout) => {
                unsafe {
                    CFRunLoopTimerSetNextFireDate(timer.as_concrete_TypeRef(), timeout.as_secs_f64())
                }
            }
            None => {

            }
        }
    });
    let timer = unsafe {
        let timer = CFRunLoopTimer::wrap_under_create_rule(
            CFRunLoopTimerCreateWithHandler(null(), CFAbsoluteTimeGetCurrent() + 100000.0, 1000.0, 1, 0, &_timer_block)
        );
        Rc::new(timer)
    };
    let _observer_runtime = Rc::downgrade(&runtime);
    let _observer_timer = Rc::downgrade(&timer);
    let observer_block = StackBlock::new(move |observer_ref:CFTypeRef, event:CFRunLoopActivity| {
        let observer = unsafe {
            CFRunLoopObserver::wrap_under_get_rule(transmute(observer_ref))
        };
        let Some(runtime) = _observer_runtime.upgrade() else {
            unsafe {
                CFRunLoopObserverInvalidate(observer.as_concrete_TypeRef())
            }
            return
        };
        // if event != kCFRunLoopBeforeWaiting {
        //     return;
        // }
        runtime.run();
        match runtime.current_timeout() {
            Some(timeout) => {
                if let Some(timer) = _observer_timer.upgrade() {
                    unsafe {
                        CFRunLoopTimerSetNextFireDate(timer.as_concrete_TypeRef(), timeout.as_secs_f64())
                    }
                }
            }
            None => {

            }
        }
    });
    let observer = unsafe {
        CFRunLoopObserver::wrap_under_create_rule(
            CFRunLoopObserverCreateWithHandler(null(), kCFRunLoopBeforeWaiting, 1, 0, &observer_block)
        )
    };
    let context = Rc::new(
        CFContextInfo {
            timer: Rc::downgrade(&timer),
            runtime: Rc::downgrade(&runtime)
        }
    );
    let cf_fd:CFFileDescriptor =  {
        let raw_ptr = Rc::into_raw(context);
        let context = CFFileDescriptorContext {
            version: 0,
            info: raw_ptr as * mut c_void,
            retain: Some(retain),
            release: Some(release),
            copyDescription: None,
        };
        let fd_source =
            CFFileDescriptor::new(runtime.as_raw_fd(), false, callback, Some(&context)).unwrap();
        fd_source
    };
    let source = cf_fd.to_run_loop_source(0).unwrap();
    let run_loop = CFRunLoop::get_current();

    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    run_loop.add_timer(&timer, unsafe { kCFRunLoopCommonModes });
    run_loop.add_observer(&observer, unsafe { kCFRunLoopCommonModes});
    let runtime_ref = &runtime;

    #[cfg(target_os = "macos") ]
    {
        let modal = "NSModalPanelRunLoopMode";
        let modal_string  = CFString::from(modal);
        run_loop.add_source(&source, modal_string.as_concrete_TypeRef());
        run_loop.add_timer(&timer, modal_string.as_concrete_TypeRef());
        run_loop.add_observer(&observer, modal_string.as_concrete_TypeRef());
    }

    
    runtime.enter(move || {
        struct ContextResource {
            timer: Rc<CFRunLoopTimer>,
            observer:CFRunLoopObserver,
            source:CFRunLoopSource,
            cf_fd:CFFileDescriptor,
        }
        impl Drop for ContextResource {
            fn drop(&mut self) {
                unsafe {
                    CFRunLoopTimerInvalidate(self.timer.as_concrete_TypeRef());
                    CFRunLoopObserverInvalidate(self.observer.as_concrete_TypeRef());
                    CFRunLoopSourceInvalidate(self.source.as_concrete_TypeRef());
                    CFFileDescriptorInvalidate(self.cf_fd.as_concrete_TypeRef());
                }
            }
        }
        cf_fd
            .enable_callbacks(kCFFileDescriptorReadCallBack);
        let _context = ContextResource {
            timer,
            observer,
            source,
            cf_fd
        };
        let signal =  unsafe {
            let mut context = CFRunLoopSourceContext{
                version: 0,
                info: null_mut(),
                retain: None,
                release: None,
                copyDescription: None,
                equal: None,
                hash: None,
                schedule: None,
                cancel: None,
                perform: dummy_perform,
            };
            CFRunLoopSource::wrap_under_create_rule(
                CFRunLoopSourceCreate(null(), 0, &mut context)
            )
        };
        let mut result = None;
        let run_loop = CFRunLoop::get_current();
        unsafe {
            runtime_ref
                .spawn_unchecked(async {
                    result = Some(future.await);
                    CFRunLoopSourceSignal(signal.as_concrete_TypeRef());
                    CFRunLoopWakeUp(run_loop.as_concrete_TypeRef());
                })
        }
            .detach();
        CFRunLoop::get_current().add_source(&signal, unsafe { kCFRunLoopDefaultMode});

        let output = loop {
            if let Some(result) = result.take() {
                unsafe {
                    CFRunLoopSourceInvalidate(signal.as_concrete_TypeRef());
                }

                break result;
            }
            CFRunLoop::run_in_mode(
                unsafe { kCFRunLoopDefaultMode },
                Duration::MAX,
                true,
            );

        };
        drop(_context);
        output
    })
}

#[test]
#[cfg(target_vendor = "apple")]
fn test_loop() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use block2::{Block, StackBlock};
    use compio::runtime::event::Event;
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRef};
    use core_foundation::string::CFStringRef;
    use core_foundation::base::TCFType;

    apple_run_loop(async {
        compio::runtime::time::sleep(Duration::from_secs(1)).await;

        let event = Event::new();
        let handle = Arc::new(Mutex::new(Some(event.handle())));
        let run_loop = CFRunLoop::get_current();
        let block = StackBlock::new(move || {
            handle.lock().unwrap().take().unwrap().notify();

        });
        extern "C" {
            fn CFRunLoopPerformBlock(rl: CFRunLoopRef, mode: CFStringRef, block: &Block<dyn Fn()>);
        }
        let run_loop = CFRunLoop::get_current();
        unsafe {
            CFRunLoopPerformBlock(
                run_loop.as_concrete_TypeRef(),
                kCFRunLoopDefaultMode,
                &block,
            );
        }
        event.wait().await;
    })
}


