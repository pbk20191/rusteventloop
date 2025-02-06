use std::future::Future;

#[cfg(target_os = "windows")]
pub(crate) fn message_queue<F: Future>(future: F) -> F::Output {
    use crate::winloop::job_specialization::FutureJob;

    message_queue_impl(FutureJob{ variable: None }, future)
}

#[cfg(target_os = "windows")]
pub(crate) fn message_loop() -> i32 {
    use crate::winloop::job_specialization::NeverJob;

    message_queue_impl(NeverJob{}, ())
}

#[test]
#[cfg(target_os = "windows")]
pub(self) fn test_window() {
    use std::sync::Mutex;
    use std::time::Duration;
    use compio::runtime::event::{Event, EventHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        KillTimer, MessageBoxW, SetTimer, MB_OK
    };
    
    message_queue(async{
        compio::runtime::time::sleep(Duration::from_secs(1)).await;

        static GLOBAL_EVENT: Mutex<Option<EventHandle>> = Mutex::new(None);

        let event = Event::new();
        *GLOBAL_EVENT.lock().unwrap() = Some(event.handle());

        unsafe extern "system" fn timer_callback(hwnd: HWND, _msg: u32, id: usize, _dwtime: u32) {
            let handle = GLOBAL_EVENT.lock().unwrap().take().unwrap();
            handle.notify();
            KillTimer(hwnd, id);
            let a = 'A';
            let title: Vec<u16> = "Rust MessageBox".encode_utf16().chain(std::iter::once(0)).collect();

            let message: Vec<u16> = "Hello, Windows API!".encode_utf16().chain(std::iter::once(0)).collect();

            compio::runtime::spawn(async {
                compio::runtime::time::sleep(Duration::from_secs(5)).await;
            }).detach();
            let a = MessageBoxW(0, title.as_ptr(),message.as_ptr(), MB_OK);
        }

        unsafe {
            SetTimer(0, 0, 1, Some(timer_callback));
        }

        event.wait().await;
    })
}


#[cfg(target_os = "windows")]
fn message_queue_impl<J: job_specialization::JobTrait>(job: J, input: J::Input) -> J::Output {
    use std::{mem::MaybeUninit, time::Duration};
    use compio::driver::AsRawFd;
    use compio::runtime::Runtime;
    use libffi::high::{
        Closure3, Closure4
    };

    use windows_sys::Win32::{
        Foundation::{HANDLE, HWND, LPARAM, LRESULT, WAIT_FAILED, WPARAM},
        System::Threading::{
            GetCurrentThreadId, INFINITE
        },
        UI::WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetAncestor, IsDialogMessageW, KillTimer,
            MsgWaitForMultipleObjectsEx, PeekMessageW, SetTimer, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, GA_ROOT, HHOOK, MSG, MSGF_DIALOGBOX,
            MSGF_MENU, MSGF_MESSAGEBOX, MSGF_SCROLLBAR, MWMO_ALERTABLE, MWMO_INPUTAVAILABLE,
            PM_REMOVE, QS_ALLINPUT, USER_TIMER_MAXIMUM, WH_MSGFILTER, WM_QUIT
        },
    };
    
    struct OwnedTimer {
        native: usize
    }

    impl Drop for OwnedTimer {
        
        fn drop(&mut self) {
            unsafe {
                KillTimer(0, self.native);
            }
        }
    }
    
    
    struct OwnedWindowHook {
        native: HHOOK
    }
    
    impl Drop for OwnedWindowHook {
        fn drop(&mut self) {
            unsafe {
                UnhookWindowsHookEx(self.native);
            }
        }
    }
    let runtime = Runtime::new().unwrap();
    let runtime_ref = &runtime;


    runtime_ref.enter( move || {
        use std::cell::RefCell;
        use std::mem::transmute;
        use windows_sys::Win32::UI::WindowsAndMessaging::{MSGF_DIALOGBOX, MSGF_MENU, MSGF_MESSAGEBOX, MSGF_SCROLLBAR, TIMERPROC};

        let timer =  {
            let handle = unsafe{
                SetTimer(0, 0, 1000, None)
            };
            if handle == 0 {
                panic!("{:?}", std::io::Error::last_os_error());
            }

            OwnedTimer{ native:handle }
        };
        let timer_ref = &timer;
        let timer_proc :RefCell<TIMERPROC> = RefCell::new(None);
        let timer_proc_ref = &timer_proc;
        let timer_cb = move |hwnd: HWND, _msg: u32, id: usize, _dwtime: u32| {

            runtime_ref.poll_with(Some(Duration::ZERO));
            runtime_ref.run();
            
            match runtime_ref.current_timeout() {
                None => unsafe {
                    SetTimer(0, timer_ref.native, USER_TIMER_MAXIMUM, *timer_proc_ref.borrow());
                }
                Some(timeout) => unsafe {
                    SetTimer(0, timer_ref.native, timeout.as_millis() as u32, *timer_proc_ref.borrow());

                }
            }
        };
        let timer_closure = Closure4::new(&timer_cb);

        unsafe {

            timer_proc.replace(Some(transmute(*timer_closure.code_ptr())));

            SetTimer(0, timer.native, 1000, Some(transmute(*timer_closure.code_ptr())));
        };

        let message_proc = move |code: i32, w_param: WPARAM, l_param: LPARAM| -> LRESULT {

            match code as u32 {
                MSGF_DIALOGBOX | MSGF_MESSAGEBOX => {
                    runtime_ref.poll_with(Some(Duration::ZERO));
                    runtime_ref.run();
                    match runtime_ref.current_timeout() {
                        None =>  {
                            // this procedure is called rapidly
                            // so it would be better to do nothing, rather than updating timer to distant future
                        }
                        Some(timeout) => unsafe {
                            SetTimer(0, timer_ref.native, timeout.as_millis() as u32, *timer_proc_ref.borrow());
                        }
                    }

                }
                MSGF_MENU => {}
                MSGF_SCROLLBAR => {}
                _ => {}
            }
            unsafe {
                CallNextHookEx(0, code, w_param, l_param)
            }
        };
        let _hook_store = Closure3::new(&message_proc);
        let _hook =  {
            let store = &_hook_store;
            let value = unsafe {
                SetWindowsHookExW(WH_MSGFILTER, Some(transmute(*store.code_ptr())), 0, GetCurrentThreadId())
            };
            if value == 0 {
                panic!("{:?}", std::io::Error::last_os_error());
            }
            OwnedWindowHook{ native: value }
        };
        let mut my_job = job;
        my_job.spawn(input, runtime_ref);
        
        use std::ptr::null;
        'outer: loop {
            runtime_ref.poll_with(Some(Duration::ZERO));
            let remaining_tasks = runtime_ref.run();
            if let Some(result) = my_job.check(null()) {
                break result;
            }
            let timeout = if remaining_tasks {
                Some(Duration::ZERO)
            } else {
                runtime_ref.current_timeout()
            };
            match timeout {
                Some(timeout) => {
                    unsafe {
                        SetTimer(
                            0, timer.native, timeout.as_millis() as u32, *timer_proc.borrow()
                        )
                    };
                }
                None => {
                    unsafe {
                        SetTimer(0, timer.native, USER_TIMER_MAXIMUM, *timer_proc.borrow());
                    }
                }
            }

            let handle = runtime_ref.as_raw_fd() as HANDLE;
            let res = unsafe {
                MsgWaitForMultipleObjectsEx(
                    1,
                    &handle,
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_ALERTABLE | MWMO_INPUTAVAILABLE,
                )
            };
            if res == WAIT_FAILED {
                panic!("{:?}", std::io::Error::last_os_error());
            }
            let mut msg = MaybeUninit::uninit();
            while unsafe{ PeekMessageW(msg.as_mut_ptr(), 0, 0, 0, PM_REMOVE)} != 0 {
                let msg = unsafe { msg.assume_init() };
                if msg.message == WM_QUIT {
                    let k = &msg;
                    use std::ffi::c_void;

                    let k2 = (k as *const MSG) as *const c_void;
                    if let Some(value) = my_job.check(k2) {
                        break 'outer value;
                    }
                }
                unsafe {
                    if IsDialogMessageW(GetAncestor(msg.hwnd, GA_ROOT), &msg) == 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
    })
}


#[cfg(target_os = "windows")]
mod job_specialization {
    use compio::runtime::Runtime;
    use std::ffi::c_void;
    use std::future::Future;

    pub trait JobTrait {

        type Output;
        type Input;
        // fn check() -> bool;

        fn spawn(&mut self, input:Self::Input, runtime: &Runtime);

        fn check(&mut self, msg: *const c_void) -> Option<Self::Output>;

    }

    pub struct FutureJob <F:Future>{


        pub variable:Option<F::Output>,
    }

    pub struct NeverJob {

    }

    impl JobTrait for NeverJob {

        type Output = i32;
        type Input = ();

        fn spawn(&mut self, _input: Self::Input, _runtime: &Runtime) {

        }

        fn check(&mut self, msg: *const c_void) -> Option<Self::Output> {
            if msg.is_null() {
                return None;
            }
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                MSG, WM_QUIT
            };

            let message = &unsafe { *(msg as *const MSG) };
            if message.message == WM_QUIT {
                Some(message.wParam as i32)
            } else {
                None
            }
        }
    }

    impl <F:Future> JobTrait for FutureJob<F> {
        type Output = F::Output;
        type Input = F;

        fn spawn(&mut self, input: Self::Input, runtime: &Runtime) {
            unsafe {
                runtime.spawn_unchecked(async move {
                    self.variable = Some(input.await);
                }).detach()
            }
        }

        fn check(&mut self, _msg: *const c_void) -> Option<Self::Output> {
            if let Some(variable) = self.variable.take() {
                Some(variable)
            } else {
                None
            }
        }


    }
}
