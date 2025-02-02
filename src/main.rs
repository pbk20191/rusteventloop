
mod winloop;
mod osxloop;
mod gloop;

fn main() {

    #[cfg(target_os = "windows")]
    winloop::message_queue(async {

    });
    
    #[cfg(target_os = "macos") ]
    osxloop::apple_run_loop(async {
       
       
       
    });
    #[cfg(target_os = "linux")]
    gloop::glib_context(async {
        use std::time::Duration;

        use gtk4::{
            gio::prelude::{
                ApplicationExtManual, ApplicationExt
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
