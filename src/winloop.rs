

#[test]
#[cfg(target_os = "windows")]
pub(crate) fn message_queue() {
    use std::{future::Future, mem::MaybeUninit, sync::Mutex, time::Duration};
    use std::cell::{Cell, OnceCell, RefCell, UnsafeCell};
    use std::sync::{ Arc, Weak };
    use std::ptr::{null, null_mut};
    use compio::driver::AsRawFd;
    use compio::runtime::{
        Runtime,
        event::{Event, EventHandle},
    };
    use windows_sys::core::PCWSTR;
    use windows_sys::Win32::{
        Foundation::{HANDLE, HWND, WAIT_FAILED, WPARAM, LPARAM, LRESULT},
        System::Threading::{
            INFINITE, GetCurrentThreadId
        },
        UI::WindowsAndMessaging::{
            DispatchMessageW, KillTimer, MWMO_ALERTABLE, MWMO_INPUTAVAILABLE,
            MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, SetTimer,
            TranslateMessage, SetWindowsHookExW, WH_MSGFILTER, CallNextHookEx, MSG, CallMsgFilterW, MSGF_USER,
            HHOOK, UnhookWindowsHookEx, MessageBoxW,
            MSGF_DIALOGBOX, MSGF_MENU, MB_OK,  MSGF_MESSAGEBOX, MSGF_SCROLLBAR
        },
    };
    
    thread_local! {
        static RUNTIMEREF : RefCell<Weak<Runtime>> = RefCell::new(Weak::new());
    }

    extern "system" fn message_proc(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {

        if (code == MSGF_DIALOGBOX as i32) || code == MSGF_MESSAGEBOX as i32 {
            RUNTIMEREF.with_borrow(|variable| {
               if let Some(runtime) = variable.upgrade() {
                   runtime.poll_with(Some(Duration::ZERO));
                   runtime.run();
               }
            });

        }
        if (code == MSGF_MENU as i32) {

        }
        if code == MSGF_SCROLLBAR as i32 {

        }
        unsafe {
            CallNextHookEx(0, code, w_param, l_param)
        }
    }

    struct MQRuntime {
        runtime: Arc<Runtime>,
        hook:HHOOK,
    }

    impl MQRuntime {

        pub fn new() -> Self {
            let hook = unsafe {
                SetWindowsHookExW(WH_MSGFILTER, Some(message_proc), 0, GetCurrentThreadId())
            };
            if hook == 0 {
                panic!("{:?}", std::io::Error::last_os_error());
            }
            let runtime = Arc::new(Runtime::new().unwrap());

            Self {
                runtime,
                hook,
            }
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
                    RUNTIMEREF.set( Arc::downgrade(&self.runtime));
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
                    let timeout = match timeout {
                        Some(timeout) => timeout.as_millis() as u32,
                        None => INFINITE,
                    };
                    let handle = self.runtime.as_raw_fd() as HANDLE;
                    let res = unsafe {
                        MsgWaitForMultipleObjectsEx(
                            1,
                            &handle,
                            timeout,
                            QS_ALLINPUT,
                            MWMO_ALERTABLE | MWMO_INPUTAVAILABLE,
                        )
                    };
                    if res == WAIT_FAILED {
                        panic!("{:?}", std::io::Error::last_os_error());
                    }

                    let mut msg = MaybeUninit::uninit();
                    let res = unsafe { PeekMessageW(msg.as_mut_ptr(), 0, 0, 0, PM_REMOVE) };
                    if res != 0 {
                        let msg = unsafe { msg.assume_init() };
                        unsafe {
                            if CallMsgFilterW(&msg, MSGF_USER as i32) == 0 {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }
                    }
                    RUNTIMEREF.set(Weak::new());

                }
            })
        }
    }

    impl Drop for MQRuntime {
        fn drop(&mut self) {
            let hook = self.hook;
            unsafe {
                UnhookWindowsHookEx(hook);
            }
        }
    }
    let runtime = MQRuntime::new();


    runtime.block_on(async {
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

            let a = MessageBoxW(0, title.as_ptr(),message.as_ptr(), MB_OK);
        }

        unsafe {
            SetTimer(0, 0, 1, Some(timer_callback));
        }

        event.wait().await;
    });
}
