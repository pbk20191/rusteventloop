mod winloop;
mod osxloop;
mod gloop;

fn main() {
    #[cfg(target_os = "windows")]
    {
        extern crate native_windows_gui as nwg;
        use nwg::NativeUi;
        nwg::init().expect("Failed to init Native Windows GUI");
        let _app = window_app::BasicApp::build_ui(Default::default()).expect("Failed to build UI");
        winloop::message_loop();

    };
   
    // tokio::runtime::Builder::new_multi_thread()
    //     .enable_all()
    //     .build().unwrap().block_on(async {
    //     
    // });
    #[cfg(target_os = "macos") ]
    osxloop::apple_run_loop(async {
        use cacao::appkit::{App, AppDelegate};
        use cacao::appkit::window::Window;
        use block2::{Block, StackBlock};
        use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoopRef};
        use core_foundation::string::CFStringRef;
        use core_foundation::runloop::CFRunLoopGetCurrent;
        
        #[derive(Default)]
        struct BasicApp {
            window: Window
        }

        impl AppDelegate for BasicApp {
            fn did_finish_launching(&self) {
                self.window.set_minimum_content_size(400., 400.);
                self.window.set_title("Hello World!");
                self.window.show();
            }

            fn should_terminate_after_last_window_closed(&self) -> bool {
                true
            }
        }
        let block = StackBlock::new(move || {
            App::new("com.hello.world", BasicApp::default()).run();
        });
        extern "C" {
            fn CFRunLoopPerformBlock(rl: CFRunLoopRef, mode: CFStringRef, block: &Block<dyn Fn()>);
        }
        // runloop::CFRunLoop::get_current()
        unsafe {
            CFRunLoopPerformBlock(CFRunLoopGetCurrent(), kCFRunLoopDefaultMode, &block);
        }
    });
    #[cfg(target_os = "linux")]
    {

        let _code = gloop::glib_context(async {
            use std::time::Duration;
            use gtk4::{
                gio::prelude::{
                    ApplicationExt, ApplicationExtManual, 
                },
                prelude::{
                    ButtonExt, GtkWindowExt
                },
                ApplicationWindow
            };
            use compio::runtime::event::Event;
    
            let event = Event::new();
            let mut handle_ref = Some(event.handle());
            let exit_code_box = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));

            let exit_atomic_ref = exit_code_box.clone();
            let source = glib::timeout_source_new(Duration::ZERO, None, glib::Priority::DEFAULT, move || {
                
                let app = gtk4::Application::builder()
                    .application_id("com.example.gtk.rust")
                    .build();
                
                app.connect_activate(move |app| {
                    let button = gtk4::Button::builder()
                        .label("Press me!")
                        .margin_top(12)
                        .margin_bottom(12)
                        .margin_start(12)
                        .margin_end(12)
                        .build();
    
                    button.connect_clicked(|button| {
                        button.set_label("Hello World!");
                        compio::runtime::spawn(async move {
    
                            compio::runtime::time::sleep(Duration::from_secs(1)).await;
                            compio::runtime::time::sleep(Duration::from_secs(1)).await;
                            println!("beep")
                        }).detach();
    
                    });
                    let window = ApplicationWindow::builder()
                        .application(app)
                        .title("My GTK App")
                        .child(&button)
                        .build();
                    window.present();
                    
                });
    
    
                let code = app.run();
                exit_atomic_ref.store(code.value(), std::sync::atomic::Ordering::Relaxed);
                if let Some(handle) = handle_ref.take() {
                    handle.notify();
                }
                return glib::ControlFlow::Break
            });
            source.attach(Some(&glib::MainContext::ref_thread_default()));
            event.wait().await;
            drop(source);
            exit_code_box.load(std::sync::atomic::Ordering::Relaxed)
        });
    }
}


#[cfg(target_os = "windows")]
mod window_app {
    extern crate native_windows_gui as nwg;
    extern crate native_windows_derive as nwd;
    use nwd::NwgUi;

    #[derive(Default, NwgUi)]
    pub struct BasicApp {
        #[nwg_control(size: (300, 115), position: (300, 300), title: "Basic example", flags: "WINDOW|VISIBLE")]
        #[nwg_events( OnWindowClose: [BasicApp::say_goodbye] )]
        window: nwg::Window,

        #[nwg_control(text: "Heisenberg", size: (280, 25), position: (10, 10))]
        name_edit: nwg::TextInput,

        #[nwg_control(text: "Say my name", size: (280, 60), position: (10, 40))]
        #[nwg_events( OnButtonClick: [BasicApp::say_hello] )]
        hello_button: nwg::Button
    }

    impl BasicApp {

        fn say_hello(&self) {
            nwg::simple_message("Hello", &format!("Hello {}", self.name_edit.text()));
        }

        fn say_goodbye(&self) {
            nwg::simple_message("Goodbye", &format!("Goodbye {}", self.name_edit.text()));
            nwg::stop_thread_dispatch();
        }

    }
}
