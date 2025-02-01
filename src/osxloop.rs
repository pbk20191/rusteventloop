use std::future::Future;

#[cfg(
    all(
        target_vendor = "apple",
        any(target_os = "macos", target_os = "ios")
    )
)]
pub(crate) fn apple_run_loop<F: Future>(future: F) -> F::Output {
    use std::{
        future::Future,
        os::raw::c_void,
        sync::{Arc, Mutex, Weak},
        time::Duration,
        cell::{ Cell, UnsafeCell, OnceCell}
    };
    use std::ptr::{ null_mut, null };
    use block2::{Block, StackBlock};
    use compio::driver::AsRawFd;
    use compio::runtime::{Runtime, event::Event};
    use core_foundation::{
        base::TCFType,
        base::{ CFAllocatorRef, CFIndex, CFOptionFlags, Boolean, CFType, FromVoid,
                CFRetain, CFRelease, ToVoid
        },
        date::{CFAbsoluteTime, CFTimeInterval, CFAbsoluteTimeGetCurrent},
        filedescriptor::{CFFileDescriptor, CFFileDescriptorRef, kCFFileDescriptorReadCallBack, CFFileDescriptorContext, CFFileDescriptorEnableCallBacks},
        runloop::{CFRunLoop, CFRunLoopRef, CFRunLoopStop, kCFRunLoopDefaultMode,CFRunLoopSourceContext,
                  CFRunLoopObserver, CFRunLoopTimerContext, CFRunLoopObserverContext, CFRunLoopTimerInvalidate, CFRunLoopWakeUp,
                  CFRunLoopObserverInvalidate, kCFRunLoopBeforeWaiting, kCFRunLoopAfterWaiting, kCFRunLoopBeforeTimers, kCFRunLoopEntry,
                  CFRunLoopTimerGetContext, CFRunLoopTimerSetNextFireDate,CFRunLoopSource,CFRunLoopSourceSignal, CFRunLoopSourceCreate, CFRunLoopSourceInvalidate,
                  CFRunLoopObserverRef, CFRunLoopTimer, CFRunLoopTimerRef, CFRunLoopActivity,kCFRunLoopAllActivities, CFRunLoopObserverCreate, CFRunLoopGetCurrent
        },
        string::CFStringRef,
    };

    struct CFContextInfo {
        source: OnceCell<CFRunLoopSource>,
        timer: OnceCell<CFRunLoopTimer>,
        observer: OnceCell<CFRunLoopObserver>,
        runtime: Weak<Runtime>,
        dummy_source: CFRunLoopSource,
    }
    
    struct CFRunLoopRuntime {
        runtime: Arc<Runtime>,
        fd_source: CFFileDescriptor,
        context: Arc<CFContextInfo>
    }

    extern "C" fn callback(
        _fdref: CFFileDescriptorRef,
        _callback_types: usize,
        info: *mut c_void,
    ) {
        let context = unsafe {
            let ptr = info as *const CFContextInfo;
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        };
        // let _ = context_ref.clone().into_raw();
        // let Some(context) = context_ref.upgrade() else { return;};
        let Some(runtime) = context.runtime.upgrade() else { return;};
        runtime.poll_with(Some(Duration::ZERO));
        let remaining_tasks = runtime.run();
        let Some(timeout) = runtime.current_timeout() else { return; };
        let Some(timer) = context.timer.get() else { return;};
        unsafe {
            CFRunLoopTimerSetNextFireDate(timer.as_concrete_TypeRef(), timeout.as_secs_f64())
        }

    }

    extern "C" fn timer(timer: CFRunLoopTimerRef, info: *mut c_void) {
        let context = unsafe {
            let ptr = info as *const CFContextInfo;
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        };
        // print!("{}", context_ref.weak_count());
        // let Some(context) = context_ref.upgrade() else { return;};
        let Some(runtime) = context.runtime.upgrade() else { return;};
        runtime.poll_with(Some(Duration::ZERO));
        let remaining_tasks = runtime.run();
        let dummy = context.dummy_source.as_concrete_TypeRef();
        unsafe {
            CFRunLoopSourceSignal(dummy);
        }
        let Some(timeout) = runtime.current_timeout() else { return; };

        unsafe {
            CFRunLoopTimerSetNextFireDate(timer, timeout.as_secs_f64())
        }
    }

    extern "C" fn observe(observer: CFRunLoopObserverRef, activity: CFRunLoopActivity, info: *mut c_void) {
        let context = unsafe {
            let ptr = info as *const CFContextInfo;
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        };
        if activity == kCFRunLoopBeforeWaiting {
            let Some(runtime) = context.runtime.upgrade() else { return;};
            runtime.poll_with(Some(Duration::ZERO));
            let remaining_tasks = runtime.run();
            let dummy = context.dummy_source.as_concrete_TypeRef();
            unsafe {
                CFRunLoopSourceSignal(dummy);
            }
            let Some(timeout) = runtime.current_timeout() else { return; };

            unsafe {
                CFRunLoopTimerSetNextFireDate(context.timer.get().unwrap().as_concrete_TypeRef(), timeout.as_secs_f64())
            }
        }
    }

    extern "C" fn retain(info: * const c_void) -> * const c_void {

        let ptr = info as * const CFContextInfo;
        unsafe {
            Arc::increment_strong_count(ptr);
        }
        return ptr as *const c_void
    }

    extern "C" fn release(info: * const c_void) {

        let ptr = info as * const CFContextInfo;
        unsafe {
            Arc::decrement_strong_count(ptr);
        }
        
    }
    
    extern  "C" fn dummy_perform(info: * const c_void) {
        unsafe {
            CFRunLoopStop(CFRunLoopGetCurrent());
        }
    }
    
    // CFRunLoopTimerRef CFRunLoopTimerCreateWithHandler(CFAllocatorRef allocator, CFAbsoluteTime fireDate, CFTimeInterval interval, CFOptionFlags flags, CFIndex order, void (^block)(CFRunLoopTimerRef timer));
    impl CFRunLoopRuntime {

        pub fn new() -> Self {
            let runtime = Runtime::new().unwrap();
            let wrapper = Arc::new(runtime);
            
           
            let arc_context = Arc::new(CFContextInfo{
                source: OnceCell::new(),
                timer: OnceCell::new(),
                observer: OnceCell::new(),
                runtime: Arc::downgrade(&wrapper),
                dummy_source: unsafe {
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
                }
            });



            let raw_ptr = Arc::into_raw(arc_context);
            let a = CFFileDescriptorContext {
                version: 0,
                info: raw_ptr as * mut c_void,
                retain: Some(retain),
                release: Some(release),
                copyDescription: None,
            };

            let fd_source =
                CFFileDescriptor::new(wrapper.as_raw_fd(), false, callback, Some(&a)).unwrap();

            let source = fd_source.to_run_loop_source(0).unwrap();
            let observer = unsafe {
                let mut context = CFRunLoopObserverContext {
                    version: 0,
                    info: raw_ptr as * mut c_void,
                    retain: Some(retain),
                    release: Some(release),
                    copyDescription: None,
                };

                CFRunLoopObserver::wrap_under_create_rule(
                    CFRunLoopObserverCreate(null(), kCFRunLoopAllActivities, 1, 0, observe, &mut context)
                )
            };
            // CFRunLoopObserverRef::
            let mut timer_context = CFRunLoopTimerContext{
                version: 0,
                info: raw_ptr as * mut c_void,
                retain: Some(retain),
                release: Some(release),
                copyDescription: None,
            };
            let run_loop_timer = CFRunLoopTimer::new(
                Duration::MAX.as_secs_f64(),
                1000.0,
                0, 0,
                timer,
                &mut timer_context,
            );
            
            // let timer = CFRunLoopTimerCreateWithHandler(null(), 0, 1,0,0, &timerBlock);
            let run_loop = CFRunLoop::get_current();
            run_loop.add_source(&source, unsafe { kCFRunLoopDefaultMode });
            run_loop.add_timer(&run_loop_timer, unsafe { kCFRunLoopDefaultMode });
            run_loop.add_observer(&observer, unsafe { kCFRunLoopDefaultMode});
            let reconstructed_context = unsafe { Arc::from_raw(raw_ptr) };
            run_loop.add_source(&reconstructed_context.dummy_source, unsafe { kCFRunLoopDefaultMode });
            let _ = reconstructed_context.timer.set(run_loop_timer);
            let _ = reconstructed_context.source.set(source);
            let _ = reconstructed_context.observer.set(observer);
            Self { runtime:wrapper, fd_source, context:reconstructed_context }
        }

        pub fn block_on<F: Future>(&self, future: F) -> F::Output {
            self.runtime.enter(|| {
                let mut result = None;
                let run_loop = CFRunLoop::get_current();
                let signal = self.context.dummy_source.as_concrete_TypeRef();
                unsafe {
                    self.runtime
                        .spawn_unchecked(async {
                            result = Some(future.await);
                            CFRunLoopSourceSignal(signal);
                            CFRunLoopWakeUp(run_loop.as_concrete_TypeRef());
                            // CFRunLoopStop(run_loop.as_concrete_TypeRef());
                        })
                }
                    .detach();
                self.fd_source
                    .enable_callbacks(kCFFileDescriptorReadCallBack);    

                loop {
                    if let Some(result) = result.take() {
                        break result;
                    }
                    let run_result = CFRunLoop::run_in_mode(
                        unsafe { kCFRunLoopDefaultMode },
                        Duration::MAX,
                        true,
                    );
                    
                }
            })
        }
    }
    
    impl Drop for CFContextInfo {
        fn drop(&mut self) {
            // CFRunLoopRunTime already invalidate CFTypes
            self.observer.take();
            self.timer.take();
            self.source.take();
        }
    }

    impl Drop for CFRunLoopRuntime {
        fn drop(&mut self) {
            // break reference cycle!
            self.fd_source.invalidate();
            let source = self.context.dummy_source.as_concrete_TypeRef();
            let observer = self.context.observer.get().unwrap().as_concrete_TypeRef();
            unsafe {
                if (source != null_mut()) {
                    CFRunLoopSourceInvalidate(source);

                }
                CFRunLoopObserverInvalidate(observer);
                CFRunLoopTimerInvalidate(self.context.timer.get().unwrap().as_concrete_TypeRef());
                // CFRunLoopObserverInvalidate(self.context.source.take().unwrap().as_concrete_TypeRef());
            }
           
        }
    }
    let runtime = CFRunLoopRuntime::new();

    runtime.block_on(future)
}

#[test]
#[cfg(
    all(
        target_vendor = "apple",
        any(target_os = "macos", target_os = "ios")
    )
)]
fn test_loop() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use block2::{Block, StackBlock};
    use compio::runtime::event::Event;
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRef};
    use core_foundation::string::CFStringRef;
    
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