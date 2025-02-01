
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
        
    })
}
