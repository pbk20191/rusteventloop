



#[cfg(
    all(
        target_vendor = "apple",
        any(target_os = "macos", target_os = "ios")
    )
)]
pub(crate) fn apple_run_loop() {
    use std::{
        future::Future,
        os::raw::c_void,
        sync::{Arc, Mutex, Weak},
        time::Duration,
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
        runloop::{CFRunLoop, CFRunLoopRef, CFRunLoopStop, kCFRunLoopDefaultMode,
                  CFRunLoopObserver, CFRunLoopTimerContext, CFRunLoopObserverContext, CFRunLoopTimerInvalidate,
                  CFRunLoopObserverInvalidate, kCFRunLoopBeforeWaiting, kCFRunLoopAfterWaiting, kCFRunLoopBeforeTimers, kCFRunLoopEntry,
                  CFRunLoopTimerGetContext, CFRunLoopTimerSetNextFireDate,
                  CFRunLoopObserverRef, CFRunLoopTimer, CFRunLoopTimerRef, CFRunLoopActivity,kCFRunLoopAllActivities, CFRunLoopObserverCreate
        },
        string::CFStringRef,
    };

    struct CFRunLoopRuntime {
        runtime: Arc<Runtime>,
        fd_source: CFFileDescriptor,
        observer: CFRunLoopObserver,
        timer: CFRunLoopTimer,
    }

    // CFRunLoopTimerRef CFRunLoopTimerCreateWithHandler(CFAllocatorRef allocator, CFAbsoluteTime fireDate, CFTimeInterval interval, CFOptionFlags flags, CFIndex order, void (^block)(CFRunLoopTimerRef timer));
    impl CFRunLoopRuntime {

        pub fn new() -> Self {
            let runtime = Runtime::new().unwrap();
            let wrapper = Arc::new(runtime);

            extern "C" fn callback(
                _fdref: CFFileDescriptorRef,
                _callback_types: usize,
                info: *mut c_void,
            ) {
                let runtime:&Runtime = unsafe{
                    &*(info as *const Runtime)
                };

                runtime.poll_with(Some(Duration::ZERO));

                let remaining_tasks = runtime.run();
                // if !remaining_tasks {
                //     CFRunLoop::get_current().stop()
                // }
                unsafe {
                    CFFileDescriptorEnableCallBacks(_fdref, kCFFileDescriptorReadCallBack)
                }
            }

            extern "C" fn observer(observer: CFRunLoopObserverRef, activity: CFRunLoopActivity, info: *mut c_void) {
                let timer = unsafe {
                    CFRunLoopTimer::from_void(info).as_concrete_TypeRef()
                };
                let weakRef = unsafe {
                    let mut context = CFRunLoopTimerContext{
                        version: 0,
                        info: null_mut(),
                        retain: None,
                        release: None,
                        copyDescription: None,
                    };
                    CFRunLoopTimerGetContext(
                        timer, &mut context);
                    let ptr = context.info as *const Runtime;
                    Weak::from_raw(ptr)
                };
                let runtime = weakRef.upgrade().unwrap();
                if activity == kCFRunLoopBeforeWaiting || activity == kCFRunLoopEntry || activity == kCFRunLoopBeforeTimers {
                    if let Some(timeout) = runtime.current_timeout() {
                        unsafe {

                            CFRunLoopTimerSetNextFireDate(
                                timer,
                                timeout.as_secs() as CFTimeInterval + timeout.subsec_nanos() as CFTimeInterval * 1_000_000_000.0
                            );
                        }
                    } else {
                        unsafe {

                            CFRunLoopTimerSetNextFireDate(
                                timer,
                                Duration::MAX.as_secs() as CFTimeInterval
                            );
                        }
                    }

                }
                let _ = weakRef.into_raw();
                return ();
            }

            extern "C" fn timer(timer: CFRunLoopTimerRef, info: *mut c_void) {
                let weakRef = unsafe {
                    let ptr = info as *const Runtime;
                    Weak::from_raw(ptr)
                };
                let runtime = weakRef.upgrade().unwrap();
                // Weak::from_raw()
                runtime.poll_with(Some(Duration::ZERO));

                let remaining_tasks =  runtime.run();
                // if !remaining_tasks {
                //     CFRunLoop::get_current().stop()
                // }
                let _ = weakRef.into_raw();
            }

            extern "C" fn retain(info: * const c_void) -> * const c_void {

                let ptr = unsafe {
                    Weak::from_raw(info as * const Runtime)
                };

                let cloned = ptr.clone();
                return ptr.into_raw() as *const c_void
            }

            extern "C" fn release(info: * const c_void) {
                unsafe {
                    Weak::from_raw(info);
                }
            }

            extern "C" fn cf_retain(info: * const c_void) -> * const c_void {

                unsafe {

                    CFRetain(CFType::from_void(info)
                        .as_CFTypeRef())

                }
            }

            extern "C" fn cf_release(info: * const c_void) {

                unsafe {
                    CFRelease(CFType::from_void(info)
                        .as_CFTypeRef())
                };
            }

            let a = CFFileDescriptorContext {
                version: 0,
                info: unsafe {
                    let v1 = Arc::downgrade(&wrapper);

                    v1.into_raw() as * mut c_void
                },
                retain: Some(retain),
                release: Some(release),
                copyDescription: None,
            };

            let fd_source =
                CFFileDescriptor::new(wrapper.as_raw_fd(), false, callback, Some(&a)).unwrap();

            let source = fd_source.to_run_loop_source(0).unwrap();
            // CFRunLoopObserverRef::
            let mut timer_context = CFRunLoopTimerContext{
                version: 0,
                info: unsafe {
                    let v1 = Arc::downgrade(&wrapper);

                    v1.into_raw() as * mut c_void
                },
                retain: Some(retain),
                release: Some(release),
                copyDescription: None,
            };
            let run_loop_timer = CFRunLoopTimer::new(
                unsafe { CFAbsoluteTimeGetCurrent() } + 1000.0,
                10.0,
                0, 0,
                timer,
                &mut timer_context,
            );

            let run_loop_observer = unsafe {
                let mut  a = CFRunLoopObserverContext{
                    version: 0,
                    info: run_loop_timer.as_CFType().to_void().cast_mut(),
                    retain: Some(cf_retain),
                    release: Some(cf_release),
                    copyDescription: None,
                };
                let reff = CFRunLoopObserverCreate(
                    null(), kCFRunLoopAllActivities, 1, 0, observer, &mut a
                );
                CFRunLoopObserver::wrap_under_create_rule(reff)
            };
            // let timer = CFRunLoopTimerCreateWithHandler(null(), 0, 1,0,0, &timerBlock);
            CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopDefaultMode });
            CFRunLoop::get_current().add_observer(&run_loop_observer, unsafe { kCFRunLoopDefaultMode});
            CFRunLoop::get_current().add_timer(&run_loop_timer, unsafe { kCFRunLoopDefaultMode });
            Self { runtime:wrapper, fd_source, timer: run_loop_timer, observer: run_loop_observer }
        }

        pub fn block_on<F: Future>(&self, future: F) -> F::Output {
            self.runtime.enter(|| {
                let mut result = None;
                unsafe {
                    self.runtime
                        .spawn_unchecked(async { result = Some(future.await) })
                }
                    .detach();
                loop {
                    // self.runtime.poll_with(Some(Duration::ZERO));

                    let remaining_tasks = self.runtime.run();
                    if let Some(result) = result.take() {
                        break result;
                    }

                    let timeout = if remaining_tasks {
                        Some(Duration::ZERO)
                    } else {
                        self.runtime.current_timeout()
                    };
                    self.fd_source
                        .enable_callbacks(kCFFileDescriptorReadCallBack);
                    CFRunLoop::run_in_mode(
                        unsafe { kCFRunLoopDefaultMode },
                        timeout.unwrap_or(Duration::MAX),
                        true,
                    );
                }
            })
        }
    }

    impl Drop for CFRunLoopRuntime {
        fn drop(&mut self) {
            self.fd_source.invalidate();
            unsafe {

                CFRunLoopTimerInvalidate(self.timer.as_concrete_TypeRef());
                CFRunLoopObserverInvalidate(self.observer.as_concrete_TypeRef());
            }
        }
    }
    let runtime = CFRunLoopRuntime::new();

    runtime.block_on(async {
        compio::runtime::time::sleep(Duration::from_secs(1)).await;

        let event = Event::new();
        let handle = Arc::new(Mutex::new(Some(event.handle())));
        let run_loop = CFRunLoop::get_current();
        let block = StackBlock::new(move || {
            handle.lock().unwrap().take().unwrap().notify();
            unsafe {
                CFRunLoopStop(run_loop.as_concrete_TypeRef());
            }
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
    });
}

#[test]
#[cfg(
    all(
        target_vendor = "apple",
        any(target_os = "macos", target_os = "ios")
    )
)]
fn test_loop() {
    apple_run_loop()
}