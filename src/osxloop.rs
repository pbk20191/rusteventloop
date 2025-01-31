#[cfg(target_os = "macos")]
#[test]
pub(crate) fn cf_run_loop() {
    use std::{
        future::Future,
        os::raw::c_void,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use block2::{Block, StackBlock};
    use compio::driver::AsRawFd;
    use compio::runtime::{Runtime, event::Event};
    use core_foundation::{
        base::TCFType,
        filedescriptor::{CFFileDescriptor, CFFileDescriptorRef, kCFFileDescriptorReadCallBack},
        runloop::{CFRunLoop, CFRunLoopRef, CFRunLoopStop, kCFRunLoopDefaultMode},
        string::CFStringRef,
    };

    struct CFRunLoopRuntime {
        runtime: Runtime,
        fd_source: CFFileDescriptor,
    }

    impl CFRunLoopRuntime {
        pub fn new() -> Self {
            let runtime = Runtime::new().unwrap();

            extern "C" fn callback(
                _fdref: CFFileDescriptorRef,
                _callback_types: usize,
                _info: *mut c_void,
            ) {

            }

            let fd_source =
                CFFileDescriptor::new(runtime.as_raw_fd(), false, callback, None).unwrap();
            let source = fd_source.to_run_loop_source(0).unwrap();

            CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopDefaultMode });

            Self { runtime, fd_source }
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
                    self.runtime.poll_with(Some(Duration::ZERO));

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

    let runtime = CFRunLoopRuntime::new();

    runtime.block_on(async {
        compio_runtime::time::sleep(Duration::from_secs(1)).await;

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