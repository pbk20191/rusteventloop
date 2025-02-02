use std::future::Future;

#[cfg(target_vendor = "apple")]
pub(crate) fn apple_run_loop<F: Future>(future: F) -> F::Output {
    use std::{
        cell::OnceCell,
        future::Future,
        os::raw::c_void,
        // sync::{Arc, Weak},
        rc::{Weak, Rc},
        time::Duration
    };
    use std::ptr::{null, null_mut};
    use compio::driver::AsRawFd;
    use compio::runtime::Runtime;
    use core_foundation::{
        base::TCFType

        ,
        filedescriptor::{kCFFileDescriptorReadCallBack, CFFileDescriptor, CFFileDescriptorContext, CFFileDescriptorRef},
        runloop::{kCFRunLoopAllActivities, kCFRunLoopBeforeWaiting, kCFRunLoopDefaultMode, CFRunLoop,
                  CFRunLoopActivity, CFRunLoopGetCurrent, CFRunLoopObserver, CFRunLoopObserverContext, CFRunLoopObserverCreate,
                  CFRunLoopObserverInvalidate, CFRunLoopObserverRef
                  , CFRunLoopSource, CFRunLoopSourceContext, CFRunLoopSourceCreate, CFRunLoopSourceInvalidate, CFRunLoopSourceSignal,
                  CFRunLoopStop, CFRunLoopTimer, CFRunLoopTimerContext, CFRunLoopTimerInvalidate, CFRunLoopTimerRef, CFRunLoopTimerSetNextFireDate, CFRunLoopWakeUp
        }
        ,
    };

    struct CFContextInfo {
        source: OnceCell<CFRunLoopSource>,
        timer: OnceCell<CFRunLoopTimer>,
        observer: OnceCell<CFRunLoopObserver>,
        runtime: Weak<Runtime>
    }
    
    struct CFRunLoopRuntime {
        runtime: Rc<Runtime>,
        fd_source: CFFileDescriptor,
        context: Rc<CFContextInfo>
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
            &*ptr
        };
        // print!("{}", context_ref.weak_count());
        // let Some(context) = context_ref.upgrade() else { return;};
        let Some(runtime) = context.runtime.upgrade() else { return;};
        runtime.poll_with(Some(Duration::ZERO));
        let remaining_tasks = runtime.run();
        let Some(timeout) = runtime.current_timeout() else { return; };
        unsafe {
            CFRunLoopTimerSetNextFireDate(timer, timeout.as_secs_f64())
        }
    }

    extern "C" fn observe(observer: CFRunLoopObserverRef, activity: CFRunLoopActivity, info: *mut c_void) {
        let context = unsafe {
            let ptr = info as *const CFContextInfo;
            &*ptr
        };
        if activity == kCFRunLoopBeforeWaiting {
            let Some(runtime) = context.runtime.upgrade() else { return;};
            let remaining_tasks = runtime.run();
            let Some(timeout) = runtime.current_timeout() else { return; };
            unsafe {
                CFRunLoopTimerSetNextFireDate(context.timer.get().unwrap().as_concrete_TypeRef(), timeout.as_secs_f64())
            }
        }
    }

    extern "C" fn retain(info: * const c_void) -> * const c_void {

        let ptr = info as * const CFContextInfo;
        unsafe {
            Rc::increment_strong_count(ptr);
        }
        return ptr as *const c_void
    }

    extern "C" fn release(info: * const c_void) {

        let ptr = info as * const CFContextInfo;
        unsafe {
            Rc::decrement_strong_count(ptr);
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
            let wrapper = Rc::new(runtime);
            
           
            let arc_context = Rc::new(CFContextInfo{
                source: OnceCell::new(),
                timer: OnceCell::new(),
                observer: OnceCell::new(),
                runtime: Rc::downgrade(&wrapper),
            });


            let raw_ptr = Rc::into_raw(arc_context);
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
            let reconstructed_context = unsafe { Rc::from_raw(raw_ptr) };
            let _ = reconstructed_context.timer.set(run_loop_timer);
            let _ = reconstructed_context.source.set(source);
            let _ = reconstructed_context.observer.set(observer);
            Self { runtime:wrapper, fd_source, context:reconstructed_context }
        }

        pub fn block_on<F: Future>(&self, future: F) -> F::Output {

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
            self.runtime.enter(|| {
                let mut result = None;
                let run_loop = CFRunLoop::get_current();
                unsafe {
                    self.runtime
                        .spawn_unchecked(async {
                            result = Some(future.await);
                            CFRunLoopSourceSignal(signal.as_concrete_TypeRef());
                            CFRunLoopWakeUp(run_loop.as_concrete_TypeRef());
                        })
                }
                    .detach();
                CFRunLoop::get_current().add_source(&signal, unsafe { kCFRunLoopDefaultMode});
                self.fd_source
                    .enable_callbacks(kCFFileDescriptorReadCallBack);    

                loop {
                    if let Some(result) = result.take() {
                        unsafe {
                            CFRunLoopSourceInvalidate(signal.as_concrete_TypeRef());
                        }
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
            let observer = self.context.observer.get().unwrap().as_concrete_TypeRef();
            unsafe {
                CFRunLoopObserverInvalidate(observer);
                CFRunLoopTimerInvalidate(self.context.timer.get().unwrap().as_concrete_TypeRef());
            }
           
        }
    }
    let runtime = CFRunLoopRuntime::new();

    runtime.block_on(future)
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