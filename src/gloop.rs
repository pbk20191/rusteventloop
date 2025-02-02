use std::future::Future;

#[cfg(not(any(windows, target_os = "macos", target_os = "ios", target_os = "android")))]
pub(crate) fn glib_context<F: Future>(future: F) -> F::Output {
    use std::{future::Future, time::Duration};
    use std::sync::Arc;
    use std::os::raw::c_uint;
    use std::sync::Weak;
    use std::ffi::c_int;
    use compio::driver::AsRawFd;
    use compio::runtime::Runtime;
    use glib::{
        ffi::{
            g_get_monotonic_time, g_source_set_callback_indirect, g_source_set_ready_time, gpointer, GSourceCallbackFuncs, GSourceFunc,
            g_source_new, gboolean, GSource, GSourceFuncs
        }, translate::{
            IntoGlib, from_glib_full, mut_override
        }, ControlFlow,
        subclass::shared::RefCounted,
        {unix_fd_source_new, Priority, Source},
        IOCondition,
        MainContext
    };

    struct GLibRuntime {
        runtime: Arc<Runtime>,
        ctx: MainContext,
        source: Source,
    }

    unsafe extern "C" fn g_prepare(source: *mut GSource, timeout: *mut c_int) -> gboolean {
       // return 1;
        let pointer = (*source).callback_data as (*const Runtime);
        let a = &(*pointer);
        // a.poll_with(Some(Duration::ZERO));
        // a.run();
        if let Some(duration) = a.current_timeout() {
            if duration == Duration::ZERO {
                return 1;
            }
           *timeout = duration.as_millis() as c_int;
            g_source_set_ready_time(source, g_get_monotonic_time() + (duration.as_micros() as i64));
            return 0;
        } else {
            *timeout = -1;
            return 0;
        }
    }

    unsafe extern "C" fn g_check(source: *mut GSource) -> gboolean {
        let pointer = (*source).callback_data as (*const Runtime);
        let a = &(*pointer);
        if Some(Duration::ZERO) == a.current_timeout() {
            a.poll_with(Some(Duration::ZERO));
            return 1
        } else {
            return 0
        }
    }

    unsafe extern "C" fn g_dispatch(source: *mut GSource, callback: GSourceFunc, user_data:gpointer) -> gboolean {
        let pointer = (*source).callback_data as (*const Runtime);
        let a = &(*pointer);
        if let Some(cb) = callback {
            return cb(user_data);
        } else {
            a.run();
            if let Some(duration) = a.current_timeout() {
                g_source_set_ready_time(source, g_get_monotonic_time() + (duration.as_micros() as i64))
            }
        }
        return ControlFlow::Continue.into_glib();
    }

    unsafe extern "C" fn g_finalize(source: *mut GSource) {

        // let ptr = (*source).callback_data as *const Runtime;
        // Arc::decrement_strong_count(ptr);
    }

    unsafe extern "C" fn g_retain(data:gpointer) {
        let ptr = data as *const Runtime;
        Arc::increment_strong_count(ptr);
    }
    unsafe extern "C" fn g_release(data:gpointer) {
        let ptr = data as *const Runtime;
        Arc::decrement_strong_count(ptr);
    }

    // fn(gpointer, *mut GSource, *mut GSourceFunc, *mut gpointer)>,
    unsafe extern "C" fn g_get(
        data: gpointer, source: *mut GSource, cb:*mut GSourceFunc, result: *mut gpointer,
    ) {
        let ptr = data as *const Runtime;

    }

    fn create_file_source(
        runtime: &Arc<Runtime>,
        timer: Source,
    ) -> Source {
        struct UnsafeWrapper {
            wrapped: Weak<Runtime>
        }
        unsafe impl Send for UnsafeWrapper {}
        impl Clone for UnsafeWrapper {
            fn clone(&self) -> Self {
                UnsafeWrapper{ wrapped: self.wrapped.clone() }
            }
        }
        struct UnsafeSending {
            wrapped: *mut GSource,
        }
        unsafe impl Send for UnsafeSending {}
        let weak_runtime = Arc::downgrade(&runtime);
        let wrapped = UnsafeWrapper{ wrapped: weak_runtime };
        // let boxed = Box::new(timer);
        let ptr = UnsafeSending{ wrapped: timer.as_ptr() };
        let source = unix_fd_source_new(
            runtime.as_raw_fd(),
            IOCondition::IN,
            Some("what"),
            Priority::DEFAULT,
            move |_fd, _condition| {
                let moved = &ptr;
                let a = wrapped.clone();
                if let Some(runtime) = a.wrapped.upgrade() {

                    runtime.poll_with(Some(Duration::ZERO));
                    runtime.run();
                    if let Some(duration) = runtime.current_timeout() {
                        
                        // if let Some(source) = weak.upgrade() {
                        //     let stash = source.to_glib_none();
                        unsafe {
                            g_source_set_ready_time(moved.wrapped, g_get_monotonic_time() + duration.as_micros() as i64);
                        }
                        // }
                    }
                    ControlFlow::Continue
                } else {
                    ControlFlow::Break
                }
            });
        source.add_child_source(&timer);
        source
    }

    fn create_time_source(runtime: &Arc<Runtime>) -> Source {
        static SOURCEWHAT: GSourceFuncs = GSourceFuncs{
            prepare: Some(g_prepare),
            check: Some(g_check),
            dispatch: Some(g_dispatch),
            finalize: Some(g_finalize),
            closure_callback: None,
            closure_marshal: None,
        };

       let raw_ptr = unsafe { runtime.clone().into_raw() };
        let source = unsafe {
            let block_size = size_of::<GSource>();
            let g_source = g_source_new(
                mut_override(&SOURCEWHAT),
                (block_size) as c_uint
            );
            //Arc::increment_strong_count(raw_ptr);
            static CB_INFO: GSourceCallbackFuncs = GSourceCallbackFuncs{
                ref_: Some(g_retain),
                unref: Some(g_release),
                get: Some(g_get),
            };
            g_source_set_callback_indirect(
                g_source,
                raw_ptr as gpointer,
                mut_override(&CB_INFO),
            );
            // Arc::increment_strong_count(raw_ptr);

      //      g_source_set_can_recurse(g_source, 0);
            from_glib_full(g_source)
        };

        /*
                source: *mut GSource,
        func: GSourceFunc,
        data: gpointer,
        notify: GDestroyNotify,
        */
   //     let arc = unsafe {  Arc::from_raw(raw_ptr) };

        source
    }

    impl Drop for GLibRuntime {
        fn drop(&mut self) {
            self.source.destroy();
        }
    }

    impl GLibRuntime {
        pub fn new() -> Self {

            let runtime = Arc::new(Runtime::new().unwrap());
            let ctx = MainContext::default();
            let timer = create_time_source(&runtime);

            let file_source = create_file_source(&runtime, timer);
            Self { runtime, ctx, source: file_source }
        }

        pub fn block_on<F: Future>(&self, future: F) -> F::Output {
            self.runtime.enter(|| {
                let mut result = None;
                unsafe {
                    self.runtime
                        .spawn_unchecked(async { result = Some(future.await) })
                }
                    .detach();
                let id = self.source.attach(Some(&self.ctx));
                self.runtime.run();
                loop {
                    if let Some(result) = result.take() { 
                        id.remove();
                        break result;
                    }
                    self.ctx.iteration(true);
                }
            })
        }
    }
    
    let runtime = GLibRuntime::new();

    runtime.block_on(future)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "ios", target_os = "android")))]
#[test]
fn gtk_test() {

    use std::time::Duration;
    use compio::runtime::event::Event;
    
    glib_context(
        async {
            compio::runtime::time::sleep(Duration::from_secs(1)).await;  
            let event = Event::new();
            let handle = event.handle();
            let task = glib::spawn_future_local(async move {
                handle.notify();
            });
            event.wait().await;
            task.await.unwrap();
        }
    )
}