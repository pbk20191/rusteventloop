use std::future::Future;

#[cfg(not(any(windows, target_os = "macos", target_os = "ios", target_os = "android")))]
pub(crate) fn glib_context<F: Future>(future: F) -> F::Output {
    use std::cell::RefCell;
    use std::ptr::null_mut;
    use std::time::Duration;
    use compio::driver::AsRawFd;
    use compio::runtime::Runtime;
    use glib::ffi::{g_source_add_unix_fd, g_source_query_unix_fd};
    
    use glib::translate::FromGlib;
    
    use glib::{
        ffi::gpointer, translate::IntoGlib, ControlFlow,
        IOCondition,
        MainContext
    };

    struct DriverSource {
        runtime:Runtime,
        tag:RefCell<gpointer>
    }

    impl DriverSource {
        
        fn new() -> Self {
            let runtime = Runtime::new().unwrap();
            return Self {
                runtime,
                tag: RefCell::new(null_mut())
            }
        }
    }

    impl source_util::SourceFuncs for DriverSource {

        fn prepare(&self, _source:&glib::Source) -> (bool, Option<u32>) {
            if self.runtime.run() {
                return (true, None)
            }
            if let Some(timeout) = self.runtime.current_timeout() {
                if (timeout == Duration::ZERO) {
                    // needs to call through check to call runtime::poll
                    return (false, Some(0))
                }
                return (false, Some(timeout.as_millis() as u32))

            } else {
                
                return (false, None)
            }
        }

    
        fn check(&self, source:&glib::Source) -> bool {
            let condition =  {
                let k = self.tag.borrow();
                unsafe {
                    IOCondition::from_glib(
                        g_source_query_unix_fd(source.as_ptr(), *k)
                    )
                }
            };
            self.runtime.poll_with(Some(Duration::ZERO));
            if condition == IOCondition::IN {
                return true
            }
            if self.runtime.current_timeout() == Some(Duration::ZERO) {
                return true
            }
            return false
        }

        fn dispatch(&self, _source:&glib::Source) -> glib::ControlFlow {
            self.runtime.run();
            return ControlFlow::Continue;
        }
    

    }

    let source = source_util::new_source(DriverSource::new());
    let data:&DriverSource = source_util::source_get(&source);
    {
        let mut mut_tag = data.tag.borrow_mut();
    
        *mut_tag = unsafe {
            let k = g_source_add_unix_fd(source.as_ptr(), data.runtime.as_raw_fd(), IOCondition::IN.into_glib());
            k
        };
    }

    let runtime = &data.runtime;

    let ctx = MainContext::default();
    return runtime.enter(|| {
        ctx.with_thread_default( || {
            let mut result = None;
            unsafe {
                runtime
                    .spawn_unchecked(async { 
                        
                        result = Some(future.await); 
                        let ctx = MainContext::ref_thread_default();
                        ctx.wakeup();
                    })
            }.detach();
            let _id = source.attach(Some(&ctx));
            loop {
                if let Some(result) = result.take() {
                    source.destroy();
                    break result;
                }
                ctx.iteration(true);
            }
        }).expect("Can not acquire context")
    });
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

#[cfg(not(any(windows, target_os = "macos", target_os = "ios", target_os = "android")))]
mod source_util {
    use std::{ffi::c_int, mem, ptr};

    use glib::{
        ffi::{
            g_source_new, GSource, GSourceFunc, GSourceFuncs
        }, translate::{
            from_glib_borrow, from_glib_full, Borrowed, IntoGlib, ToGlibPtr
        }, ControlFlow, Source
    };


    pub trait SourceFuncs {
        fn check(&self, _source:&Source) -> bool {
            false
        }
    
        fn dispatch(&self, source:&Source) -> ControlFlow;
        fn prepare(&self, source:&Source) -> (bool, Option<u32>);
    }
    
    #[repr(C)]
    struct SourceData<T> {
        _source: GSource,
        funcs: Box<GSourceFuncs>,
        data: T,
    }
    
    pub fn new_source<T: SourceFuncs>(data: T) -> Source {
        unsafe {
            let mut funcs: GSourceFuncs = mem::zeroed();
            funcs.prepare = Some(prepare::<T>);
            funcs.check = Some(check::<T>);
            funcs.dispatch = Some(dispatch::<T>);
            funcs.finalize = Some(finalize::<T>);
            let mut funcs = Box::new(funcs);
            let source = g_source_new(&mut *funcs, mem::size_of::<SourceData<T>>() as u32);
            ptr::write(&mut (*(source as *mut SourceData<T>)).data, data);
            ptr::write(&mut (*(source as *mut SourceData<T>)).funcs, funcs);
            from_glib_full(source)
        }
    }
    
    pub fn source_get<T: SourceFuncs>(source: &Source) -> &T {
        unsafe {
            &(*(<Source as ToGlibPtr<'_, *mut GSource>>::to_glib_none(source).0
                as *const SourceData<T>))
                .data
        }
    }
    
    unsafe extern "C" fn check<T: SourceFuncs>(source: *mut GSource) -> c_int {
        let object = source as *mut SourceData<T>;
        let a:Borrowed<Source> = from_glib_borrow(source);
        bool_to_int((*object).data.check(a.as_ref()))
    }
    
    unsafe extern "C" fn dispatch<T: SourceFuncs>(source: *mut GSource, _callback: GSourceFunc, _user_data: *mut libc::c_void)
        -> c_int
    {
        let object = source as *mut SourceData<T>;
        let a:Borrowed<Source> = from_glib_borrow(source);
        let control = (*object).data.dispatch(a.as_ref());
        control.into_glib()
    }
    
    unsafe extern "C" fn finalize<T: SourceFuncs>(source: *mut GSource) {
        // TODO: needs a bomb to abort on panic
        let source = source as *mut SourceData<T>;
        ptr::read(&(*source).funcs);
        ptr::read(&(*source).data);
    }
    
    extern "C" fn prepare<T: SourceFuncs>(source: *mut GSource, timeout: *mut c_int) -> c_int {
        let object = source as *mut SourceData<T>;
        let a:Borrowed<Source> = unsafe { from_glib_borrow(source) };
        let (result, source_timeout) = unsafe { (*object).data.prepare(a.as_ref()) };
        if let Some(source_timeout) = source_timeout {
            unsafe { *timeout = source_timeout as i32; }
        }
        bool_to_int(result)
    }
    
    fn bool_to_int(boolean: bool) -> c_int {
        boolean.into()
    }
}