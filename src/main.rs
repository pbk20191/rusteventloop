

mod winloop;
mod osxloop;
mod gloop;

fn main() {

    #[cfg(target_os = "windows")]
    winloop::message_queue(async {

    });
    
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
    gloop::glib_context(async {
        use std::time::Duration;

        use gtk4::{
            gio::prelude::{
                ApplicationExt, ApplicationExtManual
            },
            prelude::{
                ButtonExt, GtkWindowExt
            },
            ApplicationWindow
        };
        let handle = glib::spawn_future_local( async move {

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
        
                    }).detach();
                    
                });
                let window = ApplicationWindow::builder()
                .application(app)
                .title("My GTK App")
                .child(&button)
                .build();
                window.present();
            });
            app.run();
        });
        handle.await.unwrap();
    })
}
